# rxplain

**A deterministic Rust compiler diagnostic analyzer that explains errors using the compiler's own structured diagnostics and provides safe fixes when the compiler can prove them.**

## Why this exists

Rust's compiler is extremely powerful, but compiler diagnostics can still be difficult to understand, especially for developers learning ownership, borrowing, lifetimes, and type systems.

Rust already provides detailed diagnostic information through:

```text
cargo check --message-format=json
```

This includes:

* error codes
* error messages
* source files
* line and column information
* primary and secondary spans
* labels
* compiler suggestions
* suggested replacements
* suggestion applicability
* compiler explanations

`rxplain` uses this information to build a developer-friendly layer on top of the Rust compiler.

The goal is **not to replace the compiler** and not to maintain a huge database of hardcoded fixes.

Instead, `rxplain` reads what the compiler already knows about the user's actual code and presents it in a simpler format.

---

## The problem with hardcoded fixes

A simple implementation could do this:

```text
if error == E0382:
    suggest .clone()
```

This is not reliable enough for a real developer tool.

The same compiler error can occur in many different situations, and the correct solution depends on the actual code and the developer's intention.

For example, a moved value might be fixed by:

* borrowing the value
* cloning the value
* changing ownership
* changing a function signature
* restructuring the code
* using `Copy`
* changing the lifetime or scope

Therefore, `rxplain` does **not** assume that every occurrence of an error has the same solution.

---

# Core design principle

`rxplain` follows this rule:

> **Use the Rust compiler as the source of truth.**

The compiler produces structured diagnostic information.

`rxplain` consumes that information and turns it into a simpler developer experience.

```text
                 Rust Project
                      │
                      ▼
              Cargo / rustc
                      │
                      ▼
       Structured JSON diagnostics
                      │
                      ▼
                diagnostics.rs
                      │
                      ▼
              Parsed diagnostics
                      │
          ┌───────────┴───────────┐
          ▼                       ▼
   Explanation engine        Fix analyzer
          │                       │
          ▼                       ▼
   Human-readable          Compiler-provided
      explanation               suggestion
          │                       │
          └───────────┬───────────┘
                      ▼
                  CLI output
```

---

# Project structure

```text
rxplain/
│
├── Cargo.toml
├── Cargo.lock
├── README.md
│
├── src/
│   ├── main.rs
│   ├── runner.rs
│   ├── diagnostics.rs
│   ├── explain.rs
│   └── fixer.rs
│
└── examples/
    └── broken_project/
        ├── Cargo.toml
        └── src/
            └── main.rs
```

---

# Module responsibilities

## `src/main.rs`

CLI entry point.

Responsible for:

* reading command-line arguments
* selecting the target project
* running the diagnostic pipeline
* displaying results
* handling `--fix`

It should **not** contain compiler-analysis logic.

---

## `src/runner.rs`

Responsible for executing Cargo.

Example:

```text
cargo check --message-format=json
```

or:

```text
cargo build --message-format=json
```

It captures the JSON output and passes compiler messages to the diagnostic parser.

The runner does not decide what an error means.

---

## `src/diagnostics.rs`

Responsible for converting Rust's JSON diagnostic format into internal Rust structures.

It extracts information such as:

```text
error code
message
file
line
column
source snippet
primary span
secondary spans
labels
suggested replacement
suggestion applicability
child diagnostics
```

The important point is that `ParsedError` represents the **actual compiler diagnostic**, rather than a predefined error situation.

---

## `src/explain.rs`

Responsible for presenting compiler diagnostics in simple language.

The explanation engine should use information from the actual diagnostic.

For example:

```text
Error: E0382

File: src/main.rs
Line: 6

The value `name` was moved earlier and is being used again here.

The compiler suggests:
    name.clone()
```

The system may have small explanations for common Rust concepts, but these should support the compiler diagnostic rather than replace it.

---

## `src/fixer.rs`

Responsible for determining whether a compiler suggestion can safely be applied.

The fixer should primarily use:

```text
suggested_replacement
```

and:

```text
suggestion_applicability
```

provided by rustc.

For example:

```text
MachineApplicable
```

means the compiler considers the suggested change mechanically applicable.

Other applicability levels should not automatically modify the user's code.

The fixer should never blindly assume:

```text
E0382 = clone()
```

Instead:

```text
E0382
   ↓
Does rustc provide a suggestion?
   ↓
Is it MachineApplicable?
   ↓
Apply compiler suggestion
```

If no safe suggestion exists:

```text
Do not modify the source.
Explain the problem instead.
```

---

# How rxplain works

Suppose a user has:

```rust
fn main() {
    let name = String::from("manisha");

    let other = name;

    println!("{}", name);
}
```

The user runs:

```bash
rxplain .
```

`rxplain` executes:

```bash
cargo check --message-format=json
```

Rust produces a diagnostic containing information similar to:

```text
error: E0382

message:
borrow of moved value: `name`

primary span:
src/main.rs:6

compiler suggestion:
name.clone()

applicability:
MachineApplicable
```

`rxplain` converts this into:

```text
E0382 — Use of a moved value

Location:
src/main.rs:6

What happened:
`name` was moved to `other`, so the original value
cannot be used again.

Compiler suggestion:
name.clone()

This suggestion is safe to apply automatically.
```

If the compiler provides no safe suggestion, `rxplain` does not invent one.

---

# Dynamic error handling

`rxplain` should support errors dynamically rather than maintaining a hardcoded solution for every possible Rust error.

For example:

```text
E0382
E0502
E0499
E0597
E0308
E0505
E0596
E0106
...
```

The compiler itself remains the primary source of information.

This means that an unsupported error code can still produce useful output.

Example:

```text
Unknown compiler error: E0277

Message:
the trait bound `X: Y` is not satisfied

Location:
src/main.rs:15

Compiler diagnostic:
...

Compiler suggestion:
...
```

`rxplain` does not need a custom hardcoded implementation for every Rust error before it can be useful.

---

# Explanation strategy

The explanation system has two layers.

## Layer 1 — Compiler diagnostic

Always use the actual diagnostic information:

```text
error code
message
location
spans
labels
suggestions
```

This makes the output specific to the user's code.

## Layer 2 — Concept explanation

For common errors, `rxplain` can add a short educational explanation.

For example:

```text
E0382

Rust ownership:
A value that does not implement Copy is moved when ownership
is transferred to another variable or function.

In your code:
`name` was moved here:
    let other = name;

Then it was used here:
    println!("{}", name);
```

This is much better than simply returning:

```text
E0382 = use clone()
```

---

# Auto-fix policy

`rxplain` follows a conservative auto-fix policy.

### Safe

A fix may be automatically applied when:

```text
rustc provides a suggested replacement
AND
suggestion applicability == MachineApplicable
```

### Not automatically fixed

If the compiler says the suggestion is:

```text
MaybeIncorrect
```

or:

```text
HasPlaceholders
```

or no suggestion exists,

`rxplain` should not automatically modify the source.

Instead it shows the suggestion to the developer.

---

# Why this approach is safer

Consider:

```text
E0502
```

There may be several valid solutions:

```text
1. Reorder the code
2. Reduce the borrow scope
3. Create a separate block
4. Clone the value
5. Change the data structure
```

There is no universal solution.

Therefore `rxplain` should say:

```text
The compiler detected a mutable/immutable borrow conflict.

Possible strategies:
- reduce the lifetime of the immutable borrow
- reorder the operations
- use an owned value where appropriate
```

but should **not** blindly edit the user's code.

---

# Error coverage

`rxplain` does not need to hardcode every Rust compiler error.

Errors are divided into two categories.

| Category                                  | Behavior                            |
| ----------------------------------------- | ----------------------------------- |
| Compiler diagnostic available             | Always display compiler information |
| Common error with educational explanation | Add a simple explanation            |
| Compiler provides safe suggestion         | Offer/apply suggestion              |
| No safe suggestion                        | Explain without modifying code      |
| Unknown error code                        | Display generic compiler diagnostic |

This allows the tool to remain useful even when Rust introduces new diagnostics.

---

# Example usage

Run against the current directory:

```bash
cargo run -- .
```

Run against another Rust project:

```bash
cargo run -- /path/to/project
```

Run the example project:

```bash
cargo run -- examples/broken_project
```

Request automatic fixes:

```bash
cargo run -- examples/broken_project --fix
```

---

# Example output

```text
Building project...

1 error found

[1/1] E0382
Use of a moved value

Location:
src/main.rs:6

Code:
println!("{}", name);

What happened:
The value `name` was moved earlier and is being used again.

Moved at:
src/main.rs:4

Compiler suggestion:
name.clone()

Suggestion status:
MachineApplicable

Auto-fix:
Available

------------------------------------------------------------
```

For an error without a safe compiler suggestion:

```text
E0502
Cannot borrow as mutable because it is also borrowed as immutable

Location:
src/main.rs:12

What happened:
The compiler detected an active immutable borrow when
the code attempted to create a mutable borrow.

Possible approaches:
- shorten the immutable borrow
- reorder the operations
- restructure the code
- clone the value when appropriate

Auto-fix:
Not available because the correct solution depends
on the program's intended behavior.
```

---

# Development phases

## Phase 1 — Project setup

Completed.

Tasks:

* initialize Cargo project
* add dependencies
* create module structure
* create example Rust project

---

## Phase 2 — Compiler execution

Completed.

`runner.rs` executes Cargo and captures structured compiler output.

---

## Phase 3 — Diagnostic parsing

Completed.

`diagnostics.rs` converts rustc JSON diagnostics into internal structures.

The parser extracts:

* error code
* message
* file
* line
* snippets
* spans
* compiler suggestions
* suggestion applicability

---

## Phase 4 — Basic explanation engine

Completed.

`explain.rs` provides:

* compiler error information
* plain-English explanations
* relevant fix strategies
* actual source locations

The explanation system should not assume that one error code always has one solution.

---

## Phase 5 — CLI integration

Completed.

`main.rs` connects:

```text
runner
   ↓
diagnostics
   ↓
explain
   ↓
fixer
```

and displays the result in the terminal.

---

# Phase 6 — Compiler-driven fix system

This is the next important development phase.

Instead of hardcoding:

```rust
E0382 → insert .clone()
```

implement:

```text
ParsedError
    ↓
Check compiler suggestion
    ↓
Check suggestion applicability
    ↓
Determine whether it is safe
    ↓
Apply exact compiler replacement
```

The fixer should work from the diagnostic's actual:

```text
file_name
line_start
column_start
suggested_replacement
suggestion_applicability
```

This allows the fixer to work across many different code examples.

---

# Phase 7 — Better contextual explanations

Improve explanations using:

* primary span
* secondary spans
* labels
* surrounding source code
* compiler child diagnostics
* compiler suggestions

The goal is to explain:

```text
WHAT happened
WHERE it happened
WHY Rust rejected it
WHAT the compiler suggests
WHETHER it can be safely automated
```

---

# Phase 8 — Testing

Create multiple real Rust fixtures.

Examples:

```text
tests/
├── fixtures/
│   ├── e0382/
│   ├── e0502/
│   ├── e0499/
│   ├── e0597/
│   └── e0308/
│
└── integration_tests.rs
```

Tests should verify that the system works across different code patterns rather than checking only one hardcoded example.

---

# Phase 9 — Documentation and evaluation

Evaluate `rxplain` against:

* raw `rustc --explain`
* normal Cargo diagnostics
* `cargo fix`
* manually written explanations

Measure:

* diagnostic parsing accuracy
* explanation usefulness
* safe-fix accuracy
* number of unsupported cases
* false auto-fix rate

A particularly important metric is:

> **Number of incorrect automatic modifications.**

The target should be **zero unsafe automatic fixes**.

---

# Phase 10 — Interactive UI

Optional final-year stretch goal.

Possible interface:

```text
┌─────────────────────────────────────────────┐
│ rxplain                                     │
├─────────────────────────────────────────────┤
│ E0382 — Use of moved value                  │
│                                             │
│ src/main.rs:6                               │
│                                             │
│ 4 │ let other = name;                       │
│ 6 │ println!("{}", name);                   │
│                     ^^^^^                   │
│                                             │
│ What happened                               │
│ `name` was moved at line 4.                 │
│                                             │
│ Suggested fix                               │
│ name.clone()                                │
│                                             │
│ [ Apply Fix ]       [ Ignore ]              │
└─────────────────────────────────────────────┘
```

---

# Technology

| Technology              | Purpose                         |
| ----------------------- | ------------------------------- |
| Rust                    | Application implementation      |
| Cargo                   | Build and project management    |
| rustc                   | Source of compiler diagnostics  |
| `--message-format=json` | Structured diagnostic interface |
| serde                   | Deserialize compiler JSON       |
| serde_json              | JSON parsing                    |
| clap                    | CLI arguments                   |
| colored                 | Terminal presentation           |
| anyhow                  | Error handling                  |

No external AI API is required.

No API key is required.

No database is required.

No internet connection is required for the core functionality.

---

# Design goals

`rxplain` is designed around five principles:

### 1. Compiler-first

Rust's compiler is the source of truth.

### 2. Context-aware

Explanations should refer to the user's actual file, line, span, and diagnostic information.

### 3. Conservative

Never automatically change code when the correct solution depends on developer intent.

### 4. Deterministic

The same compiler diagnostic should produce the same result.

### 5. Extensible

New Rust compiler errors should remain usable even before a custom educational explanation is implemented.

---

# Final-year project value

The project is not simply an error-message wrapper.

The main engineering problem is building a reliable diagnostic pipeline:

```text
Compiler
   ↓
Structured diagnostic extraction
   ↓
Diagnostic normalization
   ↓
Context analysis
   ↓
Human-readable explanation
   ↓
Safe suggestion evaluation
   ↓
Optional source transformation
```

This makes the project suitable for further research and development in:

* compiler tooling
* developer experience
* static analysis
* program repair
* automated code transformation
* programming-language education

---

# Current status

```text
Phase 1  ████████████████████ Complete
Phase 2  ████████████████████ Complete
Phase 3  ████████████████████ Complete
Phase 4  ████████████████████ Complete
Phase 5  ████████████████████ Complete
Phase 6  ███████████░░░░░░░░░ In progress
Phase 7  ░░░░░░░░░░░░░░░░░░░░ Planned
Phase 8  ░░░░░░░░░░░░░░░░░░░░ Planned
Phase 9  ░░░░░░░░░░░░░░░░░░░░ Planned
Phase 10 ░░░░░░░░░░░░░░░░░░░░ Optional
```

---

# License

To be decided.
