# rxplain

**A deterministic, offline Rust compiler error explainer with safe auto-fixes.**

`rxplain` reads Rust's structured compiler diagnostics, explains what went wrong using the actual source context, and applies fixes only when `rustc` marks them `MachineApplicable`.

## ✨ Features

* Context-aware Rust compiler error explanations
* The **rule behind the concept** — teaches *why* the rule exists, not just *where*
* Source locations, spans, and related diagnostics
* Compiler-provided suggestions
* Safe `--fix` for `MachineApplicable` suggestions
* Automatic build verification after fixes
* `--walk` step-by-step tutorial mode for ownership and borrow errors
* `--tui` interactive terminal browser (ratatui)
* JSON output for tools and integrations
* Works offline — no AI API or API key required

## 🔧 How it works

```text
Rust project
     ↓
cargo check --message-format=json
     ↓
Parse diagnostics
     ↓
Analyze source context
     ↓
Explain error
     ↓
Generate candidate repairs (compiler suggestions + transformations)
     ↓
Rank candidates (compiler suggestion first, minimal change preferred)
     ↓
Apply the top candidate in an isolated workspace copy
     ↓
cargo check/build/test in isolation (compiler as oracle)
     ↓
Rejected → re-read the new diagnostics and try the follow-up fix (bounded repair loop)
     ↓
Verified → apply the complete patch set to the original project
```

## 🚀 Install

Requires Rust and Cargo.

Via `cargo install`:

```bash
cargo install --path .
```

Or with the bundled installer script (builds the release binary):

```bash
./scripts/install.sh                # to ~/.cargo/bin
./scripts/install.sh ~/.local/bin   # to a custom directory
```

Then:

```bash
rxplain --version
```

## ▶️ Usage

Analyze a project:

```bash
rxplain .
```

Analyze another project:

```bash
rxplain /path/to/project
```

Apply safe compiler fixes (verified in an isolated workspace before touching
your files):

```bash
rxplain . --fix
```

Preview a fix without modifying anything:

```bash
rxplain . --fix --dry-run
```

Get structured candidates/patch output as JSON:

```bash
rxplain . --fix --json
```

Get JSON output:

```bash
rxplain . --json
```

Walk through each error step by step (great for learning ownership and borrowing):

```bash
rxplain . --walk
```

Browse errors in an interactive terminal viewer (`j`/`k` or `↑`/`↓` to select,
`Space`/`PgUp`/`PgDn` to scroll, `q` to quit):

```bash
rxplain . --tui
```

Run the guided demo:

```bash
bash demo/demo.sh
```

### 📋 Quick reference (all commands)

| Command | What it does |
| --- | --- |
| `rxplain .` | Explain errors in the current project |
| `rxplain /path/to/project` | Explain errors in another project |
| `rxplain . --fix` | Apply a safe fix (verified in an isolated sandbox first) |
| `rxplain . --fix --dry-run` | Preview the fix — no files changed |
| `rxplain . --fix --json` | Fix candidates + patch as JSON |
| `rxplain . --fix --verify build` | Oracle = `cargo build` (default `check`) |
| `rxplain . --fix --verify test` | Oracle = `cargo test` |
| `rxplain . --json` | Full report as JSON |
| `rxplain . --walk` | Step-by-step tutorial for ownership/borrow errors |
| `rxplain . --tui` | Interactive terminal browser (`j`/`k` navigate, `q` quit) |
| `rxplain . --quiet` | Plain output (no banner) |
| `rxplain --version` | Show version |
| `rxplain --help` | Show all options |

**Easy to remember:** one letter per flag — `f`ix, `d`ry-run, `j`son,
`w`alk, `t`ui, `q`uiet.

> Tip: if `rxplain . --fix` errors with "unrecognized subcommand", run
> `cargo install --path .` to update the installed binary.

## 🛠️ Before / after — what rxplain adds

Rust beginners often hit an ownership or borrowing error and stare at the bare compiler
lines, unsure *where* the conflict is or *why* the rule exists. Here is the same E0502
error before and after rxplain.

**Before — raw compiler output:**

```
error[E0502]: cannot borrow `value` as mutable because it is also borrowed as immutable
 --> src/main.rs:4:29
  |
3 |     let reference = &value;
  |              ----- immutable borrow occurs here
4 |     let mutable_reference = &mut value;
  |                             ^^^^^^ mutable borrow occurs here
5 |     println!("{}", reference);
  |              -------- immutable borrow later used here
```

**After — `rxplain .`:**

```text
✖ 1 error found in 0 seconds

   ERROR E0502

  ▸ Compiler message
    cannot borrow `value` as mutable because it is also borrowed as immutable

  ▸ Compiler evidence
    ● src/main.rs:3:21   let reference = &value;
      └─ immutable borrow occurs here
    ● src/main.rs:4:29   let mutable_reference = &mut value;
      └─ mutable borrow occurs here
    ● src/main.rs:5:20   println!("{}", reference);
      └─ immutable borrow later used here

  ▸ Why these locations are related
    › Line 3 is related to line 4: immutable borrow → mutable borrow.
    › Line 5 is the last use of the immutable borrow.

  ┌ ────────── ┐
  │ Conflicting borrows (E0502) │
  └ ────────── ┘
    🏷 Concept: Borrowing
    💭 The rule: At any moment a value can have either many immutable references
      or one mutable reference — never both, because concurrent reads and
      writes would race.
    The immutable reference `reference` is still in use when `&mut value` is created.

  🔧 Possible fixes
    • Ensure the immutable borrow ends before the mutable borrow begins.
    • Narrow the scope of the immutable borrow.
    • Clone the value if an owned copy is acceptable.
```

rxplain ties every line of the compiler diagnostic to the *reason* behind it (Ownership,
Borrowing, Lifetimes), shows the surrounding source, and lists concrete fixes — so a
confused new user knows **where** the conflict is, **why** the rule exists, and **how**
to proceed.

For the degree learners who still feel stuck, `--walk` turns the same explanation into a
guided STEP 1 → STEP 4 tutorial, and `--tui` lets them flip between all errors in an
interactive viewer.

## 🧪 Testing

Run the tests:

```bash
cargo test
```

Run the benchmark:

```bash
./benchmark/run.sh
```

Current benchmark (17 cases):

```text
17 / 17 diagnostic cases detected
17 / 17 explanations provided
17 / 17 concept coverage
4  / 4  machine-safe fixes detected
3  / 4  auto-fixes verified as repairing the project
```

## 🏗️ Project structure

```text
src/
├── main.rs         # CLI
├── runner.rs       # Cargo execution
├── diagnostics.rs  # rustc JSON parsing
├── context.rs      # source context
├── analyzer.rs     # diagnostic analysis
├── explain.rs      # explanations
├── fixer.rs        # safe compiler-driven fixes
├── walk.rs         # step-by-step tutorial mode
└── tui.rs          # interactive terminal browser
benchmark/          # evaluation harness + 17 error cases
demo/               # guided demo script + fixtures
evaluation/         # real-project phase 12 evaluation target + report
phases.md           # development roadmap and status
```

## 🎯 Design principle

> **Use the Rust compiler as the source of truth.**

`rxplain` does not assume that an error code always has one solution. Candidate
repairs are generated from compiler evidence (suggestions) and source structure,
ranked deterministically, and — for `--fix` — verified by running `cargo check`
in an isolated copy of the project before any real file is touched. Ownership,
borrowing, and lifetime explanations are **span-aware**: they describe the
compiler's own reported locations rather than generic advice.

## 📌 Status

**Working, tested, and demo-ready.**

Tested error codes:

```text
E0106 · E0277 · E0282 · E0308 · E0382 · E0384 · E0432 · E0433 · E0499 · E0500 ·
E0502 · E0503 · E0505 · E0506 · E0515 · E0521 · E0596 · E0597 · E0599 · E0716
```

The generic fallback surfaces the compiler's own machine-applicable suggestion for any
error code not yet covered, so even unknown errors get a concrete, factual fix.

Tested behaviors:

```text
✓ Detection of 20 error classes
✓ Span-aware explanations with concept tagging + the rule behind it
✓ Generic fallback for unknown errors (surfaces real compiler suggestion)
✓ Human-readable and JSON output
✓ Step-by-step --walk tutorial mode
✓ Interactive --tui terminal browser
✓ Candidate repair generation, deterministic ranking, and loop guarding
✓ Isolated-workspace verification before any file is modified
✓ 72 automated tests passing
✓ Benchmark: 17/17 detection, 17/17 concept coverage, 4/4 verified repairs
```

## 🖥️ CLI reference

```text
rxplain [PROJECT_DIR]                  Analyze the Rust project and explain its errors
rxplain [PROJECT_DIR] --fix            Verify and apply a candidate repair
rxplain [PROJECT_DIR] --fix --dry-run  Preview candidate repairs (no files modified)
rxplain [PROJECT_DIR] --fix --json     Candidates + patch as structured JSON
rxplain [PROJECT_DIR] --fix --verify build|test
                                       Oracle for the sandbox: cargo check (default), build, or test
rxplain [PROJECT_DIR] --json           Output the full report as JSON
rxplain [PROJECT_DIR] --walk           Step-by-step tutorial mode (ownership & borrow errors)
rxplain [PROJECT_DIR] --tui            Interactive terminal browser (j/k navigate, q quit)
rxplain [PROJECT_DIR] --quiet          Suppress the banner (plain output)
rxplain --version                      Print the version
```

`--fix` never experiments on your repository directly. Every candidate repair is
applied to a temporary isolated copy and verified with `cargo check` (or
`--verify build`/`--verify test`) first. When a candidate fails, the engine
re-reads the *new* diagnostics the oracle produced and tries a follow-up fix —
a bounded repair chain (max 4 attempts, with loop detection) that handles
multi-step repairs like adding a whole lifetime annotation. Only a passing
oracle run promotes the complete patch set to your real files. Dry runs and
failed candidates leave the original project untouched. Set `RXPLAIN_TIMEOUT`
(seconds) to bound any command; the timeout hard-kills the whole cargo/rustc
process tree, not just the parent.

`--tui` requires an interactive terminal; when stdin/stdout are not terminals it
automatically falls back to the plain report.

## 🔌 Editor / tool integration

`--json` emits a structured report designed for editors, LSP servers, CI, and other tools.
Each entry in the `errors` array has:

```json
{
  "code": "E0308",
  "message": "mismatched types",
  "locations": [ { "file": "src/main.rs", "line": 3, "column": 13,
                   "snippet": "...", "label": "..." } ],
  "relationships": [ "Line 3 is related to line 3: ..." ],
  "explanation": { "title": "Mismatched types (E0308)",
                   "summary": "...", "concept": "Types",
                   "principle": "Rust usually infers types from usage...",
                   "fix_options": [ "...", "..." ] },
  "suggestions": [ { "file": "...", "line": 1, "column": 26,
                     "replacement": "&mut ", "applicability": "MachineApplicable",
                     "label": "..." } ],
  "fix": { "kind": "RequiresHumanJudgment | CompilerSuggested",
           "description": "...", "file": null, "line": null,
           "column": null, "replacement": null, "applicability": null }
}
```

A ready-made consumer (`examples/json_consumer.sh`) projects this down to the fields a
diagnostics panel needs:

```bash
bash examples/json_consumer.sh path/to/project
```

## 🧭 Architecture

```text
src/
├── main.rs         CLI (clap) + human/JSON rendering
├── runner.rs       Invokes `cargo check --message-format=json` (with timeout)
├── diagnostics.rs  Parses rustc JSON diagnostics into ParsedError (+ suggestions)
├── context.rs      Captures the relevant source context around each span
├── analyzer.rs     Locations, relationships, compiler fixes, type extraction
├── explain.rs      Explanations; span-aware summaries + concept + fix options
├── fixer.rs        Safe, overlap-guarded, multi-file auto-fix + build verification
├── repair.rs       Repair candidates: kinds, confidence, ranking, loop guarding
├── patch.rs        Reviewable patches (preview / validate / apply)
├── verification.rs Isolated workspace copy + oracle (`cargo check`/`build`/`test`)
├── walk.rs         `--walk` tutorial mode: STEP 1 problem → STEP 2 where → STEP 3 why → STEP 4 fix
└── tui.rs          `--tui` ratatui browser: error list + scrollable explanation
```

## 📊 Benchmark methodology

`benchmark/run.sh` drives 17 isolated fixtures (one per supported error code). For each
it measures: **detection** (does rxplain surface the exact `<CODE>` error), **explanation**
(present), **concept coverage**, and **verified repair** (copies the fixture, applies
`--fix`, and confirms a subsequent build reports success). Fixtures are copies so the
originals are never mutated.

## ⚠️ Limitations

- Coverage is a fixed set of common error codes plus a generic fallback; rarer codes get
  the fallback, which only echoes the compiler message and its real suggestion.
- `--fix` verifies every candidate in an isolated workspace before applying it. The repair
  loop chains before failing: a candidate that fails `cargo check` (or the `--verify`
  mode) is replaced by follow-up candidates generated from the oracle's *new*
  diagnostics, up to a bounded 4 attempts. The complete patch set is only promoted to
  the original project if a final oracle run passes; otherwise nothing is modified.
  Errors without any expressible candidate (no compiler suggestion at any step) still
  require human judgment and are reported as such.
- A hard error in the library crate stops `cargo check` before the binary crate is
  compiled, so errors are reported per-crate in compiler order.
- Explanations are deterministic and offline; `fix_options`/`concept` are reference text,
  while the summary is stitched from the compiler's own labels and lines.
- Timeouts bound every oracle call (`RXPLAIN_TIMEOUT`, default 120s, 300s for the
  uncached isolated copy) and hard-kill the entire cargo/rustc process tree;
  an isolated copy is created for `--fix` verification (skipping `target/` and VCS
  directories).

## 📦 Packaging & release

Build an optimized release binary:

```bash
cargo build --release
```

Create a tagged GitHub release (`scripts/release.sh` builds the binary into a tarball
and uploads it via the GitHub CLI):

```bash
./scripts/release.sh v0.1.0
```

Changelog and version notes live in `CHANGELOG.md`.

## License

MIT
