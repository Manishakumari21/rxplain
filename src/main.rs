use anyhow::Result;
use clap::Parser;
use colored::*;
use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
use serde::Serialize;
use std::time::Instant;

mod analyzer;
mod context;
mod diagnostics;
mod explain;
mod fixer;
mod patch;
mod repair;
mod runner;
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

    let errors = parse_errors(&output);

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

const MAX_REPAIR_ATTEMPTS: u32 = 4;

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

fn candidate_repairs(
    errors: &[ParsedError],
    history: &repair::RepairHistory,
) -> Vec<repair::RepairCandidate> {
    let mut candidates: Vec<repair::RepairCandidate> = Vec::new();

    for error in errors {
        candidates.extend(repair::candidates_for_error(error));
    }

    candidates = repair::rank_candidates(candidates);

    candidates
        .into_iter()
        .filter(|candidate| !history.contains(&candidate.patch))
        .collect()
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
    dry_run: bool,
    verification: Option<JsonVerification>,
    attempts: u32,
) -> Result<()> {
    let report = fix_json_report(status, errors, candidates, dry_run, verification, attempts);
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

    if !json {
        print_banner();
    }

    let output = runner::run_cargo_build(project_dir)?;
    let errors = parse_errors(&output);

    if errors.is_empty() {
        if json {
            emit_fix_json("nothing_to_repair", &errors, &[], dry_run, None, 0)?;
        } else {
            println!(
                "{} No compiler errors found. Nothing to fix.",
                "✔".green().bold()
            );
        }
        return Ok(());
    }

    match verification::reproduce_failure(project_dir, verify_mode) {
        Ok(true) => {}
        Ok(false) => {
            if json {
                emit_fix_json("nothing_to_repair", &errors, &[], dry_run, None, 0)?;
            } else {
                println!(
                    "{} Project passes `cargo {}` in isolation; nothing to repair.",
                    "✔".green().bold(),
                    verify_mode.as_str()
                );
            }
            return Ok(());
        }
        Err(err) => {
            if json {
                emit_fix_json("isolation_check_failed", &errors, &[], dry_run, None, 0)?;
            } else {
                println!("  {} Isolation check failed: {}", "⚠".yellow(), err);
            }
        }
    }

    let mut history = repair::RepairHistory::new();
    let proposed = candidate_repairs(&errors, &history);

    if dry_run {
        if json {
            emit_fix_json("preview", &errors, &proposed, true, None, 0)?;
        } else {
            println!();
            println!(
                "{} {} candidate repair{} generated (most confident first).",
                "🔧".cyan(),
                proposed.len(),
                if proposed.len() == 1 { "" } else { "s" }
            );
            print_candidates(&proposed, true);
            println!();
            println!("{} Dry run — no files were modified.", "ℹ".cyan().bold());
            println!(
                "{}",
                format!("Completed in {}", HumanDuration(started.elapsed())).dimmed()
            );
        }
        return Ok(());
    }

    if proposed.is_empty() {
        if json {
            emit_fix_json("human_review_required", &errors, &proposed, false, None, 0)?;
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
            proposed.len(),
            if proposed.len() == 1 { "" } else { "s" }
        );
        print_candidates(&proposed, false);
        println!();
        println!(
            "{} Verifying candidates with `cargo {}` in an isolated workspace...",
            "⏳".yellow(),
            verify_mode.as_str()
        );
    }

    let workspace = match verification::IsolatedWorkspace::create(project_dir) {
        Ok(ws) => ws,
        Err(err) => anyhow::bail!("could not create isolated workspace: {}", err),
    };

    let mut attempts: u32 = 0;
    let mut verified = false;
    let mut workspace_applied: Vec<repair::RepairCandidate> = Vec::new();
    let mut final_verification: Option<verification::VerificationResult> = None;
    let mut pending_errors = errors.clone();

    while attempts < MAX_REPAIR_ATTEMPTS {
        let candidates = candidate_repairs(&pending_errors, &history);

        if candidates.is_empty() {
            break;
        }

        let candidate = &candidates[0];
        attempts += 1;
        history.record_patch(&candidate.patch);

        if !json {
            println!();
            println!(
                "  {}. [{}] {} (confidence: {})",
                attempts,
                candidate.kind.as_str(),
                candidate.description,
                candidate.confidence.as_str()
            );
        }

        if !candidate.patch.edits.is_empty() {
            if let Err(err) = candidate.patch.validate(workspace.path().to_str().unwrap()) {
                if !json {
                    println!(
                        "  {} Candidate {} invalid on isolated copy: {}",
                        "✖".red(),
                        candidate.kind.as_str(),
                        err
                    );
                }
                continue;
            }

            if let Err(err) = candidate.patch.apply(workspace.path().to_str().unwrap()) {
                if !json {
                    println!("  {} Could not apply candidate: {}", "✖".red(), err);
                }
                continue;
            }

            workspace_applied.push(candidate.clone());
        }

        let result = match verification::verify_in_workspace(&workspace, verify_mode) {
            Ok(result) => result,
            Err(err) => {
                if !json {
                    println!(
                        "  {} Verification error for {}: {}",
                        "✖".red(),
                        candidate.kind.as_str(),
                        err
                    );
                }
                continue;
            }
        };

        final_verification = Some(result.clone());

        if result.passed {
            verified = true;
            if !json {
                println!(
                    "  {} Verified repair: {} (`cargo {}` passed in {} ms).",
                    "✔".green().bold(),
                    candidate.kind.as_str(),
                    verify_mode.as_str(),
                    result.duration_ms
                );
            }
            break;
        }

        if !json {
            println!(
                "  {} Candidate {} failed `cargo {}` in isolation.",
                "✖".red(),
                candidate.kind.as_str(),
                verify_mode.as_str()
            );
        }

        let combined = format!("{}\n{}", result.stdout, result.stderr);
        let next_errors = parse_errors(&combined);

        if next_errors.is_empty() {
            break;
        }

        pending_errors = next_errors;
    }

    if verified {
        for candidate in &workspace_applied {
            fixer::apply_patches(&candidate.patch, project_dir)?;
        }
    }

    if json {
        let verification = final_verification.map(|result| JsonVerification {
            mode: verify_mode.as_str().to_string(),
            command: result.command,
            passed: verified,
            attempts,
            duration_ms: result.duration_ms,
            applied: if verified {
                workspace_applied
                    .iter()
                    .map(|candidate| JsonAppliedStep {
                        kind: candidate.kind.as_str().to_string(),
                        description: candidate.description.clone(),
                        patch: candidate.patch.edits.iter().map(JsonEdit::from).collect(),
                    })
                    .collect()
            } else {
                Vec::new()
            },
        });

        let status = if verified {
            "repair_applied"
        } else {
            "repair_attempted"
        };

        return emit_fix_json(status, &errors, &proposed, false, verification, attempts);
    }

    if !verified {
        println!(
            "{} No candidate passed verification in isolation; original project left untouched.",
            "⚠".yellow().bold()
        );
    } else {
        println!(
            "{} {} verified repair{} applied to the original project.",
            "✔".green().bold(),
            workspace_applied.len(),
            if workspace_applied.len() == 1 {
                ""
            } else {
                "s"
            }
        );
    }

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

    let candidates = candidates
        .iter()
        .map(|candidate| JsonFixCandidate {
            kind: candidate.kind.as_str().to_string(),
            confidence: candidate.confidence.as_str().to_string(),
            description: candidate.description.clone(),
            evidence: candidate.evidence.clone(),
            patch: candidate.patch.edits.iter().map(JsonEdit::from).collect(),
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

fn parse_errors(output: &str) -> Vec<ParsedError> {
    let mut errors = Vec::new();

    for line in output.lines() {
        let Ok(message) = serde_json::from_str::<diagnostics::CargoMessage>(line) else {
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
