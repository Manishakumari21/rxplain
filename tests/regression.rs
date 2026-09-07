// Regression and safety tests for the repair engine:
//  * a failed repair must never touch the original project
//  * dry-run previews must report candidates as unverified
//  * multi-error projects still terminate and report honestly
//  * the engine repository itself is never modified by a run

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_rxplain");
const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rxplain_reg_{}_{}_{}",
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

const MUTABLE: &str = "fn main() {\n    let x = 1;\n    x += 1;\n    println!(\"{}\", x);\n}\n";

/// An error with no expressible fix: a closure that escapes a borrowed value.
/// The engine must report it honestly and leave the project untouched.
const NO_FIX: &str = "fn main() {\n    let mut list: Vec<&str> = Vec::new();\n    let _add = |el: &str| {\n        list.push(el);\n    };\n}\n";

/// Two independent errors in separate functions; repair must still terminate
/// with an honest status (here: no safe single-shot fix, so human review is
/// expected).
const TWO_ERRORS: &str = "fn accumulate() {\n    let x = 1;\n    x += 1;\n}\n\nfn parse() {\n    let s: i32 = \"hello\";\n}\n\nfn main() {\n    accumulate();\n    parse();\n}\n";

#[test]
fn failed_repair_leaves_external_project_untouched() {
    let project = temp_dir("no_fix");
    write_project(&project, NO_FIX);

    let main_rs = project.join("src/main.rs");
    let before = std::fs::read_to_string(&main_rs).unwrap();

    let arg = project.to_str().unwrap();
    let output = run(&["--fix", arg], &project);

    assert!(output.status.success());
    let out = stdout(&output);
    assert!(
        out.contains("human judgment"),
        "no-fix error must ask for human review: {out}"
    );

    let after = std::fs::read_to_string(&main_rs).unwrap();
    assert_eq!(
        before, after,
        "an unsuccessful repair must not modify the source"
    );

    std::fs::remove_dir_all(&project).ok();
}

#[test]
fn dry_run_json_previews_candidates_with_null_verdict() {
    let project = temp_dir("dry_json");
    write_project(&project, MUTABLE);

    let arg = project.to_str().unwrap();
    let output = run(&["--fix", "--dry-run", "--json", arg], &project);

    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("stdout must be pure JSON");

    assert_eq!(value["status"], "preview");
    assert_eq!(value["dry_run"], true);

    let candidates = value["candidates"].as_array().unwrap();
    assert!(!candidates.is_empty(), "dry run must propose candidates");
    for candidate in candidates {
        assert_eq!(
            candidate["verified"],
            serde_json::Value::Null,
            "previews are not verified yet"
        );
    }

    let main_rs = project.join("src/main.rs");
    let after = std::fs::read_to_string(&main_rs).unwrap();
    assert!(
        after.contains("let x = 1;"),
        "dry run must not modify files"
    );

    std::fs::remove_dir_all(&project).ok();
}

#[test]
fn multi_error_project_reports_honest_status_and_terminates() {
    let project = temp_dir("multi");
    write_project(&project, TWO_ERRORS);

    let arg = project.to_str().unwrap();
    let output = run(&["--fix", "--json", arg], &project);

    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_str(&stdout(&output)).expect("stdout must be pure JSON");

    let status = value["status"].as_str().unwrap();
    assert!(
        [
            "repair_applied",
            "multiple_verified",
            "repair_attempted",
            "human_review_required"
        ]
        .contains(&status),
        "status must be an honest, known outcome: {status}"
    );

    let diagnostics = value["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics.len(),
        2,
        "both independent errors must be reported"
    );

    std::fs::remove_dir_all(&project).ok();
}

#[test]
fn engine_repository_untouched_by_external_repair() {
    let project = temp_dir("engine_touch");
    write_project(&project, MUTABLE);

    let engine_main_rs = Path::new(MANIFEST_DIR).join("src/main.rs");
    let engine_before = std::fs::read_to_string(&engine_main_rs).unwrap();

    let arg = project.to_str().unwrap();
    let output = run(&["--fix", arg], &project);
    assert!(output.status.success());

    let engine_after = std::fs::read_to_string(&engine_main_rs).unwrap();
    assert_eq!(
        engine_before, engine_after,
        "running against an external project must not touch the engine repository"
    );

    std::fs::remove_dir_all(&project).ok();
}
