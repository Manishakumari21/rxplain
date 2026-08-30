use crate::diagnostics::ParsedError;

pub struct Explanation {
    pub title: String,
    pub plain_summary: String,
    pub fix_options: Vec<String>,
}

pub fn explain(err: &ParsedError) -> Explanation {
    match err.code.as_str() {
        "E0382" => e0382(err),
        "E0502" => e0502(err),
        "E0499" => e0499(err),
        "E0597" => e0597(err),
        _ => generic(err),
    }
}

fn e0382(err: &ParsedError) -> Explanation {
    build(
        "Use of a moved value",
        format!(
            "Line {} uses a value that was already moved earlier in the code. \
             Rust transfers ownership when a value is moved, so the original \
             variable cannot be used again.",
            err.primary_line
        ),
        &[
            "Use a reference (&value) if you only need to read the value.",
            "Use .clone() before the move if you need an independent copy.",
            "If the type implements Copy, it can be copied instead of moved.",
        ],
    )
}

fn e0502(err: &ParsedError) -> Explanation {
    build(
        "Mutable borrow while immutably borrowed",
        format!(
            "Line {} tries to change a value while it is still being read \
             through an immutable borrow.",
            err.primary_line
        ),
        &[
            "Finish using the immutable borrow before creating the mutable borrow.",
            "Reduce the borrow's scope by using a smaller { } block.",
            "Use .clone() if making a copy is appropriate.",
        ],
    )
}

fn e0499(err: &ParsedError) -> Explanation {
    build(
        "Multiple mutable borrows",
        format!(
            "Line {} tries to create another mutable borrow while an earlier \
             mutable borrow is still active.",
            err.primary_line
        ),
        &[
            "Finish using the first mutable borrow before creating the second.",
            "Reduce the scope of the first borrow.",
            "Restructure the data or use methods such as split_at_mut when appropriate.",
        ],
    )
}

fn e0597(err: &ParsedError) -> Explanation {
    build(
        "Value does not live long enough",
        format!(
            "Line {} uses a reference after the value it refers to may have \
             already been dropped.",
            err.primary_line
        ),
        &[
            "Move the value into a scope where it lives long enough.",
            "Return an owned value such as String instead of a reference.",
            "Use an explicit lifetime when the design requires references.",
        ],
    )
}

fn generic(err: &ParsedError) -> Explanation {
    build(
        "No custom explanation yet",
        err.raw_message.clone(),
        &[&format!(
            "Run `rustc --explain {}` for the compiler's detailed explanation.",
            err.code
        )],
    )
}

fn build(title: &str, summary: String, fixes: &[&str]) -> Explanation {
    Explanation {
        title: title.to_string(),
        plain_summary: summary,
        fix_options: fixes.iter().map(|s| s.to_string()).collect(),
    }
}