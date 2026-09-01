# Changelog

All notable changes to this project are documented in this file.
This project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
