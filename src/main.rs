mod diagnostics;
mod explain;
mod runner;

use clap::Parser;
use colored::*;

#[derive(Parser)]
#[command(name = "rxplain", about = "A deterministic Rust compiler error explainer")]
struct Cli {
    #[arg(default_value = ".")]
    project_dir: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    println!("{}", "Building project...".dimmed());

    let output = runner::run_cargo_build(&cli.project_dir)?;

    let mut errors = Vec::new();

    for line in output.lines() {
        let Ok(cargo_message) =
            serde_json::from_str::<diagnostics::CargoMessage>(line)
        else {
            continue;
        };

        if cargo_message.reason != "compiler-message" {
            continue;
        }

        if let Some(rustc_message) = cargo_message.message {
            if let Some(error) =
                diagnostics::ParsedError::from_rustc_message(&rustc_message)
            {
                errors.push(error);
            }
        }
    }

    if errors.is_empty() {
        println!("{}", "\n✓ No compiler errors found.".green().bold());
        return Ok(());
    }

    println!(
        "\n{} {}",
        errors.len().to_string().red().bold(),
        if errors.len() == 1 {
            "error found".red().bold()
        } else {
            "errors found".red().bold()
        }
    );

    for (index, error) in errors.iter().enumerate() {
        let explanation = explain::explain(error);

        println!(
            "\n{} {} {}",
            format!("[{}/{}]", index + 1, errors.len()).dimmed(),
            error.code.red().bold(),
            explanation.title.bold()
        );

        println!(
            "{} {}:{}",
            "Location:".blue().bold(),
            error.file,
            error.primary_line
        );

        println!(
            "\n{}",
            error.primary_snippet.bright_white()
        );

        println!("\n{}", "What happened:".yellow().bold());
        println!("  {}", explanation.plain_summary);

        println!("\n{}", "Fix options:".yellow().bold());

        for (number, fix) in explanation.fix_options.iter().enumerate() {
            println!("  {}. {}", number + 1, fix);
        }

        println!("\n{}", "-".repeat(60).dimmed());
    }

    Ok(())
}