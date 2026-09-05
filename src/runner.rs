use std::process::Command;

pub fn run_cargo_build(project_dir: &str) -> anyhow::Result<String> {
    let output = Command::new("cargo")
        .args(["check", "--message-format=json"])
        .current_dir(project_dir)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let has_diagnostics = stdout
        .lines()
        .any(|line| line.contains("\"reason\":\"compiler-message\""));
    let mut messages = stdout.into_owned();

    if !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() && !has_diagnostics {
            anyhow::bail!(
                "cargo check failed before producing diagnostics: {}",
                stderr.trim()
            );
        }

        messages.push_str(&stderr);
    }

    Ok(messages)
}

pub fn verify_build(project_dir: &str) -> anyhow::Result<bool> {
    let output = Command::new("cargo")
        .args(["check", "--message-format=json"])
        .current_dir(project_dir)
        .output()?;

    Ok(output.status.success())
}
