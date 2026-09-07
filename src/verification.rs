use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyMode {
    Check,
    Build,
    Test,
}

impl VerifyMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            VerifyMode::Check => "check",
            VerifyMode::Build => "build",
            VerifyMode::Test => "test",
        }
    }

    pub fn cargo_args(&self) -> &'static [&'static str] {
        match self {
            VerifyMode::Check => &["check", "--message-format=json"],
            VerifyMode::Build => &["build", "--message-format=json"],
            VerifyMode::Test => &["test", "--message-format=json"],
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VerificationResult {
    pub passed: bool,
    pub exit_status: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
    pub command: String,
}

pub struct IsolatedWorkspace {
    dir: PathBuf,
}

const KEEP_DIRS: &[&str] = &["target", ".git", ".hg", ".svn"];

const ISOLATED_TIMEOUT_SECS: u64 = 300;

impl IsolatedWorkspace {
    pub fn create(source_dir: &str) -> anyhow::Result<Self> {
        let source = Path::new(source_dir);

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        let dest =
            std::env::temp_dir().join(format!("rxplain_ws_{}_{}", std::process::id(), stamp));

        Self::copy_project(source, &dest)?;

        Ok(Self { dir: dest })
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }

    fn copy_project(src: &Path, dest: &Path) -> anyhow::Result<()> {
        Self::copy_dir(src, dest)
    }

    fn copy_dir(src: &Path, dest: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dest)?;

        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if KEEP_DIRS.contains(&name_str.as_ref()) {
                continue;
            }

            let src_path = entry.path();
            let dest_path = dest.join(&name);

            if src_path.is_dir() {
                Self::copy_dir(&src_path, &dest_path)?;
            } else if src_path.is_file() {
                std::fs::copy(&src_path, &dest_path)?;
            }
        }

        Ok(())
    }
}

impl Drop for IsolatedWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub fn verify_in_workspace(
    workspace: &IsolatedWorkspace,
    mode: VerifyMode,
) -> anyhow::Result<VerificationResult> {
    let started = Instant::now();

    let output = run_cargo(workspace.path(), mode)?;

    let duration_ms = started.elapsed().as_millis();

    let passed = output.status.success();

    Ok(VerificationResult {
        passed,
        exit_status: passed,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        duration_ms,
        command: format!("cargo {}", mode.cargo_args().join(" ")),
    })
}

fn run_cargo(dir: &Path, mode: VerifyMode) -> anyhow::Result<std::process::Output> {
    let timeout_secs = std::env::var("RXPLAIN_TIMEOUT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(ISOLATED_TIMEOUT_SECS);

    let mut command = Command::new("cargo");
    command.args(mode.cargo_args()).current_dir(dir);

    crate::runner::run_command_with_timeout(
        &mut command,
        Duration::from_secs(timeout_secs),
        &format!("cargo {} in the isolated workspace", mode.as_str()),
    )
}

pub fn reproduce_failure(project_dir: &str, mode: VerifyMode) -> anyhow::Result<bool> {
    let workspace = IsolatedWorkspace::create(project_dir)?;
    let result = verify_in_workspace(&workspace, mode)?;
    Ok(!result.passed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_broken_project(dir: &Path) {
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"broken\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/main.rs"),
            "fn main() {\n    let x: i32 = \"nope\";\n    println!(\"{}\", x);\n}\n",
        )
        .unwrap();
    }

    #[test]
    fn isolated_workspace_reproduces_failure_and_is_garbage_collected() {
        let project = std::env::temp_dir().join(format!(
            "rxplain_ws_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        write_broken_project(&project);

        let reproduced = reproduce_failure(project.to_str().unwrap(), VerifyMode::Check).unwrap();
        assert!(
            reproduced,
            "broken project must fail to compile in isolation"
        );

        let ws = IsolatedWorkspace::create(project.to_str().unwrap()).unwrap();
        let ws_path = ws.path().to_path_buf();
        assert!(ws_path.exists());
        drop(ws);
        assert!(!ws_path.exists(), "workspace must be cleaned up on drop");

        fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn isolated_workspace_skips_target_and_git() {
        let project = std::env::temp_dir().join(format!(
            "rxplain_ws_skip_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        write_broken_project(&project);
        fs::create_dir_all(project.join("target")).unwrap();
        fs::create_dir_all(project.join(".git")).unwrap();

        let ws = IsolatedWorkspace::create(project.to_str().unwrap()).unwrap();

        assert!(!ws.path().join("target").exists());
        assert!(!ws.path().join(".git").exists());

        fs::remove_dir_all(&project).ok();
    }

    #[test]
    fn verify_mode_args_map_to_cargo_commands() {
        assert_eq!(
            VerifyMode::Check.cargo_args(),
            &["check", "--message-format=json"]
        );
        assert_eq!(
            VerifyMode::Build.cargo_args(),
            &["build", "--message-format=json"]
        );
        assert_eq!(
            VerifyMode::Test.cargo_args(),
            &["test", "--message-format=json"]
        );
        assert_eq!(VerifyMode::Check.as_str(), "check");
        assert_eq!(VerifyMode::Build.as_str(), "build");
        assert_eq!(VerifyMode::Test.as_str(), "test");
    }

    #[test]
    fn verified_project_passes_test_mode() {
        let project = std::env::temp_dir().join(format!(
            "rxplain_ws_pass_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        write_broken_project(&project);
        fs::write(
            project.join("src/main.rs"),
            "fn main() {\n    let x: i32 = 1;\n    println!(\"{}\", x);\n}\n",
        )
        .unwrap();

        let ws = IsolatedWorkspace::create(project.to_str().unwrap()).unwrap();
        let result = verify_in_workspace(&ws, VerifyMode::Test).unwrap();
        assert!(result.passed, "cargo test must pass on a valid project");

        fs::remove_dir_all(&project).ok();
    }
}
