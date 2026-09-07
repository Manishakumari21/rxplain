use crate::diagnostics::ParsedError;
use crate::patch::{Edit, Patch};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum RepairKind {
    CompilerSuggested,
    Borrow,
    Clone,
    Mutability,
    ArgumentAdjustment,
    TypeAdjustment,
    ImportAdjustment,
    Other,
}

impl RepairKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RepairKind::CompilerSuggested => "compiler_suggested",
            RepairKind::Borrow => "borrow",
            RepairKind::Clone => "clone",
            RepairKind::Mutability => "mutability",
            RepairKind::ArgumentAdjustment => "argument_adjustment",
            RepairKind::TypeAdjustment => "type_adjustment",
            RepairKind::ImportAdjustment => "import_adjustment",
            RepairKind::Other => "other",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RepairCandidate {
    pub kind: RepairKind,
    pub confidence: Confidence,
    pub description: String,
    pub evidence: Vec<String>,
    pub patch: Patch,
}

fn ordering_score(candidate: &RepairCandidate) -> u32 {
    let kind_score = match candidate.kind {
        RepairKind::CompilerSuggested => 0,
        RepairKind::Borrow => 1,
        RepairKind::Clone => 2,
        RepairKind::Mutability => 3,
        RepairKind::ArgumentAdjustment => 4,
        RepairKind::TypeAdjustment => 5,
        RepairKind::ImportAdjustment => 6,
        RepairKind::Other => 7,
    };

    let confidence_score = match candidate.confidence {
        Confidence::High => 0,
        Confidence::Medium => 1,
        Confidence::Low => 2,
    };

    let edit_count = candidate.patch.edits.len() as u32;

    kind_score * 1000 + confidence_score * 100 + edit_count.min(99)
}

pub fn rank_candidates(candidates: Vec<RepairCandidate>) -> Vec<RepairCandidate> {
    let mut candidates = candidates;

    candidates.sort_by_key(ordering_score);

    candidates
}

pub fn candidates_for_error(error: &ParsedError) -> Vec<RepairCandidate> {
    let mut candidates = Vec::new();

    for suggestion in &error.suggestions {
        candidates.push(RepairCandidate {
            kind: RepairKind::CompilerSuggested,
            confidence: if suggestion.applicability == "MachineApplicable" {
                Confidence::High
            } else if suggestion.applicability == "MaybeIncorrect" {
                Confidence::Low
            } else {
                Confidence::Medium
            },
            description: format!(
                "Apply the compiler-suggested change at {}:{}.",
                suggestion.file, suggestion.line
            ),
            evidence: vec![
                if let Some(label) = &suggestion.label {
                    format!("Compiler note: {}", label)
                } else {
                    format!("Applicability: {}", suggestion.applicability)
                },
                format!(
                    "Replace {}:{}-{} with `{}`",
                    suggestion.file, suggestion.line, suggestion.column, suggestion.replacement
                ),
            ],
            patch: Patch::new(vec![Edit {
                file: suggestion.file.clone(),
                line: suggestion.line,
                start_col: suggestion.column,
                end_col: suggestion.column_end,
                replacement: suggestion.replacement.clone(),
                label: suggestion.label.clone(),
            }]),
        });
    }

    if error.code == "E0382" {
        moved_value_candidates(error, &mut candidates);
    }

    candidates
}

fn moved_value_candidates(error: &ParsedError, candidates: &mut Vec<RepairCandidate>) {
    if !error.suggestions.is_empty() {
        return;
    }

    let _ = candidates;
}

#[derive(Debug, Default)]
pub struct RepairHistory {
    attempted: BTreeSet<String>,
}

impl RepairHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_patch(&mut self, patch: &Patch) -> bool {
        let signature = patch_signature(patch);
        self.attempted.insert(signature)
    }

    pub fn contains(&self, patch: &Patch) -> bool {
        self.attempted.contains(&patch_signature(patch))
    }
}

pub fn patch_signature(patch: &Patch) -> String {
    let mut parts: Vec<String> = patch
        .edits
        .iter()
        .map(|edit| {
            format!(
                "{}:{}:{}-{}:{}",
                edit.file, edit.line, edit.start_col, edit.end_col, edit.replacement
            )
        })
        .collect();
    parts.sort();
    parts.join("|")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suggestion_patch(kind: RepairKind) -> RepairCandidate {
        RepairCandidate {
            kind,
            confidence: Confidence::High,
            description: "test".to_string(),
            evidence: Vec::new(),
            patch: Patch::new(vec![Edit::new("src/main.rs", 1, 1, 1, "x")]),
        }
    }

    #[test]
    fn ranking_prefers_compiler_suggestion_and_fewer_edits() {
        let mut candidates = vec![
            suggestion_patch(RepairKind::Clone),
            suggestion_patch(RepairKind::CompilerSuggested),
        ];

        candidates[0].patch = Patch::new(vec![
            Edit::new("src/main.rs", 1, 1, 1, "x"),
            Edit::new("src/main.rs", 2, 1, 1, "x"),
        ]);

        let ranked = rank_candidates(candidates);

        assert_eq!(ranked[0].kind, RepairKind::CompilerSuggested);
    }

    #[test]
    fn history_detects_repeated_patches() {
        let mut history = RepairHistory::new();

        let patch = Patch::new(vec![Edit::new("src/main.rs", 1, 1, 1, "x")]);

        assert!(history.record_patch(&patch), "first occurrence is new");
        assert!(!history.record_patch(&patch), "repeat is blocked");
    }

    #[test]
    fn history_contains_detects_attempted_patch() {
        let mut history = RepairHistory::new();

        let patch = Patch::new(vec![Edit::new("src/main.rs", 1, 1, 1, "x")]);

        assert!(!history.contains(&patch));
        history.record_patch(&patch);
        assert!(history.contains(&patch));
    }

    #[test]
    fn candidate_generation_from_suggestion() {
        let error = crate::diagnostics::ParsedError {
            code: "E0594".to_string(),
            raw_message: "cannot assign".to_string(),
            spans: Vec::new(),
            suggestions: vec![crate::diagnostics::CompilerSuggestion {
                file: "src/main.rs".to_string(),
                line: 1,
                column: 9,
                column_end: 12,
                replacement: "&mut ".to_string(),
                applicability: "MachineApplicable".to_string(),
                label: Some("change to mutable".to_string()),
            }],
        };

        let candidates = candidates_for_error(&error);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].kind, RepairKind::CompilerSuggested);
        assert_eq!(candidates[0].confidence, Confidence::High);
        assert_eq!(candidates[0].patch.edits.len(), 1);
    }

    #[test]
    fn e0382_without_suggestion_yields_no_fabricated_patch() {
        let error = crate::diagnostics::ParsedError {
            code: "E0382".to_string(),
            raw_message: "use of moved value".to_string(),
            spans: Vec::new(),
            suggestions: Vec::new(),
        };

        let candidates = candidates_for_error(&error);

        assert!(
            candidates.is_empty(),
            "without compiler evidence we must not fabricate a patch"
        );
    }
}
