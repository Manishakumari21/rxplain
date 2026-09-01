use rxplain::diagnostics::{ParsedError, RustcMessage};

fn parse(json: serde_json::Value) -> ParsedError {
    let diagnostic: RustcMessage = serde_json::from_value(json).unwrap();
    ParsedError::from_rustc_message(&diagnostic).expect("diagnostic should be parsed")
}

fn span(line: u32, column: u32, label: &str) -> serde_json::Value {
    serde_json::json!({
        "file_name": "src/main.rs",
        "byte_start": 0,
        "byte_end": 1,
        "line_start": line,
        "line_end": line,
        "column_start": column,
        "column_end": column + 1,
        "is_primary": false,
        "text": [{"text": "some code", "highlight_start": column, "highlight_end": column + 1}],
        "label": label,
        "suggested_replacement": null,
        "suggestion_applicability": null
    })
}

#[test]
fn parses_mismatched_types_error() {
    let diagnostic: RustcMessage = serde_json::from_value(serde_json::json!({
        "message": "mismatched types",
        "code": {
            "code": "E0308",
            "explanation": null
        },
        "level": "error",
        "spans": [
            {
                "file_name": "src/main.rs",
                "byte_start": 20,
                "byte_end": 27,
                "line_start": 2,
                "line_end": 2,
                "column_start": 17,
                "column_end": 24,
                "is_primary": true,
                "text": [
                    {
                        "text": "let number: i32 = \"hello\";",
                        "highlight_start": 17,
                        "highlight_end": 24
                    }
                ],
                "label": "expected `i32`, found `&str`",
                "suggested_replacement": null,
                "suggestion_applicability": null
            }
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }))
    .unwrap();

    let error = ParsedError::from_rustc_message(&diagnostic).expect("diagnostic should be parsed");

    assert_eq!(error.code, "E0308");
    assert_eq!(error.raw_message, "mismatched types");
    assert_eq!(error.spans.len(), 1);
}

#[test]
fn parses_moved_value_error() {
    let diagnostic: RustcMessage = serde_json::from_value(serde_json::json!({
        "message": "use of moved value: `name`",
        "code": {
            "code": "E0382",
            "explanation": null
        },
        "level": "error",
        "spans": [
            {
                "file_name": "src/main.rs",
                "byte_start": 70,
                "byte_end": 74,
                "line_start": 4,
                "line_end": 4,
                "column_start": 5,
                "column_end": 9,
                "is_primary": true,
                "text": [
                    {
                        "text": "println!(\"{}\", name);",
                        "highlight_start": 19,
                        "highlight_end": 23
                    }
                ],
                "label": "value used here after move",
                "suggested_replacement": null,
                "suggestion_applicability": null
            }
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }))
    .unwrap();

    let error = ParsedError::from_rustc_message(&diagnostic).expect("diagnostic should be parsed");

    assert_eq!(error.code, "E0382");
    assert_eq!(error.raw_message, "use of moved value: `name`");
    assert_eq!(error.spans.len(), 1);
}

#[test]
fn explains_moved_value_from_compiler_labels() {
    let error = parse(serde_json::json!({
        "message": "borrow of moved value: `value`",
        "code": {"code": "E0382", "explanation": null},
        "level": "error",
        "spans": [
            span(2, 9, "move occurs because `value` has type `String`"),
            span(3, 10, "value moved here"),
            span(4, 20, "value borrowed here after move")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Ownership"));
    assert!(explanation.plain_summary.contains("move occurs because"));
    assert!(explanation.plain_summary.contains("line 4"));
}

#[test]
fn explains_mutable_borrow_conflict_from_compiler_labels() {
    let error = parse(serde_json::json!({
        "message": "cannot borrow `value` as mutable more than once at a time",
        "code": {"code": "E0499", "explanation": null},
        "level": "error",
        "spans": [
            span(3, 19, "first mutable borrow occurs here"),
            span(4, 19, "second mutable borrow occurs here")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Borrowing"));
    assert!(explanation.plain_summary.contains("first mutable borrow"));
    assert!(explanation.plain_summary.contains("line 3"));
}

#[test]
fn generic_explanation_for_unknown_error() {
    let error = parse(serde_json::json!({
        "message": "some future error",
        "code": {"code": "E9999", "explanation": null},
        "level": "error",
        "spans": [span(1, 1, "custom label")],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept, None);
    assert!(explanation.title.contains("E9999"));
    assert!(explanation.plain_summary.contains("some future error"));
}

#[test]
fn explains_dangling_borrow_from_compiler_labels() {
    let error = parse(serde_json::json!({
        "message": "`x` does not live long enough",
        "code": {"code": "E0597", "explanation": null},
        "level": "error",
        "spans": [
            span(6, 9, "`x` dropped here while still borrowed"),
            span(7, 5, "borrow later used here")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Lifetimes"));
    assert!(
        explanation
            .plain_summary
            .contains("dropped here while still borrowed")
    );
    assert!(explanation.plain_summary.contains("line 6"));
}

#[test]
fn explains_missing_lifetime_from_compiler_labels() {
    let error = parse(serde_json::json!({
        "message": "missing lifetime specifier",
        "code": {"code": "E0106", "explanation": null},
        "level": "error",
        "spans": [span(1, 31, "expected named lifetime parameter")],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Lifetimes"));
    assert!(
        explanation
            .plain_summary
            .contains("expected named lifetime parameter")
    );
}

#[test]
fn explains_trait_bound_from_compiler_labels() {
    let error = parse(serde_json::json!({
        "message": "`Vec<{integer}>` doesn't implement `std::fmt::Display`",
        "code": {"code": "E0277", "explanation": null},
        "level": "error",
        "spans": [
            span(4, 5, "the trait `std::fmt::Display` is not implemented for `Vec<{integer}>`")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Traits"));
    assert!(explanation.plain_summary.contains("std::fmt::Display"));
}

#[test]
fn explains_mutable_borrow_from_compiler_labels() {
    let error = parse(serde_json::json!({
        "message": "cannot borrow `*v` as mutable, as it is behind a `&` reference",
        "code": {"code": "E0596", "explanation": null},
        "level": "error",
        "spans": [
            span(3, 5, "`v` is a `&` reference, so it cannot be borrowed as mutable")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Borrowing"));
    assert!(
        explanation
            .plain_summary
            .contains("cannot be borrowed as mutable")
    );
    assert!(explanation.plain_summary.contains("line 3"));
}

#[test]
fn explains_method_not_found_from_compiler_labels() {
    let error = parse(serde_json::json!({
        "message": "no method named `reverse` found for reference `&str` in the current scope",
        "code": {"code": "E0599", "explanation": null},
        "level": "error",
        "spans": [
            span(6, 5, "method not found in `&str`")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Methods & Traits"));
    assert!(
        explanation
            .plain_summary
            .contains("method not found in `&str`")
    );
}

#[test]
fn explains_type_annotation_from_compiler_labels() {
    let error = parse(serde_json::json!({
        "message": "type annotations needed for `Vec<_>`",
        "code": {"code": "E0282", "explanation": null},
        "level": "error",
        "spans": [
            span(10, 13, "type must be known at this point")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Type Inference"));
    assert!(
        explanation
            .plain_summary
            .contains("type must be known at this point")
    );
}

#[test]
fn generic_explanation_surfaces_compiler_suggestion() {
    let diagnostic: RustcMessage = serde_json::from_value(serde_json::json!({
        "message": "cannot assign to `x`, which is behind a `&` reference",
        "code": {"code": "E0594", "explanation": null},
        "level": "error",
        "spans": [
            {
                "file_name": "src/lib.rs",
                "byte_start": 0,
                "byte_end": 1,
                "line_start": 8,
                "line_end": 8,
                "column_start": 9,
                "column_end": 10,
                "is_primary": true,
                "text": [{"text": "        x.push(1);"}],
                "label": "cannot assign",
                "suggested_replacement": null,
                "suggestion_applicability": null
            }
        ],
        "children": [
            {
                "message": "consider changing this to be a mutable reference",
                "spans": [
                    {
                        "file_name": "src/lib.rs",
                        "byte_start": 0,
                        "byte_end": 1,
                        "line_start": 1,
                        "line_end": 1,
                        "column_start": 26,
                        "column_end": 29,
                        "is_primary": false,
                        "text": [{"text": "pub fn f(v: &Vec<i32>)"}],
                        "label": null,
                        "suggested_replacement": "&mut ",
                        "suggestion_applicability": "MachineApplicable"
                    }
                ]
            }
        ],
        "rendered": null,
        "suggestions": []
    }))
    .unwrap();

    let error = ParsedError::from_rustc_message(&diagnostic).expect("diagnostic should be parsed");
    assert!(!error.suggestions.is_empty(), "suggestion should be parsed");

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept, None);
    assert!(
        explanation
            .fix_options
            .iter()
            .any(|option| option.contains("&mut") && option.contains("src/lib.rs:1"))
    );
}

#[test]
fn explains_use_of_value_after_mutable_borrow() {
    let error = parse(serde_json::json!({
        "message": "cannot use `value` because it was mutably borrowed",
        "code": {"code": "E0503", "explanation": null},
        "level": "error",
        "spans": [
            span(3, 16, "borrow of `value` occurs here"),
            span(4, 25, "use of borrowed `value`")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Ownership"));
    assert!(explanation.title.contains("E0503"));
    assert!(explanation.plain_summary.contains("line 4"));
    assert!(
        explanation
            .fix_options
            .iter()
            .any(|option| option.contains("borrow"))
    );
}

#[test]
fn explains_assignment_while_borrowed() {
    let error = parse(serde_json::json!({
        "message": "cannot assign to `x` because it is borrowed",
        "code": {"code": "E0506", "explanation": null},
        "level": "error",
        "spans": [
            span(3, 13, "`x` is borrowed here"),
            span(4, 13, "assignment to borrowed `x` occurs here")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Borrowing"));
    assert!(explanation.title.contains("E0506"));
    assert!(explanation.plain_summary.contains("line 3"));
}

#[test]
fn explains_return_referencing_local() {
    let error = parse(serde_json::json!({
        "message": "cannot return reference to local variable `x`",
        "code": {"code": "E0515", "explanation": null},
        "level": "error",
        "spans": [
            span(2, 13, "returns a reference to data owned by the current function"),
            span(2, 18, "returns a value referencing data owned by the current function")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Lifetimes"));
    assert!(explanation.title.contains("E0515"));
    assert!(
        explanation
            .fix_options
            .iter()
            .any(|option| option.contains("ownership"))
    );
}

#[test]
fn explains_borrowed_data_escaping_closure() {
    let error = parse(serde_json::json!({
        "message": "borrowed data escapes outside of closure",
        "code": {"code": "E0521", "explanation": null},
        "level": "error",
        "spans": [
            span(3, 14, "closure captures `list`"),
            span(4, 18, "`el` escapes the closure body here")
        ],
        "children": [],
        "rendered": null,
        "suggestions": []
    }));

    let analysis = rxplain::analyzer::analyze(&error);
    let explanation = rxplain::explain::explain(&error, &analysis);

    assert_eq!(explanation.concept.as_deref(), Some("Lifetimes"));
    assert!(explanation.title.contains("E0521"));
    assert!(
        explanation
            .fix_options
            .iter()
            .any(|option| option.contains("lifetime"))
    );
}
