use std::process::Command;

pub fn run_cargo_build(project_dir: &str) -> anyhow::Result<String> {
    let output = Command::new("cargo")
        .args(["check", "--message-format=json"])
        .current_dir(project_dir)
        .output()?;

    let mut messages = String::from_utf8_lossy(&output.stdout).into_owned();

    if !output.stderr.is_empty() {
        messages.push_str(&String::from_utf8_lossy(&output.stderr));
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
