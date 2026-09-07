use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CargoMessage {
    pub reason: String,
    pub message: Option<RustcMessage>,
}

#[derive(Debug, Deserialize)]
pub struct ErrorCode {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct RustcMessage {
    pub code: Option<ErrorCode>,
    pub level: String,
    pub message: String,
    pub spans: Vec<Span>,
    pub children: Vec<DiagnosticChild>,
}

#[derive(Debug, Deserialize)]
pub struct DiagnosticChild {
    pub message: String,
    pub spans: Vec<Span>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Span {
    pub file_name: String,
    pub line_start: u32,
    pub line_end: u32,
    pub column_start: u32,
    pub column_end: u32,
    pub label: Option<String>,
    pub text: Vec<SpanText>,

    pub suggested_replacement: Option<String>,
    pub suggestion_applicability: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SpanText {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ParsedError {
    pub code: String,
    pub raw_message: String,
    pub spans: Vec<Span>,
    pub suggestions: Vec<CompilerSuggestion>,
}

#[derive(Debug, Clone)]
pub struct CompilerSuggestion {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub column_end: u32,
    pub replacement: String,
    pub applicability: String,
    pub label: Option<String>,
}

impl ParsedError {
    pub fn from_rustc_message(msg: &RustcMessage) -> Option<Self> {
        if msg.level != "error" {
            return None;
        }

        let code = msg.code.as_ref()?.code.clone();

        let mut spans = msg.spans.clone();
        let mut suggestions = Vec::new();

        for span in &spans {
            if let Some(replacement) = &span.suggested_replacement {
                suggestions.push(CompilerSuggestion {
                    file: span.file_name.clone(),
                    line: span.line_start,
                    column: span.column_start,
                    column_end: span.column_end,
                    replacement: replacement.clone(),
                    applicability: span
                        .suggestion_applicability
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string()),
                    label: span.label.clone(),
                });
            }
        }

        for child in &msg.children {
            for span in &child.spans {
                if let Some(replacement) = &span.suggested_replacement {
                    suggestions.push(CompilerSuggestion {
                        file: span.file_name.clone(),
                        line: span.line_start,
                        column: span.column_start,
                        column_end: span.column_end,
                        replacement: replacement.clone(),
                        applicability: span
                            .suggestion_applicability
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string()),
                        label: Some(child.message.clone()),
                    });
                }
            }
        }

        spans.sort_by_key(|span| (span.line_start, span.column_start));

        Some(Self {
            code,
            raw_message: msg.message.clone(),
            spans,
            suggestions,
        })
    }
}
