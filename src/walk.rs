use crate::analyzer;
use crate::context;
use crate::diagnostics::ParsedError;
use crate::explain;
use crate::fixer;
use colored::*;

pub fn walk_errors(errors: &[ParsedError], project_dir: &str) {
    if errors.is_empty() {
        println!(
            "\n{} {}",
            "✔".green().bold(),
            "No compiler errors found.".green().bold()
        );
        return;
    }

    println!(
        "\n{}",
        format!(
            "Walking through {} error{}...",
            errors.len(),
            if errors.len() == 1 { "" } else { "s" }
        )
        .yellow()
        .bold()
    );

    for (index, error) in errors.iter().enumerate() {
        walk_error(error, project_dir);
        pause(index + 1, errors.len());
    }
}

fn pause(current: usize, total: usize) {
    use std::io::Write;

    if current == total {
        println!(
            "\n  {} {}",
            "END".bright_black(),
            "You've covered every error.".dimmed()
        );
        return;
    }

    println!(
        "\n  {} Enter to continue to error {} of {}...",
        "▸".bright_black(),
        current + 1,
        total
    );

    let mut buffer = String::new();
    std::io::stdout().flush().ok();
    std::io::stdin().read_line(&mut buffer).ok();
}

fn walk_error(error: &ParsedError, project_dir: &str) {
    let analysis = analyzer::analyze(error);

    step_heading(
        "STEP 1",
        &format!("The problem — {}", error.code),
        Color::Red,
    );
    println!("  {}", error.raw_message.white().italic());
    println!(
        "  {} {}",
        "◦".dimmed(),
        "This is the plain message rustc reported for this error.".dimmed()
    );
    println!(
        "  {} {}",
        "◦".dimmed(),
        "A code like E0308 names a recurring class of Rust compile errors.".dimmed()
    );

    step_heading("STEP 2", "Where it happens", Color::Blue);
    let contexts = context::SourceContext::from_error(error, project_dir);

    if contexts.is_empty() {
        println!("  No source context available for this error.");
    } else {
        for source_context in &contexts {
            if contexts.len() > 1 {
                println!(
                    "  {} {}:{}",
                    "↳".bright_blue(),
                    source_context.file,
                    source_context.start_line
                );
            }

            for line in &source_context.lines {
                let marker = if line.highlighted {
                    "►".red().bold().to_string()
                } else {
                    " ".dimmed().to_string()
                };
                let gutter = format!("{:>4}", line.line_number);

                if line.highlighted {
                    println!(
                        "    {} {} {} {}",
                        marker,
                        gutter.on_red().black().bold(),
                        "│".red().bold(),
                        line.text.on_black().white()
                    );
                    if let Some(label) = &line.label {
                        println!(
                            "          {} {}",
                            "│".white().bold(),
                            format!("^ {}", label).red().bold()
                        );
                    }
                } else {
                    println!(
                        "    {} {} {} {}",
                        marker,
                        gutter.dimmed(),
                        "│".dimmed(),
                        line.text.dimmed()
                    );
                }
            }
        }
    }

    step_heading("STEP 3", "Why this is wrong", Color::Cyan);
    let explanation = explain::explain(error, &analysis);

    if let Some(concept) = &explanation.concept {
        println!(
            "  {} {} {}",
            "Concept:".cyan().bold(),
            concept.cyan(),
            "— this error lives in the core of Rust's ownership model.".dimmed()
        );
    }

    if let Some(principle) = &explanation.principle {
        println!("  {} {}", "The rule:".cyan().bold(), principle.white());
    }

    println!("  {}", explanation.plain_summary.white());

    if !analysis.relationships.is_empty() {
        println!("  {}", "The compiler links these locations:".dimmed());
        for relationship in &analysis.relationships {
            println!("    {} {}", "›".cyan(), relationship.explanation.dimmed());
        }
    }

    step_heading("STEP 4", "How to fix it", Color::Green);

    if !explanation.fix_options.is_empty() {
        for (fix_index, option) in explanation.fix_options.iter().enumerate() {
            println!("  {}. {}", fix_index + 1, option.green());
        }
    }

    if !analysis.suggestions.is_empty() {
        println!("  {}", "What rustc itself suggests:".dimmed());
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
        }
    }

    let fix = fixer::suggest_fix(error);
    println!(
        "  {} {}",
        match fix.kind {
            fixer::FixKind::CompilerSuggested => "✔".green().bold().to_string(),
            fixer::FixKind::RequiresHumanJudgment => "⚠".yellow().bold().to_string(),
        },
        fix.description
    );

    println!("\n  {}", "─".repeat(58).dimmed());
}

fn step_heading(label: &str, title: &str, color: Color) {
    println!();
    println!(
        "  {} {} {}",
        "┌".bright_black(),
        "─".repeat(label.len() + 2).bright_black(),
        "┐".bright_black()
    );
    println!(
        "  │ {} │ {}",
        label.color(color).bold(),
        title.bold().white()
    );
    println!(
        "  {} {} {}",
        "└".bright_black(),
        "─".repeat(label.len() + 2).bright_black(),
        "┘".bright_black()
    );
}
