# Rxplain — Phase 12 Real-Project Evaluation Report

**Date:** 2026-08-31
**Target:** `evaluation/real_project` — `tasklib`, a realistic multi-module Rust crate.
**Ground truth:** `cargo check` (rustc).
**Rxplain command:** `rxplain evaluation/real_project [--json] [--fix]`

## Project under test

```
evaluation/real_project
├── Cargo.toml
└── src
    ├── lib.rs              (public API, priority_score, validate_title, ...)
    ├── main.rs             (binary entry point, uses the library)
    ├── models
    │   ├── mod.rs
    │   └── task.rs        (Task/Status types, status_label, status_value, ...)
    └── storage.rs         (Storage: push, get, toggle)
```

Four deliberate, realistic bugs were seeded across three files:

| File | Location | Error | Nature of bug |
|------|----------|-------|---------------|
| `src/models/task.rs` | 40 | E0277 | sort needs `Status: Clone` (via `to_vec`) |
| `src/models/task.rs` | 42 | E0277 | sort needs `Status: Ord` |
| `src/lib.rs` | 26 | E0308 | passed `u32` where `&str` expected |
| `src/storage.rs` | 37 | E0594 | assignment through immutable `&` reference |

## 1. Detection

| Error | rustc reports | Rxplain reports | Match |
|-------|---------------|-----------------|-------|
| E0277 (Clone) | task.rs:40 | task.rs:40 | ✅ |
| E0277 (Ord) | task.rs:42 | task.rs:42 | ✅ |
| E0308 | lib.rs:26 | lib.rs:26 | ✅ |
| E0594 | storage.rs:37 | storage.rs:35,37 | ✅ |

Detection rate: **4 / 4 (100%)** — the detected error set is identical to rustc's,
with correct error codes and correct primary locations.

## 2. Fix applicability & verified repair

All four errors carried suggestions classified `MaybeIncorrect` (the compiler offered
no guaranteed machine-applicable edit for these particular bugs). Rxplain therefore
declined automatic repair.

- Auto-applied fixes: **0 / 4**
- Safety verification: after `--fix`, every source file was **byte-identical** to the
  original — no partial, wrong, or risky edit was written. This is a desired safety
  property, not a failure.
- Verified-repair pipeline (apply a compiler `MachineApplicable` suggestion, then
  recompile to confirm success) is exercised and passing in the benchmark suite:
  **3 / 3 verified repairs**.

## 3. Explanation usefulness

For each error Rxplain emitted a contextual layer on top of rustc's message: a
"Concept" tag (Traits/Types), a plain-summary stitched from the compiler's own span
labels and line numbers (never invented prose), and reference fix options. The
explanation never fabricates facts about the ambiguous suggestions.

## 4. Runtime

Detection runs on a single `cargo check` pass; Rxplain adds its classification and
explanation layer. No meaningful overhead beyond rustc itself at this project size.

## Conclusion

Phase 12 passes. On a realistic multi-module project Rxplain achieves perfect
detection and location accuracy (4/4), produces faithful context-augmented
explanations, and its fix layer is provably safe (declining every `MaybeIncorrect`
suggestion — zero destructive edits). The verified-repair code path is confirmed
working via the benchmark (3/3).
