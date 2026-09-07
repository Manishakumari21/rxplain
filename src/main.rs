use anyhow::Result;
use clap::Parser;
use colored::*;
use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
use serde::Serialize;
use std::time::Instant;

mod analyzer;
mod context;
mod diagnostics;
mod engine;
mod explain;
mod fixer;
mod patch;
mod repair;
mod repair_context;
mod runner;
mod transform;
mod tui;
mod verification;
mod walk;

use diagnostics::ParsedError;

#[derive(Parser, Debug)]
#[command(
    name = "rxplain",
    version,
    about = "A deterministic, offline explainer for Rust compiler errors"
)]
struct Cli {
    project_dir: String,

    #[arg(long)]
    fix: bool,

    #[arg(long, requires = "fix", value_name = "MODE")]
    verify: Option<String>,

    #[arg(long, requires = "fix")]
    dry_run: bool,

    #[arg(long)]
    json: bool,

    #[arg(long)]
    walk: bool,

    #[arg(long)]
    tui: bool,

    #[arg(long)]
    quiet: bool,
}

#[derive(Debug, Serialize)]
struct JsonReport {
    errors: Vec<JsonError>,
}

#[derive(Debug, Serialize)]
struct JsonError {
    code: String,
    message: String,
    locations: Vec<JsonLocation>,
    relationships: Vec<String>,
    explanation: JsonExplanation,
    suggestions: Vec<JsonSuggestion>,
    fix: JsonFix,
}

#[derive(Debug, Serialize)]
struct JsonLocation {
    file: String,
    line: u32,
    column: u32,
    snippet: String,
    label: Option<String>,
}

#[derive(Debug, Serialize)]
struct JsonExplanation {
    title: String,
    summary: String,
    concept: Option<String>,
    principle: Option<String>,
    fix_options: Vec<String>,
}

#[derive(Debug, Serialize)]
struct JsonSuggestion {
    file: String,
    line: u32,
    column: u32,
    replacement: String,
    applicability: String,
    label: Option<String>,
}

#[derive(Debug, Serialize)]
struct JsonFix {
    kind: String,
    description: String,
    file: Option<String>,
    line: Option<u32>,
    column: Option<u32>,
    replacement: Option<String>,
    applicability: Option<String>,
}

const BANNER: &str = r#"
   ██████╗ ██╗  ██╗██████╗ ██╗      █████╗ ██╗███╗   ██╗
   ██╔══██╗╚██╗██╔╝██╔══██╗██║     ██╔══██╗██║████╗  ██║
   ██████╔╝ ╚███╔╝ ██████╔╝██║     ███████║██║██╔██╗ ██║
   ██╔══██╗ ██╔██╗ ██╔═══╝ ██║     ██╔══██║██║██║╚██╗██║
   ██║  ██║██╔╝ ██╗██║     ███████╗██║  ██║██║██║ ╚████║
   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚══════╝╚═╝  ╚═╝╚═╝╚═╝  ╚═══╝
"#;

fn print_banner() {
    println!();
    for (index, line) in BANNER.lines().enumerate() {
        let color = match index {
            0 | 4 => Color::BrightRed,
            1 | 3 => Color::BrightYellow,
            _ => Color::BrightCyan,
        };
        println!("{}", line.color(color));
    }
    println!(
        "{} {}\n",
        "✦".yellow(),
        "Rust error, explained. — deterministic · offline · safe fixes"
            .white()
            .italic()
    );
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.fix {
        let verify_mode = cli
            .verify
            .as_deref()
            .map(parse_verify_mode)
            .transpose()?
            .unwrap_or(verification::VerifyMode::Check);

        return fix_command(&cli.project_dir, cli.dry_run, cli.json, verify_mode);
    }

    if !cli.json && !cli.tui && !cli.quiet {
        print_banner();
    }

    let started = Instant::now();

    let output = if cli.json || cli.tui {
        runner::run_cargo_build(&cli.project_dir)?
    } else {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["▖", "▘", "▝", "▗", "▖", "▘", "▝", "▗"])
                .template("{spinner:.yellow} {msg}")
                .unwrap(),
        );
        spinner.set_message(format!("Analyzing {}...", cli.project_dir));
        let result = runner::run_cargo_build(&cli.project_dir);
        spinner.finish_and_clear();
        result?
    };

    let errors = engine::parse_diagnostics(&output);

    let project_dir = cli.project_dir.clone();

    if cli.json {
        print_json_report(&errors)?;
    } else if cli.walk {
        walk::walk_errors(&errors, &project_dir);
    } else if cli.tui {
        if std::io::IsTerminal::is_terminal(&std::io::stdout())
            && std::io::IsTerminal::is_terminal(&std::io::stdin())
        {
            crate::tui::run(&errors, &project_dir)?;
        } else {
            eprintln!(
                "note: --tui requires an interactive terminal, \
                 falling back to plain output"
            );

            print_human_report(&errors, &project_dir, started);
        }
    } else {
        print_human_report(&errors, &project_dir, started);
    }

    Ok(())
}

fn parse_verify_mode(mode: &str) -> Result<verification::VerifyMode> {
    match mode {
        "check" => Ok(verification::VerifyMode::Check),
        "build" => Ok(verification::VerifyMode::Build),
        "test" => Ok(verification::VerifyMode::Test),
        other => {
            anyhow::bail!("invalid --verify mode `{other}`: expected `check`, `build`, or `test`")
        }
    }
}

fn print_candidates(candidates: &[repair::RepairCandidate], with_preview: bool) {
    for (index, candidate) in candidates.iter().enumerate() {
        println!();
        println!(
            "  {}. [{}] {} (confidence: {})",
            index + 1,
            candidate.kind.as_str(),
            candidate.description,
            candidate.confidence.as_str()
        );

        for evidence in &candidate.evidence {
            println!("     • {}", evidence.dimmed());
        }

        if with_preview && !candidate.patch.edits.is_empty() {
            for line in candidate.patch.preview().lines() {
                println!("       {}", line.bright_black());
            }
        }
    }
}

fn emit_fix_json(
    status: &str,
    errors: &[ParsedError],
    candidates: &[repair::RepairCandidate],
    attempted: &[engine::AttemptedCandidate],
    dry_run: bool,
    verification: Option<JsonVerification>,
    attempts: u32,
) -> Result<()> {
    let report = fix_json_report(
        status,
        errors,
        candidates,
        attempted,
        dry_run,
        verification,
        attempts,
    );
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn fix_command(
    project_dir: &str,
    dry_run: bool,
    json: bool,
    verify_mode: verification::VerifyMode,
) -> Result<()> {
    let started = Instant::now();
    let config = engine::EngineConfig::from_env();

    if !json {
        print_banner();
    }

    let report = engine::run_repair(project_dir, verify_mode, dry_run, &config)?;
    let status = report.status;

    match status {
        engine::RepairStatus::NoDiagnostic => {
            if json {
                emit_fix_json(
                    status.as_str(),
                    &report.errors,
                    &report.proposed,
                    &report.attempted,
                    dry_run,
                    None,
                    0,
                )?;
            } else {
                println!(
                    "{} No compiler errors found. Nothing to fix.",
                    "✔".green().bold()
                );
            }
            return Ok(());
        }
        engine::RepairStatus::NothingToRepair => {
            if json {
                emit_fix_json(
                    status.as_str(),
                    &report.errors,
                    &report.proposed,
                    &report.attempted,
                    dry_run,
                    None,
                    0,
                )?;
            } else {
                println!(
                    "{} Project passes `cargo {}` in isolation; nothing to repair.",
                    "✔".green().bold(),
                    verify_mode.as_str()
                );
            }
            return Ok(());
        }
        engine::RepairStatus::IsolationCheckFailed => {
            if json {
                emit_fix_json(
                    status.as_str(),
                    &report.errors,
                    &report.proposed,
                    &report.attempted,
                    dry_run,
                    None,
                    0,
                )?;
            } else {
                println!(
                    "  {} Isolation check failed: {}",
                    "⚠".yellow(),
                    report.isolation_error.as_deref().unwrap_or("unknown error")
                );
            }
            return Ok(());
        }
        _ => {}
    }

    if dry_run {
        if json {
            emit_fix_json(
                status.as_str(),
                &report.errors,
                &report.proposed,
                &report.attempted,
                true,
                None,
                0,
            )?;
        } else {
            println!();
            println!(
                "{} {} candidate repair{} generated (most confident first).",
                "🔧".cyan(),
                report.proposed.len(),
                if report.proposed.len() == 1 { "" } else { "s" }
            );
            print_candidates(&report.proposed, true);
            println!();
            println!("{} Dry run — no files were modified.", "ℹ".cyan().bold());
            println!(
                "{}",
                format!("Completed in {}", HumanDuration(started.elapsed())).dimmed()
            );
        }
        return Ok(());
    }

    if status == engine::RepairStatus::HumanReviewRequired {
        if json {
            emit_fix_json(
                status.as_str(),
                &report.errors,
                &report.proposed,
                &report.attempted,
                false,
                None,
                0,
            )?;
        } else {
            println!();
            println!(
                "{} No safely expressible candidate repair was found.",
                "⚠".yellow().bold()
            );
            println!("  The remaining errors require human judgment.");
        }
        return Ok(());
    }

    if !json {
        println!();
        println!(
            "{} {} candidate repair{} generated (most confident first).",
            "🔧".cyan(),
            report.proposed.len(),
            if report.proposed.len() == 1 { "" } else { "s" }
        );
        print_candidates(&report.proposed, false);
        println!();
        println!(
            "{} Verifying candidates with `cargo {}` in an isolated workspace...",
            "⏳".yellow(),
            verify_mode.as_str()
        );
    }

    let tried_candidates: Vec<JsonTriedCandidate> = report
        .attempted
        .iter()
        .map(|attempted| JsonTriedCandidate {
            kind: attempted.candidate.kind.as_str().to_string(),
            description: attempted.candidate.description.clone(),
            confidence: attempted.candidate.confidence.as_str().to_string(),
            verified: attempted.verified,
            rejected_reason: attempted.rejected_reason.clone(),
        })
        .collect();

    let verified_candidate = report.applied.first();

    for candidate in &report.applied {
        fixer::apply_patches(&candidate.patch, project_dir)?;
    }

    if json {
        let verification = report
            .final_verification
            .as_ref()
            .map(|result| JsonVerification {
                mode: verify_mode.as_str().to_string(),
                command: result.command.clone(),
                passed: report.is_verified(),
                attempts: report.attempts,
                duration_ms: result.duration_ms,
                applied: report
                    .applied
                    .iter()
                    .map(|candidate| JsonAppliedStep {
                        kind: candidate.kind.as_str().to_string(),
                        description: candidate.description.clone(),
                        patch: candidate.patch.edits.iter().map(JsonEdit::from).collect(),
                    })
                    .collect(),
                tried: tried_candidates,
            });

        return emit_fix_json(
            status.as_str(),
            &report.errors,
            &report.proposed,
            &report.attempted,
            false,
            verification,
            report.attempts,
        );
    }

    match status {
        engine::RepairStatus::RepairAttempted => {
            println!(
                "{} No candidate passed verification in isolation; original project left untouched.",
                "⚠".yellow().bold()
            );
        }
        engine::RepairStatus::MultipleVerifiedRepairs => {
            println!(
                "{} Multiple verified repairs found ({}); human selection required. No patch applied automatically.",
                "⚠".yellow().bold(),
                report.verified_alternatives.len() + 1
            );
            if let Some(candidate) = report.applied.first() {
                println!(
                    "   • [{}] {} (confidence: {})",
                    candidate.kind.as_str(),
                    candidate.description,
                    candidate.confidence.as_str()
                );
            }
            for candidate in &report.verified_alternatives {
                println!(
                    "   • [{}] {} (confidence: {})",
                    candidate.kind.as_str(),
                    candidate.description,
                    candidate.confidence.as_str()
                );
            }
        }
        engine::RepairStatus::Verified => {
            if let (Some(candidate), Some(result)) =
                (report.applied.first(), report.final_verification.as_ref())
            {
                println!(
                    "  {} Verified repair: {} (`cargo {}` passed in {} ms).",
                    "✔".green().bold(),
                    candidate.kind.as_str(),
                    verify_mode.as_str(),
                    result.duration_ms
                );
            }
            println!(
                "{} {} verified repair{} applied to the original project.",
                "✔".green().bold(),
                report.applied.len(),
                if report.applied.len() == 1 { "" } else { "s" }
            );
        }
        _ => {}
    }

    let _ = verified_candidate;

    println!(
        "{}",
        format!("Completed in {}", HumanDuration(started.elapsed())).dimmed()
    );

    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct JsonFixReport {
    status: String,
    diagnostics: Vec<JsonFixDiagnostic>,
    candidates: Vec<JsonFixCandidate>,
    verification: Option<JsonVerification>,
    attempts: u32,
    dry_run: bool,
}

#[derive(Debug, serde::Serialize)]
struct JsonFixDiagnostic {
    code: String,
    message: String,
    locations: Vec<JsonLocation>,
}

#[derive(Debug, serde::Serialize)]
struct JsonFixCandidate {
    kind: String,
    confidence: String,
    description: String,
    evidence: Vec<String>,
    patch: Vec<JsonEdit>,
    verified: Option<bool>,
    rejected_reason: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct JsonEdit {
    file: String,
    line: u32,
    start_col: u32,
    end_col: u32,
    replacement: String,
}

#[derive(Debug, serde::Serialize)]
struct JsonVerification {
    mode: String,
    command: String,
    passed: bool,
    attempts: u32,
    duration_ms: u128,
    applied: Vec<JsonAppliedStep>,
    tried: Vec<JsonTriedCandidate>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct JsonTriedCandidate {
    kind: String,
    description: String,
    confidence: String,
    verified: bool,
    rejected_reason: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct JsonAppliedStep {
    kind: String,
    description: String,
    patch: Vec<JsonEdit>,
}

impl From<&patch::Edit> for JsonEdit {
    fn from(edit: &patch::Edit) -> Self {
        JsonEdit {
            file: edit.file.clone(),
            line: edit.line,
            start_col: edit.start_col,
            end_col: edit.end_col,
            replacement: edit.replacement.clone(),
        }
    }
}

fn fix_json_report(
    status: &str,
    errors: &[ParsedError],
    candidates: &[repair::RepairCandidate],
    attempted: &[engine::AttemptedCandidate],
    dry_run: bool,
    verification: Option<JsonVerification>,
    attempts: u32,
) -> JsonFixReport {
    let diagnostics = errors
        .iter()
        .map(|error| JsonFixDiagnostic {
            code: error.code.clone(),
            message: error.raw_message.clone(),
            locations: error
                .spans
                .iter()
                .map(|span| JsonLocation {
                    file: span.file_name.clone(),
                    line: span.line_start,
                    column: span.column_start,
                    snippet: span
                        .text
                        .first()
                        .map(|t| t.text.clone())
                        .unwrap_or_default(),
                    label: span.label.clone(),
                })
                .collect(),
        })
        .collect();

    let attempted_by_signature: std::collections::HashMap<String, (bool, Option<String>)> =
        attempted
            .iter()
            .map(|item| {
                (
                    repair::patch_signature(&item.candidate.patch),
                    (item.verified, item.rejected_reason.clone()),
                )
            })
            .collect();

    let candidates = candidates
        .iter()
        .map(|candidate| {
            let signature = repair::patch_signature(&candidate.patch);
            let (verified, rejected_reason) = attempted_by_signature
                .get(&signature)
                .cloned()
                .map(|(verified, reason)| (Some(verified), reason))
                .unwrap_or((None, None));

            JsonFixCandidate {
                kind: candidate.kind.as_str().to_string(),
                confidence: candidate.confidence.as_str().to_string(),
                description: candidate.description.clone(),
                evidence: candidate.evidence.clone(),
                patch: candidate.patch.edits.iter().map(JsonEdit::from).collect(),
                verified,
                rejected_reason,
            }
        })
        .collect();

    JsonFixReport {
        status: status.to_string(),
        diagnostics,
        candidates,
        verification,
        attempts,
        dry_run,
    }
}

fn print_json_report(errors: &[ParsedError]) -> Result<()> {
    let mut json_errors = Vec::new();

    for error in errors {
        let analysis = analyzer::analyze(error);
        let explanation = explain::explain(error, &analysis);
        let fix = fixer::suggest_fix(error);

        let locations = analysis
            .locations
            .iter()
            .map(|location| JsonLocation {
                file: location.file.clone(),
                line: location.line,
                column: location.column,
                snippet: location.snippet.clone(),
                label: location.label.clone(),
            })
            .collect();

        let relationships = analysis
            .relationships
            .iter()
            .map(|relationship| relationship.explanation.clone())
            .collect();

        let suggestions = analysis
            .suggestions
            .iter()
            .map(|suggestion| JsonSuggestion {
                file: suggestion.file.clone(),
                line: suggestion.line,
                column: suggestion.column,
                replacement: suggestion.replacement.clone(),
                applicability: suggestion.applicability.clone(),
                label: suggestion.label.clone(),
            })
            .collect();

        let json_fix = match fix.kind {
            fixer::FixKind::CompilerSuggested => {
                if let Some(suggestion) = fix.suggestion {
                    JsonFix {
                        kind: "CompilerSuggested".to_string(),
                        description: fix.description,
                        file: Some(suggestion.file),
                        line: Some(suggestion.line),
                        column: Some(suggestion.column),
                        replacement: Some(suggestion.replacement),
                        applicability: Some(suggestion.applicability),
                    }
                } else {
                    JsonFix {
                        kind: "CompilerSuggested".to_string(),
                        description: fix.description,
                        file: None,
                        line: None,
                        column: None,
                        replacement: None,
                        applicability: None,
                    }
                }
            }

            fixer::FixKind::RequiresHumanJudgment => JsonFix {
                kind: "RequiresHumanJudgment".to_string(),
                description: fix.description,
                file: None,
                line: None,
                column: None,
                replacement: None,
                applicability: None,
            },
        };

        json_errors.push(JsonError {
            code: error.code.clone(),
            message: error.raw_message.clone(),
            locations,
            relationships,
            explanation: JsonExplanation {
                title: explanation.title,
                summary: explanation.plain_summary,
                concept: explanation.concept,
                principle: explanation.principle,
                fix_options: explanation.fix_options,
            },
            suggestions,
            fix: json_fix,
        });
    }

    let report = JsonReport {
        errors: json_errors,
    };

    println!("{}", serde_json::to_string_pretty(&report)?);

    Ok(())
}

fn print_human_report(errors: &[ParsedError], project_dir: &str, started: Instant) {
    let elapsed = started.elapsed();

    if errors.is_empty() {
        println!();

        println!(
            "{} {}",
            "✔".green().bold(),
            "No compiler errors found.".green().bold()
        );

        println!(
            "  {} {}",
            "⏱".dimmed(),
            format!("Completed in {}", HumanDuration(elapsed)).dimmed()
        );

        println!(
            "  {} {}",
            "🎉".white(),
            "All good — your Rust compiles cleanly!".white().dimmed()
        );

        println!();

        return;
    }

    println!();

    println!(
        "{} {} {}",
        "✖".red().bold(),
        format!(
            "{} error{} found",
            errors.len(),
            if errors.len() == 1 { "" } else { "s" }
        )
        .red()
        .bold(),
        format!("in {}", HumanDuration(elapsed)).dimmed()
    );

    println!();

    for (index, error) in errors.iter().enumerate() {
        println!();

        println!(
            "  {}",
            format!(" ERROR {} ", error.code)
                .on_magenta()
                .bright_white()
                .bold()
        );

        println!(
            "  {} {}{}{} {}",
            "◤".bright_magenta(),
            " ".repeat(6),
            format!("{} / {}", index + 1, errors.len()).white().bold(),
            " ".repeat(6),
            "◥".bright_magenta()
        );

        println!("  {}", "▔".repeat(40).bright_magenta());

        println!(
            "\n  {} {}",
            "▸".yellow().bold(),
            "Compiler message".white().bold()
        );

        println!("    {}", error.raw_message.white().italic());

        let analysis = analyzer::analyze(error);

        if !analysis.locations.is_empty() {
            println!(
                "\n  {} {}",
                "▸".yellow().bold(),
                "Compiler evidence".white().bold()
            );

            for location in &analysis.locations {
                println!(
                    "    {} {}:{}:{}",
                    "●".red(),
                    location.file,
                    location.line,
                    location.column
                );

                if !location.snippet.is_empty() {
                    println!("      {}", location.snippet.yellow());
                }

                if let Some(label) = &location.label {
                    println!("      {} {}", "└─".bright_black(), label.cyan().italic());
                }
            }
        }

        if !analysis.relationships.is_empty() {
            println!(
                "\n  {} {}",
                "▸".yellow().bold(),
                "Why these locations are related".white().bold()
            );

            for relationship in &analysis.relationships {
                println!("    {} {}", "›".cyan(), relationship.explanation.dimmed());
            }
        }

        let contexts = context::SourceContext::from_error(error, project_dir);

        if !contexts.is_empty() {
            println!(
                "\n  {} {}",
                "▸".yellow().bold(),
                "Source context".white().bold()
            );

            for source_context in &contexts {
                if contexts.len() > 1 {
                    println!("    {} {}", "↳".bright_blue(), source_context.file.dimmed());
                }

                for line in &source_context.lines {
                    let marker = if line.highlighted { "►" } else { " " };

                    let gutter_num = format!("{:>4}", line.line_number);

                    if line.highlighted {
                        println!(
                            "   {} {} {} {}",
                            marker.red().bold(),
                            gutter_num.on_red().black().bold(),
                            "│".red().bold(),
                            line.text.on_black().white()
                        );

                        if let Some(label) = &line.label {
                            println!(
                                "     {} {} {}",
                                " ".repeat(gutter_num.len()).on_red().black().bold(),
                                "│".white().bold(),
                                format!("^ {}", label).red().bold()
                            );
                        }
                    } else {
                        println!(
                            "     {} {} {} {}",
                            marker.dimmed(),
                            gutter_num.dimmed(),
                            "│".dimmed(),
                            line.text.dimmed()
                        );
                    }
                }
            }
        }

        let explanation = explain::explain(error, &analysis);

        let title_line = format!(" {} ", explanation.title);

        println!(
            "\n  {} {} {}",
            "┌".bright_black(),
            "─".repeat(title_line.len()).bright_black(),
            "┐".bright_black()
        );

        println!("  │{}│", title_line.bold().white());

        println!(
            "  {} {} {}",
            "└".bright_black(),
            "─".repeat(title_line.len()).bright_black(),
            "┘".bright_black()
        );

        if let Some(concept) = &explanation.concept {
            println!(
                "    {} {} {}",
                "🏷".white(),
                "Concept:".cyan().bold(),
                concept.cyan()
            );
        }

        if let Some(principle) = &explanation.principle {
            println!(
                "    {} {} {}",
                "💭".white(),
                "The rule:".cyan().bold(),
                principle.white()
            );
        }

        println!("    {}", explanation.plain_summary.white());

        if !explanation.fix_options.is_empty() {
            println!("\n  {} {}", "🔧".white(), "Possible fixes".white().bold());

            for option in &explanation.fix_options {
                println!("    {} {}", "•".green(), option.green());
            }
        }

        if !analysis.suggestions.is_empty() {
            println!(
                "\n  {} {}",
                "💡".white(),
                "Compiler suggestions".white().bold()
            );

            for suggestion in &analysis.suggestions {
                println!(
                    "    {} {}:{}:{}",
                    "─".bright_black(),
                    suggestion.file.dimmed(),
                    suggestion.line,
                    suggestion.column
                );

                println!(
                    "      {} {}",
                    "↪".bright_green(),
                    suggestion.replacement.green()
                );

                let app_color = match suggestion.applicability.as_str() {
                    "MachineApplicable" => Color::Green,
                    "MaybeIncorrect" => Color::Yellow,
                    _ => Color::White,
                };

                println!(
                    "      {} {}",
                    "✓".color(app_color),
                    suggestion.applicability.color(app_color)
                );

                if let Some(label) = &suggestion.label {
                    println!("      {} {}", "└─".bright_black(), label.dimmed());
                }
            }
        }

        let fix = fixer::suggest_fix(error);

        println!(
            "\n  {} {}",
            "🛠".white(),
            "Fix classification".white().bold()
        );

        match fix.kind {
            fixer::FixKind::CompilerSuggested => {
                println!("    {} {}", "✔".green().bold(), fix.description.green());

                if let Some(suggestion) = &fix.suggestion {
                    println!(
                        "    {} {}:{}:{}",
                        "▸".bright_cyan(),
                        suggestion.file,
                        suggestion.line,
                        suggestion.column
                    );

                    println!(
                        "    {} {}",
                        "→".bright_green(),
                        suggestion.replacement.green().bold()
                    );

                    let app_color = match suggestion.applicability.as_str() {
                        "MachineApplicable" => Color::Green,
                        "MaybeIncorrect" => Color::Yellow,
                        _ => Color::White,
                    };

                    println!(
                        "    {} {}",
                        "✓".color(app_color).bold(),
                        suggestion.applicability.color(app_color)
                    );

                    if let Some(label) = &suggestion.label {
                        println!("    {} {}", "└─".bright_black(), label.dimmed());
                    }
                }
            }

            fixer::FixKind::RequiresHumanJudgment => {
                println!("    {} {}", "⚠".yellow().bold(), fix.description.yellow());
            }
        }

        println!("\n{}", format!("└{}┘", "─".repeat(58)).bright_black());

        println!();
    }
}
