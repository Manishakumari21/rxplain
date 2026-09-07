use crate::diagnostics::ParsedError;
use crate::repair::RepairHistory;
use crate::repair::{RepairCandidate, patch_signature, rank_candidates};
use crate::verification::{self, IsolatedWorkspace, VerificationResult, VerifyMode};
use anyhow::Result;

/// Clear, JSON-stable outcome states for a repair run.
///
/// The string values are kept stable for existing consumers; the richer
/// conceptual states from the specification map onto these as documented on
/// each variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairStatus {
    /// No compiler diagnostics were produced at all.
    NoDiagnostic,
    /// The project already passes the oracle in isolation; nothing to repair.
    NothingToRepair,
    /// The failure could not be reproduced in an isolated copy, but the
    /// analysis continues in case the copy merely misses build state.
    IsolationCheckFailed,
    /// Dry run: candidates were generated but nothing was verified or applied.
    Preview,
    /// No candidate could be generated or verified; the error needs a person.
    HumanReviewRequired,
    /// Candidates were tried and failed verification in isolation.
    RepairAttempted,
    /// A single candidate passed verification and its patch set is ready.
    Verified,
    /// More than one candidate passed verification; a person must choose.
    MultipleVerifiedRepairs,
    /// A patch was generated but never verified (used for dry-run previews).
    PatchGenerated,
}

impl RepairStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RepairStatus::NoDiagnostic => "no_diagnostic",
            RepairStatus::NothingToRepair => "nothing_to_repair",
            RepairStatus::IsolationCheckFailed => "isolation_check_failed",
            RepairStatus::Preview => "preview",
            RepairStatus::HumanReviewRequired => "human_review_required",
            RepairStatus::RepairAttempted => "repair_attempted",
            RepairStatus::Verified => "repair_applied",
            RepairStatus::MultipleVerifiedRepairs => "multiple_verified",
            RepairStatus::PatchGenerated => "patch_generated",
        }
    }
}

/// Bounds for the controlled candidate search.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub max_attempts: u32,
    pub max_candidates: usize,
    pub max_alternative_checks: usize,
}

impl EngineConfig {
    pub fn from_env() -> Self {
        Self {
            max_attempts: env_u32("RXPLAIN_MAX_ATTEMPTS", 4),
            max_candidates: env_usize("RXPLAIN_MAX_CANDIDATES", 10),
            max_alternative_checks: env_usize("RXPLAIN_MAX_ALTERNATIVES", 5),
        }
    }
}

fn env_u32(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// Result of running a candidate through the isolated-workspace oracle.
#[derive(Debug, Clone)]
pub struct AttemptedCandidate {
    pub candidate: RepairCandidate,
    pub verified: bool,
    pub rejected_reason: Option<String>,
}

/// The full, serializable record of a repair run. The original project is
/// never touched by the engine itself; callers apply `applied` patches only
/// after an intentional decision (and never for dry runs or multi-verify).
#[derive(Debug, Clone)]
pub struct RepairReport {
    pub status: RepairStatus,
    pub errors: Vec<ParsedError>,
    /// Ranked, de-duplicated candidates proposed for the first batch.
    pub proposed: Vec<RepairCandidate>,
    /// Every candidate that made it as far as the oracle, with its verdict.
    pub attempted: Vec<AttemptedCandidate>,
    /// The single verified patch set ready to be applied (empty otherwise).
    pub applied: Vec<RepairCandidate>,
    /// Multiple verified candidates requiring human selection.
    pub verified_alternatives: Vec<RepairCandidate>,
    pub final_verification: Option<VerificationResult>,
    pub attempts: u32,
    pub isolation_error: Option<String>,
}

impl RepairReport {
    pub fn new(status: RepairStatus, errors: Vec<ParsedError>) -> Self {
        Self {
            status,
            errors,
            proposed: Vec::new(),
            attempted: Vec::new(),
            applied: Vec::new(),
            verified_alternatives: Vec::new(),
            final_verification: None,
            attempts: 0,
            isolation_error: None,
        }
    }

    pub fn is_verified(&self) -> bool {
        self.status == RepairStatus::Verified
    }
}

/// Run a full, isolated repair session for a project.
///
/// gesture outline:
///   cargo check → normalize diagnostics → source/context analysis →
///   generate candidates → rank → apply in an isolated workspace → oracle →
///   verified / rejected. The original project is left untouched.
pub fn run_repair(
    project_dir: &str,
    verify_mode: VerifyMode,
    dry_run: bool,
    config: &EngineConfig,
) -> Result<RepairReport> {
    let output = crate::runner::run_cargo_build(project_dir)?;
    let errors = parse_diagnostics(&output);

    if errors.is_empty() {
        return Ok(RepairReport::new(RepairStatus::NoDiagnostic, errors));
    }

    let mut report = RepairReport::new(RepairStatus::PatchGenerated, errors.clone());

    match verification::reproduce_failure(project_dir, verify_mode) {
        Ok(true) => {}
        Ok(false) => {
            report.status = RepairStatus::NothingToRepair;
            return Ok(report);
        }
        Err(err) => {
            report.isolation_error = Some(err.to_string());
        }
    }

    let mut history = RepairHistory::new();
    let proposed = candidates_for(&errors, &history, project_dir, config.max_candidates);
    report.proposed = proposed.clone();

    if dry_run {
        report.status = RepairStatus::Preview;
        return Ok(report);
    }

    if proposed.is_empty() {
        report.status = RepairStatus::HumanReviewRequired;
        return Ok(report);
    }

    let workspace = match IsolatedWorkspace::create(project_dir) {
        Ok(workspace) => workspace,
        Err(err) => anyhow::bail!("could not create isolated workspace: {}", err),
    };

    let mut attempts: u32 = 0;
    let mut verified_candidate: Option<RepairCandidate> = None;
    let mut workspace_applied: Vec<RepairCandidate> = Vec::new();
    let mut tried_candidates: Vec<AttemptedCandidate> = Vec::new();
    let mut pending_errors = errors;
    let mut last_batch: Vec<RepairCandidate> = Vec::new();

    while attempts < config.max_attempts {
        let candidates = candidates_for(
            &pending_errors,
            &history,
            project_dir,
            config.max_candidates,
        );

        if candidates.is_empty() {
            break;
        }

        last_batch = candidates.clone();

        let candidate = candidates[0].clone();
        attempts += 1;
        history.record_patch(&candidate.patch);

        if !candidate.patch.edits.is_empty() {
            if let Err(err) = candidate.patch.validate(workspace.path().to_str().unwrap()) {
                tried_candidates.push(AttemptedCandidate {
                    candidate: candidate.clone(),
                    verified: false,
                    rejected_reason: Some(format!("invalid patch: {err}")),
                });
                continue;
            }

            if let Err(err) = candidate.patch.apply(workspace.path().to_str().unwrap()) {
                tried_candidates.push(AttemptedCandidate {
                    candidate: candidate.clone(),
                    verified: false,
                    rejected_reason: Some(format!("apply failed: {err}")),
                });
                continue;
            }

            workspace_applied.push(candidate.clone());
        }

        let result = match verification::verify_in_workspace(&workspace, verify_mode) {
            Ok(result) => result,
            Err(err) => {
                tried_candidates.push(AttemptedCandidate {
                    candidate: candidate.clone(),
                    verified: false,
                    rejected_reason: Some(format!("verification error: {err}")),
                });
                continue;
            }
        };

        report.final_verification = Some(result.clone());

        if result.passed {
            verified_candidate = Some(candidate.clone());
            tried_candidates.push(AttemptedCandidate {
                candidate: candidate.clone(),
                verified: true,
                rejected_reason: None,
            });
            break;
        }

        tried_candidates.push(AttemptedCandidate {
            candidate: candidate.clone(),
            verified: false,
            rejected_reason: Some("`cargo check` failed in the isolated workspace".to_string()),
        });

        let combined = format!("{}\n{}", result.stdout, result.stderr);
        let next_errors = parse_diagnostics(&combined);

        if next_errors.is_empty() {
            break;
        }

        pending_errors = next_errors;
    }

    report.attempted = tried_candidates;
    report.attempts = attempts;

    // Detect alternative verified repairs: any other candidate from the final
    // batch that also compiles against the same failure state. If one exists,
    // we have a multiple-verified situation requiring human selection.
    let mut verified_alternatives: Vec<RepairCandidate> = Vec::new();
    if let Some(chain_candidate) = verified_candidate.as_ref() {
        let chain_signature = patch_signature(&chain_candidate.patch);
        let alternatives: Vec<RepairCandidate> = last_batch
            .iter()
            .filter(|candidate| {
                patch_signature(&candidate.patch) != chain_signature
                    || candidate.kind != chain_candidate.kind
                    || candidate.description != chain_candidate.description
            })
            .take(config.max_alternative_checks)
            .cloned()
            .collect();

        for alternative in alternatives {
            if workspace.reset().is_err() {
                continue;
            }

            let mut ready = true;
            if !alternative.patch.edits.is_empty()
                && (alternative
                    .patch
                    .validate(workspace.path().to_str().unwrap())
                    .is_err()
                    || alternative
                        .patch
                        .apply(workspace.path().to_str().unwrap())
                        .is_err())
            {
                ready = false;
            }

            if !ready {
                continue;
            }

            if let Ok(result) = verification::verify_in_workspace(&workspace, verify_mode) {
                report.final_verification = Some(result.clone());
                if result.passed {
                    report.attempted.push(AttemptedCandidate {
                        candidate: alternative.clone(),
                        verified: true,
                        rejected_reason: None,
                    });
                    verified_alternatives.push(alternative.clone());
                }
            }
        }
    }
    report.verified_alternatives = verified_alternatives;

    let multiple_verified =
        verified_candidate.is_some() && !report.verified_alternatives.is_empty();
    let single_verified = verified_candidate.is_some() && !multiple_verified;

    report.status = if single_verified {
        RepairStatus::Verified
    } else if multiple_verified {
        RepairStatus::MultipleVerifiedRepairs
    } else if report.isolation_error.is_some() {
        RepairStatus::IsolationCheckFailed
    } else if report.attempted.is_empty() {
        RepairStatus::HumanReviewRequired
    } else {
        RepairStatus::RepairAttempted
    };

    if single_verified {
        report.applied = workspace_applied;
    }

    Ok(report)
}

/// Generated candidates for the failing diagnostics, ranked and de-duplicated
/// against the repair history, capped at `max` candidates.
pub fn candidates_for(
    errors: &[ParsedError],
    history: &RepairHistory,
    project_dir: &str,
    max: usize,
) -> Vec<RepairCandidate> {
    let mut candidates: Vec<RepairCandidate> = Vec::new();

    for error in errors {
        let analysis = crate::analyzer::analyze(error);
        let source = crate::transform::source_text_for(error, project_dir).unwrap_or_default();
        candidates.extend(crate::transform::generate_candidates(
            error, &analysis, &source,
        ));
    }

    let candidates = rank_candidates(candidates);

    candidates
        .into_iter()
        .filter(|candidate| !history.contains(&candidate.patch))
        .take(max)
        .collect()
}

/// Parse rustc JSON diagnostics from a `cargo --message-format=json` stream.
pub fn parse_diagnostics(output: &str) -> Vec<ParsedError> {
    let mut errors = Vec::new();

    for line in output.lines() {
        let Ok(message) = serde_json::from_str::<crate::diagnostics::CargoMessage>(line) else {
            continue;
        };

        if message.reason != "compiler-message" {
            continue;
        }

        let Some(rustc_message) = message.message else {
            continue;
        };

        if let Some(error) = ParsedError::from_rustc_message(&rustc_message) {
            errors.push(error);
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings_are_stable_for_json_consumers() {
        assert_eq!(RepairStatus::Verified.as_str(), "repair_applied");
        assert_eq!(RepairStatus::NothingToRepair.as_str(), "nothing_to_repair");
        assert_eq!(RepairStatus::Preview.as_str(), "preview");
        assert_eq!(
            RepairStatus::MultipleVerifiedRepairs.as_str(),
            "multiple_verified"
        );
        assert_eq!(
            RepairStatus::HumanReviewRequired.as_str(),
            "human_review_required"
        );
        assert_eq!(RepairStatus::RepairAttempted.as_str(), "repair_attempted");
    }

    #[test]
    fn config_uses_conservative_defaults() {
        let config = EngineConfig::from_env();
        assert!(config.max_attempts >= 1);
        assert!(config.max_candidates >= 1);
    }

    #[test]
    fn parses_zero_diagnostics_for_empty_input() {
        assert!(parse_diagnostics("").is_empty());
        assert!(parse_diagnostics("not json").is_empty());
    }
}
