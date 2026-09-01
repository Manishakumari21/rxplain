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
    /// Path to the Rust project
    project_dir: String,

    /// Automatically apply machine-applicable compiler suggestions
    #[arg(long)]
    fix: bool,

    /// Output the diagnostic report as JSON
    #[arg(long)]
    json: bool,

    /// Walk through each error step-by-step, like a guided tutorial
    #[arg(long)]
    walk: bool,

    /// Launch the interactive full-screen terminal UI
    #[arg(long)]
    tui: bool,

    /// Suppress the decorative banner
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

fn main() -> Result<()> {
    let cli = Cli::parse();

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

    if cli.json {
        print_json_report(&errors)?;
    } else if cli.walk {
        walk::walk_errors(&errors, &cli.project_dir);
    } else if cli.tui {
        if std::io::IsTerminal::is_terminal(&std::io::stdout())
            && std::io::IsTerminal::is_terminal(&std::io::stdin())
        {
            crate::tui::run(&errors, &cli)?;
        } else {
            eprintln!("note: --tui requires an interactive terminal, falling back to plain output");
            print_human_report(&errors, &cli, started);
        }
    } else {
        print_human_report(&errors, &cli, started);
    }

    Ok(())
}

fn print_banner() {
    println!();
    for (index, line) in BANNER.lines().enumerate() {
        let color = match index {
            0 | 5 => Color::BrightRed,
            1 | 4 => Color::BrightYellow,
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

        let contexts = context::SourceContext::from_error(error, &cli.project_dir);

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

                    if cli.fix {
                        println!("\n  {} {}", "⏳".yellow(), "Applying fix...".yellow());

                        match fixer::apply_fixes(&error.suggestions, &cli.project_dir) {
                            Ok(()) => {
                                println!(
                                    "  {} {}",
                                    "✔".green().bold(),
                                    "Fix applied successfully.".green().bold()
                                );

                                println!("  {} {}", "⏳".yellow(), "Verifying project...".yellow());

                                match runner::verify_build(&cli.project_dir) {
                                    Ok(true) => {
                                        println!(
                                            "  {} {}",
                                            "✔".green().bold(),
                                            "Project now compiles successfully.".green().bold()
                                        );
                                    }

                                    Ok(false) => {
                                        println!(
                                            "  {} {}",
                                            "✖".red().bold(),
                                            "Project still has compiler errors.".red().bold()
                                        );
                                    }

                                    Err(error) => {
                                        println!(
                                            "  {} {} {}",
                                            "✖".red().bold(),
                                            "Failed to verify project:".red().bold(),
                                            error
                                        );
                                    }
                                }
                            }

                            Err(error) => {
                                println!(
                                    "\n  {} {} {}",
                                    "✖".red().bold(),
                                    "Failed to apply fix:".red().bold(),
                                    error
                                );
                            }
                        }
                    }
                }
            }

            fixer::FixKind::RequiresHumanJudgment => {
                println!("    {} {}", "⚠".yellow().bold(), fix.description.yellow());

                if cli.fix {
                    println!(
                        "    {} {}",
                        "⊘".yellow(),
                        "No automatic fix was applied.".yellow()
                    );
                }
            }
        }

        println!("\n{}", format!("└{}┘", "─".repeat(58)).bright_black());
        println!();
    }
}
