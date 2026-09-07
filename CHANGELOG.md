# Changelog

All notable changes to this project are documented in this file.
This project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Fixer no longer mis-applies or panics on non-ASCII source: rustc character
  columns are now converted to byte offsets before slicing.
- Compiler suggestions attached to top-level diagnostic spans are captured, not
  just suggestions under `children`.
- Type extraction (E0308) no longer truncates generic types at a comma inside a
  backtick-quoted type name (e.g. `HashMap<String, u32>`).
- Source-context windows now cover the full extent of multi-line spans instead
  of being drawn only around the first line.
- E0382 explanations identify the actual move/use sites instead of assuming the
  first and last labels are the move and use.
- Removed redundant identical `cargo` execution branches in the `explain` path.
- `--fix` back-propagates the *complete* verified patch set to the original
  project: chained repairs no longer drop the earlier fix steps.

### Added

- Bounded repair loop in `--fix`: when a candidate fails the oracle, the engine
  re-reads the *new* diagnostics and chains follow-up fixes (max 4 attempts, loop
  guard via patch signatures) so multi-step repairs like a full lifetime
  annotation are applied as one verified patch set.
- `--fix --verify check|build|test`: choose the oracle command (`cargo check` by
  default, `cargo build`, or `cargo test`).
- `--fix --json` now runs the verification loop first and reports a populated
  `verification` block (mode, command, passed, attempts, duration, applied
  patch steps) instead of `null`.
- Timeout now hard-kills the entire cargo/rustc process tree (own process group)
  rather than leaving orphaned children.
- `runner.rs` shared `run_command_with_timeout` helper with process-tree cleanup.
- `--fix --dry-run`: preview ranked candidate repairs without modifying files.
- `--fix --json`: structured candidate + patch output for tooling.
- `repair.rs`: repair candidates with explicit `RepairKind`, `Confidence`,
  deterministic ranking, and a patch-signature loop guard.
- `patch.rs`: reviewable patches with `preview`, `validate`, and `apply`.
- `verification.rs`: isolated workspace copy (skips `target/` and VCS dirs) used
  as a `cargo check`/`build`/`test` oracle; a candidate is applied to the real
  project only after it verifies in isolation.
- Timeout for every `cargo check` call (default 120s, `RXPLAIN_TIMEOUT`).

## [0.1.0] - 2026-08-31

Initial release.

### Added

- Read Rust compiler diagnostics via `cargo check --message-format=json`.
- Span-aware explanations with concept tagging.
- Specialized explanations for 15 error codes:
  `E0106`, `E0277`, `E0282`, `E0308`, `E0382`, `E0384`, `E0432`, `E0433`,
  `E0499`, `E0502`, `E0505`, `E0596`, `E0597`, `E0599`, `E0716`.
- Generic fallback that surfaces the compiler's own machine-applicable suggestion.
- Safe `--fix` that applies only `MachineApplicable` suggestions with post-fix
  build verification.
- Human-readable and `--json` output, `--version`.
- Benchmark harness (`benchmark/run.sh`, 13 cases) and guided demo (`demo/demo.sh`).
- Phase-12 real-project evaluation report (`evaluation/report.md`).
- 20 automated integration tests.
