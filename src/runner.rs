use std::process::Command;

pub fn run_cargo_build(project_dir: &str) -> anyhow::Result<String> {
    let output = Command::new("cargo")
        .args(["build", "--message-format=json"])
        .current_dir(project_dir)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    Ok(stdout.to_string())
}