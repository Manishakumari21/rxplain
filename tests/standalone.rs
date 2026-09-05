use rxplain::context::SourceContext;
use rxplain::diagnostics::{ParsedError, Span, SpanText};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_rxplain");
const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rxplain_it_{}_{}_{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn write_project(project: &Path, source: &str) {
    std::fs::create_dir_all(project.join("src")).unwrap();

    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
    )
    .unwrap();

    std::fs::write(project.join("src/main.rs"), source).unwrap();
}

fn run(args: &[&str], cwd: &Path) -> Output {
    Command::new(BIN)
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

const MISMATCHED: &str =
    "fn main() {\n    let number: i32 = \"hello\";\n    println!(\"{}\", number);\n}\n";
const MUTABLE: &str = "fn main() {\n    let x = 1;\n    x += 1;\n    println!(\"{}\", x);\n}\n";

#[test]
fn check_succeeds_on_engine_repository() {
    let output = run(&["check"], Path::new(MANIFEST_DIR));

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout(&output).contains("compiles successfully"));
}

#[test]
fn explain_succeeds_on_engine_repository() {
    let output = run(&["explain"], Path::new(MANIFEST_DIR));

    assert!(output.status.success());
    assert!(stdout(&output).contains("No compiler errors found"));
}

#[test]
fn check_detects_errors_in_external_project() {
    let project = temp_dir("check_external");
    write_project(&project, MISMATCHED);

    let arg = project.to_str().unwrap();
    let output = run(&["check", arg], &project);

    assert!(
        output.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let out = stdout(&output);
    assert!(out.contains("compiler error"), "out: {out}");
    assert!(out.contains("E0308"), "out: {out}");

    std::fs::remove_dir_all(&project).ok();
}

#[test]
fn explain_uses_external_project_source() {
    let project = temp_dir("explain_external");
    write_project(&project, MISMATCHED);

    let arg = project.to_str().unwrap();
    let output = run(&["explain", arg], &project);

    assert!(output.status.success());

    let out = stdout(&output);
    assert!(out.contains("E0308"), "out: {out}");
    assert!(out.contains("Mismatched types"), "out: {out}");
    assert!(
        out.contains("src/main.rs"),
        "relative location shown: {out}"
    );
    assert!(
        out.contains("let number: i32 = \"hello\";"),
        "source context must come from the external project: {out}"
    );

    std::fs::remove_dir_all(&project).ok();
}

#[test]
fn explain_json_is_valid_json_without_extra_stdout() {
    let project = temp_dir("json_external");
    write_project(&project, MISMATCHED);

    let arg = project.to_str().unwrap();
    let output = run(&["explain", "--json", arg], &project);

    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("stdout must be pure JSON");

    let errors = value
        .get("errors")
        .and_then(|entry| entry.as_array())
        .expect("errors array present");

    assert!(!errors.is_empty(), "at least one error expected");
    assert_eq!(errors[0]["code"], "E0308");

    std::fs::remove_dir_all(&project).ok();
}

#[test]
fn fix_dry_run_detects_fixes_without_modifying_external_project() {
    let project = temp_dir("dry_run_external");
    write_project(&project, MUTABLE);

    let main_rs = project.join("src/main.rs");
    let before = std::fs::read_to_string(&main_rs).unwrap();

    let arg = project.to_str().unwrap();
    let output = run(&["fix", "--dry-run", arg], &project);

    assert!(output.status.success());

    let out = stdout(&output);
    assert!(out.contains("safe fix"), "out: {out}");
    assert!(out.contains("Dry run"), "out: {out}");

    let after = std::fs::read_to_string(&main_rs).unwrap();
    assert_eq!(before, after, "dry run must not modify any files");

    std::fs::remove_dir_all(&project).ok();
}

#[test]
fn fix_applies_to_external_project_and_verifies() {
    let project = temp_dir("fix_external");
    write_project(&project, MUTABLE);

    let main_rs = project.join("src/main.rs");
    let engine_main_rs = Path::new(MANIFEST_DIR).join("src/main.rs");
    let engine_before = std::fs::read_to_string(&engine_main_rs).unwrap();

    let arg = project.to_str().unwrap();
    let output = run(&["fix", arg], &project);

    assert!(output.status.success());

    let out = stdout(&output);
    assert!(out.contains("Fixes applied successfully"), "out: {out}");
    assert!(
        out.contains("Project now compiles successfully"),
        "post-fix cargo check must run: {out}"
    );

    let after = std::fs::read_to_string(&main_rs).unwrap();
    assert!(
        after.contains("let mut x = 1;"),
        "fix must be applied inside the external project: {after}"
    );

    let engine_after = std::fs::read_to_string(&engine_main_rs).unwrap();
    assert_eq!(
        engine_before, engine_after,
        "engine repository must not be modified"
    );

    std::fs::remove_dir_all(&project).ok();
}

#[test]
fn source_context_resolves_relative_to_external_project() {
    let project = temp_dir("ctx_external");
    write_project(&project, MISMATCHED);

    let error = ParsedError {
        code: "E0308".to_string(),
        raw_message: "mismatched types".to_string(),
        spans: vec![Span {
            file_name: "src/main.rs".to_string(),
            line_start: 2,
            line_end: 2,
            column_start: 17,
            column_end: 24,
            label: Some("expected `i32`, found `&str`".to_string()),
            text: vec![SpanText {
                text: "let number: i32 = \"hello\";".to_string(),
            }],
            suggested_replacement: None,
            suggestion_applicability: None,
        }],
        suggestions: Vec::new(),
    };

    let contexts = SourceContext::from_error(&error, &project.to_string_lossy());

    assert!(
        !contexts.is_empty(),
        "source context should resolve inside the external project"
    );
    assert!(
        contexts[0]
            .lines
            .iter()
            .any(|line| line.text.contains("let number: i32 = \"hello\";")),
        "context must contain the external project's source text"
    );

    std::fs::remove_dir_all(&project).ok();
}

#[test]
fn source_context_resolves_in_nested_workspace_crate() {
    let workspace = temp_dir("ctx_workspace");
    std::fs::create_dir_all(workspace.join("crates/backend/src")).unwrap();

    std::fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/backend\"]\n",
    )
    .unwrap();

    std::fs::write(
        workspace.join("crates/backend/src/main.rs"),
        "fn main() {\n    let number: i32 = \"hello\";\n    println!(\"{}\", number);\n}\n",
    )
    .unwrap();

    let error = ParsedError {
        code: "E0308".to_string(),
        raw_message: "mismatched types".to_string(),
        spans: vec![Span {
            file_name: "src/main.rs".to_string(),
            line_start: 2,
            line_end: 2,
            column_start: 17,
            column_end: 24,
            label: Some("expected `i32`, found `&str`".to_string()),
            text: vec![SpanText {
                text: "let number: i32 = \"hello\";".to_string(),
            }],
            suggested_replacement: None,
            suggestion_applicability: None,
        }],
        suggestions: Vec::new(),
    };

    let contexts = SourceContext::from_error(&error, &workspace.to_string_lossy());

    assert!(
        !contexts.is_empty(),
        "nested workspace crate source should resolve"
    );
    assert!(
        contexts[0].file.ends_with("crates/backend/src/main.rs"),
        "resolved to the workspace member file, got: {}",
        contexts[0].file
    );

    std::fs::remove_dir_all(&workspace).ok();
}
