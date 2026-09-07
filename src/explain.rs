use crate::analyzer::DiagnosticAnalysis;
use crate::diagnostics::{ParsedError, Span};

#[derive(Debug)]
pub struct Explanation {
    pub title: String,
    pub plain_summary: String,
    pub fix_options: Vec<String>,
    pub concept: Option<String>,
    pub principle: Option<String>,
}

pub fn explain(error: &ParsedError, analysis: &DiagnosticAnalysis) -> Explanation {
    let mut explanation = match error.code.as_str() {
        "E0308" => explain_types(error, analysis),
        "E0382" => explain_moved_value(error),
        "E0384" => explain_immutable_assignment(error),
        "E0499" => explain_multiple_mutable_borrows(error),
        "E0500" => explain_borrow_with_existing_mut(error),
        "E0502" => explain_borrow_conflict(error),
        "E0503" => explain_moved_while_borrowed(error),
        "E0505" => explain_move_while_borrowed(error),
        "E0506" => explain_assign_while_borrowed(error),
        "E0515" => explain_return_referencing_local(error),
        "E0521" => explain_borrowed_data_escapes(error),
        "E0597" => explain_borrowed_not_long_enough(error),
        "E0106" => explain_missing_lifetime(error),
        "E0277" => explain_trait_bound(error),
        "E0596" => explain_mutable_borrow(error),
        "E0599" => explain_method_not_found(error),
        "E0282" => explain_type_annotation(error),
        "E0432" | "E0433" => explain_unresolved_path(error),
        "E0716" => explain_temporary_dropped(error),
        _ => generic_explanation(error),
    };

    if let Some(concept) = &explanation.concept {
        explanation.principle = principle_for(concept).map(str::to_string);
    }

    explanation
}

fn principle_for(concept: &str) -> Option<&'static str> {
    match concept {
        "Ownership" => Some(
            "Every value has exactly one owner at a time. Moving it to a new owner \
             leaves the old binding invalid, so it can no longer be used.",
        ),
        "Borrowing" => Some(
            "At any moment a value can have either many immutable references or one \
             mutable reference — never both, because concurrent reads and writes would race.",
        ),
        "Lifetimes" => Some(
            "A reference must never outlive the data it points to. Each borrow carries \
             a lifetime, which the compiler checks to guarantee dangling references cannot exist.",
        ),
        "Mutability" => Some(
            "Bindings are read-only by default. Declaring `mut` is you, the programmer, \
             stating that you intend to write to the value.",
        ),
        "Traits" => Some(
            "Traits are Rust's contracts: a type only gains the behavior described by a \
             trait when it implements that trait (or the trait is otherwise in scope).",
        ),
        "Methods & Traits" => Some(
            "A method call only works if the type provides that method — either defined on \
             the type itself or via a trait that is in scope.",
        ),
        "Type Inference" => Some(
            "Rust usually infers types from usage. When the code alone doesn't pin the type \
             down, the compiler asks you to state it explicitly.",
        ),
        "Paths & Modules" => Some(
            "A name is usable only if it can be resolved: the item must exist and be \
             reachable from the path you wrote.",
        ),
        _ => None,
    }
}

fn generic_explanation(error: &ParsedError) -> Explanation {
    let mut fix_options: Vec<String> = error
        .suggestions
        .iter()
        .map(|suggestion| {
            let where_to = format!("{}:{}", suggestion.file, suggestion.line);
            let what = if suggestion.replacement.trim().is_empty() {
                "remove the flagged text".to_string()
            } else {
                format!("replace it with `{}`", suggestion.replacement.trim())
            };
            if let Some(label) = &suggestion.label {
                format!("{}: {} — {}", where_to, label, what)
            } else {
                format!("{}: {}", where_to, what)
            }
        })
        .collect();

    if !fix_options.is_empty() {
        fix_options.push(
            "This suggestion comes directly from the compiler; apply it, then re-run to verify."
                .to_string(),
        );
    } else {
        fix_options.push("Inspect the compiler evidence and source context above.".to_string());
        fix_options
            .push("Check the related source locations mentioned by the compiler.".to_string());
    }

    Explanation {
        title: format!("Rust compiler error {}", error.code),
        plain_summary: format!("Rust reported: {}", error.raw_message),
        fix_options,
        concept: None,
        principle: None,
    }
}

fn labels_by_line(spans: &[Span]) -> Vec<(u32, String)> {
    let mut labels: Vec<(u32, String)> = spans
        .iter()
        .filter_map(|span| {
            span.label
                .as_ref()
                .filter(|label| !label.is_empty())
                .map(|label| (span.line_start, label.clone()))
        })
        .collect();

    labels.sort_by_key(|(line, _)| *line);

    labels
}

fn explain_moved_value(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let (move_line, move_label) = labels
        .iter()
        .find(|(_, label)| {
            label.contains("moved here")
                || label.contains("move occurs")
                || label.contains("value moved")
        })
        .map(|(line, label)| (*line, label.clone()))
        .unwrap_or_else(|| {
            labels
                .first()
                .map(|(line, label)| (*line, label.clone()))
                .unwrap_or((0, error.raw_message.clone()))
        });

    let use_line = labels
        .iter()
        .find(|(line, label)| {
            *line != move_line
                && (label.contains("used here after move")
                    || label.contains("borrowed here after move"))
        })
        .map(|(line, _)| *line);

    let summary = match use_line {
        Some(use_line) => format!(
            "{}. The value is moved {} (line {}), then used again at line {}.",
            error.raw_message, move_label, move_line, use_line
        ),
        None if labels.len() >= 2 => format!(
            "{}. {} (line {}), then used at line {}.",
            error.raw_message,
            move_label,
            move_line,
            labels[labels.len() - 1].0
        ),
        None => format!("{}. {}", error.raw_message, move_label),
    };

    Explanation {
        title: "Use of a moved value (E0382)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Borrow the value instead of moving it where possible.".to_string(),
            "Clone the value if an owned copy is acceptable.".to_string(),
            "Reorder the code so the value is used before it is moved.".to_string(),
        ],
        concept: Some("Ownership".to_string()),
        principle: None,
    }
}

fn explain_immutable_assignment(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if labels.len() >= 2 {
        format!(
            "{}. {} (line {}), then {} (line {}).",
            error.raw_message,
            labels[0].1,
            labels[0].0,
            labels[labels.len() - 1].1,
            labels[labels.len() - 1].0
        )
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Assignment to immutable variable (E0384)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Declare the variable with `mut` if it must change.".to_string(),
            "Use a new `let` binding instead of reassigning.".to_string(),
        ],
        concept: Some("Mutability".to_string()),
        principle: None,
    }
}

fn explain_multiple_mutable_borrows(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if labels.len() >= 2 {
        format!(
            "{}. {} (line {}), {} (line {}).",
            error.raw_message, labels[0].1, labels[0].0, labels[1].1, labels[1].0
        )
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Multiple mutable borrows (E0499)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Reduce the number of simultaneous mutable borrows.".to_string(),
            "Scope each borrow so it ends before the next begins.".to_string(),
            "Use a single `&mut` if the borrows can be merged.".to_string(),
        ],
        concept: Some("Borrowing".to_string()),
        principle: None,
    }
}

fn explain_borrow_conflict(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if labels.len() >= 2 {
        format!(
            "{}. {} (line {}), {} (line {}).",
            error.raw_message, labels[0].1, labels[0].0, labels[1].1, labels[1].0
        )
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Conflicting borrows (E0502)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Ensure the immutable borrow ends before the mutable borrow begins.".to_string(),
            "Narrow the scope of the immutable borrow.".to_string(),
            "Clone the value if an owned copy is acceptable.".to_string(),
        ],
        concept: Some("Borrowing".to_string()),
        principle: None,
    }
}

fn explain_borrow_with_existing_mut(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if labels.len() >= 2 {
        format!(
            "{}. {} (line {}), then {} (line {}).",
            error.raw_message, labels[0].1, labels[0].0, labels[1].1, labels[1].0
        )
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Cannot borrow as mutable while an immutable borrow is active (E0500)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Scope the immutable borrow so it ends before the mutable borrow begins.".to_string(),
            "Use a non-mutating method instead of taking `&mut`.".to_string(),
            "If sharing is intended, avoid mutating while references exist.".to_string(),
        ],
        concept: Some("Borrowing".to_string()),
        principle: None,
    }
}

fn explain_moved_while_borrowed(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if labels.len() >= 2 {
        format!(
            "{}. {} (line {}), and the borrow is used at line {}.",
            error.raw_message, labels[0].1, labels[0].0, labels[1].0
        )
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Cannot use a value that has been moved while it is borrowed (E0503)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Keep the borrow alive until after the value's last use.".to_string(),
            "Clone the value if it needs to be both borrowed and moved.".to_string(),
            "Reorder so the move happens after the borrow is finished.".to_string(),
        ],
        concept: Some("Ownership".to_string()),
        principle: None,
    }
}

fn explain_assign_while_borrowed(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if labels.len() >= 2 {
        format!(
            "{}. {} (line {}), then {} (line {}).",
            error.raw_message, labels[0].1, labels[0].0, labels[1].1, labels[1].0
        )
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Cannot assign to a variable while it is borrowed (E0506)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Ensure the borrow ends before reassigning the variable.".to_string(),
            "Reduce the borrow's scope so it does not overlap the assignment.".to_string(),
            "Mutate via the reference if it is a `&mut` borrow.".to_string(),
        ],
        concept: Some("Borrowing".to_string()),
        principle: None,
    }
}

fn explain_return_referencing_local(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if labels.len() >= 2 {
        format!(
            "{}. The value is created at line {}, but returned as a reference at line {}.",
            error.raw_message,
            labels[0].0,
            labels[labels.len() - 1].0
        )
    } else {
        format!(
            "{}. The function returns a reference to a value that is local to it.",
            error.raw_message
        )
    };

    Explanation {
        title: "Cannot return a reference to a local variable (E0515)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Return the value by ownership instead of by reference.".to_string(),
            "Have the caller pass data in so it outlives the function.".to_string(),
            "Use a smart pointer like `Box` or `Rc` to extend the value's lifetime.".to_string(),
        ],
        concept: Some("Lifetimes".to_string()),
        principle: None,
    }
}

fn explain_borrowed_data_escapes(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if labels.len() >= 2 {
        format!(
            "{}. The reference is captured at line {}, but it would escape the function at line {}.",
            error.raw_message,
            labels[0].0,
            labels[labels.len() - 1].0
        )
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Borrowed data escapes outside of its function (E0521)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Add a lifetime parameter so the borrowed data is tied to the caller.".to_string(),
            "Return the owned value instead of a reference into local data.".to_string(),
            "Use a reference that is valid for at least the function's return lifetime."
                .to_string(),
        ],
        concept: Some("Lifetimes".to_string()),
        principle: None,
    }
}

fn explain_move_while_borrowed(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if labels.len() >= 2 {
        let move_line = labels
            .iter()
            .find(|(_, label)| label.starts_with("move out of"))
            .map(|(line, _)| *line)
            .unwrap_or(labels[0].0);

        let held = labels
            .iter()
            .find(|(_, label)| label.contains("later used here"))
            .map(|(line, label)| (line.to_string(), label.clone()));

        if let Some((_, used_label)) = held {
            format!(
                "{}. The value is moved at line {}, while {}.",
                error.raw_message, move_line, used_label
            )
        } else {
            format!("{}. Move occurs at line {}.", error.raw_message, move_line)
        }
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Cannot move with an active borrow (E0505)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "End the borrow before moving the value.".to_string(),
            "Clone the value if it must be moved while borrowed.".to_string(),
        ],
        concept: Some("Ownership".to_string()),
        principle: None,
    }
}

fn explain_borrowed_not_long_enough(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if labels.len() >= 2 {
        let dropped = labels
            .iter()
            .find(|(_, label)| label.contains("dropped here"))
            .map(|(line, label)| (line.to_string(), label.clone()));

        let used = labels
            .iter()
            .find(|(_, label)| label.contains("later used here"))
            .map(|(line, label)| (line.to_string(), label.clone()));

        match (dropped, used) {
            (Some((dropped_line, dropped_label)), Some((_used_line, used_label))) => format!(
                "{}. {} at line {}, then {}.",
                error.raw_message, dropped_label, dropped_line, used_label
            ),
            _ => error.raw_message.clone(),
        }
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Borrowed value does not live long enough (E0597)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Ensure the borrowed value lives at least as long as its borrow.".to_string(),
            "Extend the scope of the value or restructure so the reference is valid.".to_string(),
        ],
        concept: Some("Lifetimes".to_string()),
        principle: None,
    }
}

fn explain_missing_lifetime(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if let Some((_, label)) = labels.first() {
        format!("{}. {}", error.raw_message, label)
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Missing lifetime specifier (E0106)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Add a lifetime parameter, for example `<'a>`.".to_string(),
            "Use an explicit lifetime on the reference that is returned.".to_string(),
        ],
        concept: Some("Lifetimes".to_string()),
        principle: None,
    }
}

fn explain_trait_bound(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if let Some((_, label)) = labels.first() {
        format!("{}. {}", error.raw_message, label)
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: format!("Trait bound not satisfied ({})", error.code),
        plain_summary: summary,
        fix_options: vec![
            "Implement the required trait for this type.".to_string(),
            "Use a type that already satisfies the trait bound.".to_string(),
        ],
        concept: Some("Traits".to_string()),
        principle: None,
    }
}

fn explain_mutable_borrow(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if let Some((line, label)) = labels.first() {
        format!("{}. {} (line {}).", error.raw_message, label, line)
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Cannot borrow as mutable (E0596)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Make the reference itself mutable, for example `&mut ...`.".to_string(),
            "Or declare the value binding as `mut` so it can be borrowed mutably.".to_string(),
            "If the mutation is not required, change the receiver/parameter type.".to_string(),
        ],
        concept: Some("Borrowing".to_string()),
        principle: None,
    }
}

fn explain_method_not_found(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if let Some((line, label)) = labels.first() {
        format!("{}. {} (line {}).", error.raw_message, label, line)
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Method not found (E0599)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Check that the method name is spelled correctly.".to_string(),
            "Confirm the type actually provides this method (trait must be imported).".to_string(),
            "If the method belongs to a trait, bring that trait into scope with `use`.".to_string(),
        ],
        concept: Some("Methods & Traits".to_string()),
        principle: None,
    }
}

fn explain_type_annotation(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if let Some((line, label)) = labels.first() {
        format!("{}. {} (line {}).", error.raw_message, label, line)
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Type annotations needed (E0282)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Add an explicit type annotation where the value is declared.".to_string(),
            "For collections, specify the element type, for example `Vec<String>`.".to_string(),
            "If the type is fixed by an assignment, use a typed binding or `let ... = ...: T`."
                .to_string(),
        ],
        concept: Some("Type Inference".to_string()),
        principle: None,
    }
}

fn explain_unresolved_path(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if let Some((line, label)) = labels.first() {
        format!("{}. {} (line {}).", error.raw_message, label, line)
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Unresolved name or path (E0432/E0433)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Check the spelling of the item `use`d or referenced.".to_string(),
            "Confirm the module/item actually exists at the referenced path.".to_string(),
            "Ensure any required dependency is declared under `[dependencies]`.".to_string(),
        ],
        concept: Some("Paths & Modules".to_string()),
        principle: None,
    }
}

fn explain_temporary_dropped(error: &ParsedError) -> Explanation {
    let labels = labels_by_line(&error.spans);

    let summary = if let Some((line, label)) = labels.first() {
        format!("{}. {} (line {}).", error.raw_message, label, line)
    } else {
        error.raw_message.clone()
    };

    Explanation {
        title: "Temporary value dropped while borrowed (E0716)".to_string(),
        plain_summary: summary,
        fix_options: vec![
            "Bind the temporary to a variable that outlives the borrow.".to_string(),
            "Extend the temporary's lifetime so the reference stays valid.".to_string(),
            "Restructure so the reference does not outlive the temporary.".to_string(),
        ],
        concept: Some("Lifetimes".to_string()),
        principle: None,
    }
}

fn explain_types(error: &ParsedError, analysis: &DiagnosticAnalysis) -> Explanation {
    let expected = analysis.expected_type.as_deref();
    let found = analysis.found_type.as_deref();

    match (expected, found) {
        (Some(expected), Some(found)) => {
            let (summary, fixes) = classify_type_mismatch(expected, found);

            Explanation {
                title: "Mismatched types (E0308)".to_string(),
                plain_summary: summary,
                fix_options: fixes,
                concept: Some("Types".to_string()),
                principle: None,
            }
        }

        _ => Explanation {
            title: "Mismatched types (E0308)".to_string(),
            plain_summary: format!(
                "Rust found incompatible types. The compiler reported: {}",
                error.raw_message
            ),
            fix_options: vec![
                "Check the type expected by this expression.".to_string(),
                "Check the type of the value being provided.".to_string(),
            ],
            concept: Some("Types".to_string()),
            principle: None,
        },
    }
}

fn classify_type_mismatch(expected: &str, found: &str) -> (String, Vec<String>) {
    match (expected, found) {
        ("i32", "&str") => (
            "Rust expected an integer of type `i32`, but the code \
             provided a string slice of type `&str`. The value cannot \
             be used as an integer."
                .to_string(),
            vec![
                "Replace the string value with an integer, for example `42`.".to_string(),
                "If the value should be text, change the variable type to `String` or `&str`."
                    .to_string(),
            ],
        ),

        ("i64", "&str") => (
            "Rust expected an integer of type `i64`, but the code \
             provided a string slice of type `&str`."
                .to_string(),
            vec![
                "Replace the string with an `i64` value.".to_string(),
                "If the value comes from text input, parse it into `i64`.".to_string(),
            ],
        ),

        ("f32", "&str") | ("f64", "&str") => (
            format!(
                "Rust expected a floating-point value of type `{expected}`, \
                 but the code provided a string slice of type `&str`."
            ),
            vec![
                format!("Replace the string with a `{expected}` value."),
                format!("If the value comes from text, parse the string into `{expected}`."),
            ],
        ),

        ("String", "&str") => (
            "Rust expected an owned `String`, but the code provided a \
             string slice `&str`. A string literal such as `\"hello\"` \
             normally has type `&str`."
                .to_string(),
            vec![
                "Convert the string slice using `.to_string()`.".to_string(),
                "Convert it using `String::from(...)`.".to_string(),
            ],
        ),

        ("&str", "String") => (
            "Rust expected a borrowed string slice `&str`, but the code \
             provided an owned `String`. The String needs to be borrowed \
             when an `&str` is required."
                .to_string(),
            vec![
                "Borrow the String using `&value`.".to_string(),
                "Use `.as_str()` when an `&str` is required.".to_string(),
            ],
        ),

        ("i32", "f32") | ("i32", "f64") => (
            format!(
                "Rust expected an integer of type `i32`, but the code \
                 provided a floating-point value of type `{found}`. \
                 Rust does not automatically convert between these \
                 numeric types."
            ),
            vec![
                "Use an integer value if `i32` is intended.".to_string(),
                "Convert the value explicitly if a floating-point value is intended.".to_string(),
            ],
        ),

        ("f32", "i32") | ("f64", "i32") => (
            format!(
                "Rust expected a floating-point value of type `{expected}`, \
                 but the code provided an integer of type `i32`."
            ),
            vec![
                format!("Convert the integer explicitly to `{expected}`."),
                "Change the expected type to `i32` if an integer is intended.".to_string(),
            ],
        ),

        ("bool", "i32") | ("bool", "i64") => (
            format!(
                "Rust expected a boolean value, but the code provided \
                 an integer of type `{found}`. Rust does not automatically \
                 treat integers as booleans."
            ),
            vec![
                "Use `true` or `false` when a boolean is required.".to_string(),
                "If the integer represents a condition, compare it explicitly.".to_string(),
            ],
        ),

        _ => (
            format!(
                "Rust expected a value of type `{expected}`, but the \
                 code provided a value of type `{found}`. These types \
                 are incompatible in this context."
            ),
            vec![
                format!("Change the provided value so that it has type `{expected}`."),
                format!("If `{found}` is the intended type, change the expected type."),
            ],
        ),
    }
}
