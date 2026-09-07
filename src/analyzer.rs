use crate::diagnostics::{ParsedError, Span};

#[derive(Debug)]
pub struct DiagnosticAnalysis {
    pub locations: Vec<CodeLocation>,
    pub relationships: Vec<DiagnosticRelationship>,
    pub suggestions: Vec<CompilerFix>,
    pub expected_type: Option<String>,
    pub found_type: Option<String>,
}

#[derive(Debug)]
pub struct CodeLocation {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub snippet: String,
    pub label: Option<String>,
}

#[derive(Debug)]
pub struct DiagnosticRelationship {
    pub explanation: String,
}

#[derive(Debug)]
pub struct CompilerFix {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub replacement: String,
    pub applicability: String,
    pub label: Option<String>,
}

pub fn analyze(error: &ParsedError) -> DiagnosticAnalysis {
    let locations = error
        .spans
        .iter()
        .map(location_from_span)
        .collect::<Vec<_>>();

    let relationships = analyze_relationships(&error.spans);

    let suggestions = error
        .suggestions
        .iter()
        .map(|suggestion| CompilerFix {
            file: suggestion.file.clone(),
            line: suggestion.line,
            column: suggestion.column,
            replacement: suggestion.replacement.clone(),
            applicability: suggestion.applicability.clone(),
            label: suggestion.label.clone(),
        })
        .collect::<Vec<_>>();

    let (expected_type, found_type) = extract_types(&error.spans);

    DiagnosticAnalysis {
        locations,
        relationships,
        suggestions,
        expected_type,
        found_type,
    }
}

fn location_from_span(span: &Span) -> CodeLocation {
    CodeLocation {
        file: span.file_name.clone(),
        line: span.line_start,
        column: span.column_start,
        snippet: span
            .text
            .first()
            .map(|text| text.text.trim().to_string())
            .unwrap_or_default(),
        label: span.label.clone(),
    }
}

fn analyze_relationships(spans: &[Span]) -> Vec<DiagnosticRelationship> {
    let mut relationships = Vec::new();

    for (index, first) in spans.iter().enumerate() {
        for second in spans.iter().skip(index + 1) {
            if !same_file(first, second) {
                continue;
            }

            if first.line_start == second.line_start && first.column_start == second.column_start {
                continue;
            }

            let first_label = first
                .label
                .as_deref()
                .unwrap_or("compiler-reported location");

            let second_label = second
                .label
                .as_deref()
                .unwrap_or("related compiler location");

            relationships.push(DiagnosticRelationship {
                explanation: format!(
                    "Line {} is related to line {}: \"{}\" → \"{}\".",
                    first.line_start, second.line_start, first_label, second_label
                ),
            });
        }
    }

    relationships
}

fn extract_types(spans: &[Span]) -> (Option<String>, Option<String>) {
    let mut expected = None;
    let mut found = None;

    for span in spans {
        let Some(label) = span.label.as_deref() else {
            continue;
        };

        if let Some(value) = extract_type_after(label, "expected ") {
            expected = Some(value);
        }

        if let Some(value) = extract_type_after(label, "found ") {
            found = Some(value);
        }
    }

    (expected, found)
}

fn extract_type_after(text: &str, prefix: &str) -> Option<String> {
    let start = text.find(prefix)?;
    let remaining = &text[start + prefix.len()..];

    let immediate = remaining.trim_start();

    if let Some(after) = immediate.strip_prefix('`')
        && let Some(type_end) = after.find('`')
    {
        let value = after[..type_end].trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }

    let segment = remaining.split(',').next().unwrap_or(remaining).trim();

    if let Some(type_start) = segment.find('`') {
        let after = &segment[type_start + 1..];
        if let Some(type_end) = after.find('`') {
            let value = after[..type_end].trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }

    let value = segment.trim().trim_end_matches('.');

    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn same_file(first: &Span, second: &Span) -> bool {
    first.file_name == second.file_name
}
