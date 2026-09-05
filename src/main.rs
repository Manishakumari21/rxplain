use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
use serde::Serialize;
use std::time::Instant;

mod analyzer;
mod context;
mod diagnostics;
mod explain;
mod fixer;
mod runner;
mod tui;
mod walk;

use diagnostics::ParsedError;

#[derive(Parser, Debug)]
#[command(
    name = "rxplain",
    version,
    about = "A deterministic, offline explainer for Rust compiler errors"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check whether a Rust project compiles
    Check {
        #[arg(default_value = ".")]
        project_dir: String,
    },

    /// Explain Rust compiler errors
    Explain {
        #[arg(default_value = ".")]
        project_dir: String,

        #[arg(long)]
        json: bool,

        #[arg(long)]
        walk: bool,

        #[arg(long)]
        tui: bool,

        #[arg(long)]
        quiet: bool,
    },

    /// Safely apply compiler-suggested fixes
    Fix {
        #[arg(default_value = ".")]
        project_dir: String,

        #[arg(long)]
        dry_run: bool,
    },
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Check { project_dir } => check_command(&project_dir),

        Command::Explain {
            project_dir,
            json,
            walk,
            tui,
            quiet,
        } => explain_command(&project_dir, json, walk, tui, quiet),

        Command::Fix {
            project_dir,
            dry_run,
        } => fix_command(&project_dir, dry_run),
    }
}

fn check_command(project_dir: &str) -> Result<()> {
    let started = Instant::now();

    let output = runner::run_cargo_build(project_dir)?;
    let errors = parse_errors(&output);

    let elapsed = started.elapsed();

    if errors.is_empty() {
        println!(
            "{} Rust project compiles successfully. {}",
            "✔".green().bold(),
            format!("({})", HumanDuration(elapsed)).dimmed()
        );
    } else {
        println!(
            "{} {} compiler error{} found. {}",
            "✖".red().bold(),
            errors.len(),
            if errors.len() == 1 { "" } else { "s" },
            format!("({})", HumanDuration(elapsed)).dimmed()
        );

        for error in &errors {
            println!(
                "  {} {}: {}",
                "→".red(),
                error.code.red().bold(),
                error.raw_message
            );
        }

        println!();
        println!(
            "Run {} for detailed explanations.",
            "rxplain explain".cyan().bold()
        );
    }

    Ok(())
}

fn explain_command(
    project_dir: &str,
    json: bool,
    walk: bool,
    tui: bool,
    quiet: bool,
) -> Result<()> {
    let started = Instant::now();

    let output = if json || tui {
        runner::run_cargo_build(project_dir)?
    } else if quiet {
        runner::run_cargo_build(project_dir)?
    } else {
        let spinner = ProgressBar::new_spinner();

        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["▖", "▘", "▝", "▗", "▖", "▘", "▝", "▗"])
                .template("{spinner:.yellow} {msg}")
                .unwrap(),
        );

        spinner.set_message(format!("Analyzing {}...", project_dir));

        let result = runner::run_cargo_build(project_dir);

        spinner.finish_and_clear();

        result?
    };

    let errors = parse_errors(&output);

    let cli = Cli {
        command: Command::Explain {
            project_dir: project_dir.to_string(),
            json,
            walk,
            tui,
            quiet,
        },
    };

    if json {
        print_json_report(&errors)?;
    } else if walk {
        walk::walk_errors(&errors, project_dir);
    } else if tui {
        if std::io::IsTerminal::is_terminal(&std::io::stdout())
            && std::io::IsTerminal::is_terminal(&std::io::stdin())
        {
            crate::tui::run(&errors, project_dir)?;
        } else {
            eprintln!(
                "note: --tui requires an interactive terminal, \
                 falling back to plain output"
            );

            print_human_report(&errors, &cli, started);
        }
    } else {
        print_human_report(&errors, &cli, started);
    }

    Ok(())
}

fn fix_command(project_dir: &str, dry_run: bool) -> Result<()> {
    let started = Instant::now();

    let output = runner::run_cargo_build(project_dir)?;
    let errors = parse_errors(&output);

    if errors.is_empty() {
        println!(
            "{} No compiler errors found. Nothing to fix.",
            "✔".green().bold()
        );
        return Ok(());
    }

    let mut suggestions = Vec::new();

    for error in &errors {
        for suggestion in &error.suggestions {
            if suggestion.applicability == "MachineApplicable" {
                suggestions.push(suggestion.clone());
            }
        }
    }

    if suggestions.is_empty() {
        println!(
            "{} No safe compiler-suggested fixes were found.",
            "⚠".yellow().bold()
        );

        println!("  The remaining errors require human judgment.");

        return Ok(());
    }

    println!(
        "{} {} safe fix{} found.",
        "🔧".cyan(),
        suggestions.len(),
        if suggestions.len() == 1 { "" } else { "es" }
    );

    for suggestion in &suggestions {
        println!();
        println!(
            "  {} {}:{}:{}",
            "→".cyan(),
            suggestion.file,
            suggestion.line,
            suggestion.column
        );

        println!(
            "    {} {}",
            "Replacement:".dimmed(),
            suggestion.replacement.green()
        );

        if let Some(label) = &suggestion.label {
            println!("    {} {}", "Reason:".dimmed(), label.dimmed());
        }

        println!(
            "    {} {}",
            "Safety:".dimmed(),
            suggestion.applicability.green()
        );
    }

    if dry_run {
        println!();
        println!("{} Dry run — no files were modified.", "ℹ".cyan().bold());

        return Ok(());
    }

    println!();
    println!("{} Applying safe compiler suggestions...", "⏳".yellow());

    fixer::apply_fixes(&suggestions, project_dir)?;

    println!("{} Fixes applied successfully.", "✔".green().bold());

    println!("{} Verifying project...", "⏳".yellow());

    match runner::verify_build(project_dir)? {
        true => {
            println!("{} Project now compiles successfully.", "✔".green().bold());
        }

        false => {
            println!("{} Project still has compiler errors.", "✖".red().bold());

            println!(
                "  Run {} to inspect the remaining errors.",
                "rxplain explain".cyan()
            );
        }
    }

    println!(
        "{}",
        format!("Completed in {}", HumanDuration(started.elapsed())).dimmed()
    );

    Ok(())
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

fn print_human_report(errors: &[ParsedError], cli: &Cli, started: Instant) {
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

        let contexts = context::SourceContext::from_error(error, &project_dir_from_cli(cli));

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

fn project_dir_from_cli(cli: &Cli) -> String {
    match &cli.command {
        Command::Check { project_dir } => project_dir.clone(),

        Command::Explain { project_dir, .. } => project_dir.clone(),

        Command::Fix { project_dir, .. } => project_dir.clone(),
    }
}
