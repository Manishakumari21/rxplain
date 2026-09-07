use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_SECS: u64 = 120;

pub fn timeout_secs() -> u64 {
    std::env::var("RXPLAIN_TIMEOUT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
}

pub fn run_command_with_timeout(
    command: &mut Command,
    timeout: Duration,
    what: &str,
) -> anyhow::Result<std::process::Output> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");

    let out_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let mut reader = std::io::BufReader::new(stdout);
        let _ = reader.read_to_end(&mut buffer);
        buffer
    });

    let err_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let mut reader = std::io::BufReader::new(stderr);
        let _ = reader.read_to_end(&mut buffer);
        buffer
    });

    let pid = child.id();
    let deadline = Instant::now() + timeout;

    let status = loop {
        if Instant::now() >= deadline {
            kill_process_group(pid);
            let _ = child.wait();
            anyhow::bail!(
                "{} exceeded the {}s timeout (set RXPLAIN_TIMEOUT to adjust)",
                what,
                timeout.as_secs()
            );
        }

        match child.try_wait()? {
            Some(status) => break status,
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };

    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();

    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    let _ = Command::new("kill")
        .arg("-9")
        .arg(format!("-{pid}"))
        .status();
}

pub fn run_cargo_build(project_dir: &str) -> anyhow::Result<String> {
    let mut command = Command::new("cargo");
    command
        .args(["check", "--message-format=json"])
        .current_dir(project_dir);

    let output = run_command_with_timeout(
        &mut command,
        Duration::from_secs(timeout_secs()),
        "cargo check",
    )?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_captures_output() {
        let mut command = Command::new("echo");
        command.arg("hello");
        let output =
            run_command_with_timeout(&mut command, Duration::from_secs(5), "echo").unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
    }

    #[test]
    fn timeout_terminates_sleeping_process() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]);
        let err = run_command_with_timeout(&mut command, Duration::from_millis(500), "sleep")
            .err()
            .expect("long-running child must time out");
        assert!(err.to_string().contains("timeout"));
    }
}
