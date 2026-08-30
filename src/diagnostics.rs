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
    pub level: String,
    pub message: String,
    pub spans: Vec<Span>,
}

#[derive(Debug, Deserialize)]
pub struct Span {
    pub is_primary: bool,
    pub file_name: String,
    pub line_start: u32,
    pub column_start: u32,
    pub text: Vec<SpanText>,

    pub suggested_replacement: Option<String>,
    pub suggestion_applicability: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SpanText {
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ParsedError {
    pub code: String,
    pub raw_message: String,
    pub file: String,
    pub primary_line: u32,
    pub primary_snippet: String,

    // Information provided by rustc for safe automatic fixes
    pub suggested_replacement: Option<String>,
    pub suggestion_applicability: Option<String>,
}

impl ParsedError {
    pub fn from_rustc_message(msg: &RustcMessage) -> Option<Self> {
        if msg.level != "error" {
            return None;
        }

        let code = msg.code.as_ref()?.code.clone();

        let primary = msg.spans.iter().find(|s| s.is_primary)?;

        // Look for a compiler-provided safe suggestion.
        let suggestion = msg
            .children
            .iter()
            .flat_map(|child| child.spans.iter())
            .find(|span| span.suggested_replacement.is_some());

        Some(ParsedError {
            code,
            raw_message: msg.message.clone(),
            file: primary.file_name.clone(),
            primary_line: primary.line_start,
            primary_snippet: primary
                .text
                .first()
                .map(|t| t.text.trim().to_string())
                .unwrap_or_default(),

            suggested_replacement: suggestion
                .and_then(|s| s.suggested_replacement.clone()),

            suggestion_applicability: suggestion
                .and_then(|s| s.suggestion_applicability.clone()),
        })
    }
}