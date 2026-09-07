use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CargoMessage {
    pub reason: String,
    pub message: Option<RustcMessage>,
}

#[derive(Debug, Deserialize)]
pub struct ErrorCode {
    pub code: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub explanation: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RustcMessage {
    pub code: Option<ErrorCode>,
    #[allow(dead_code)]
    pub level: String,
    #[allow(dead_code)]
    pub message: String,
    #[allow(dead_code)]
    pub spans: Vec<Span>,
    pub children: Vec<DiagnosticChild>,
    #[serde(default)]
    pub rendered: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DiagnosticChild {
    pub message: String,
    #[serde(default)]
    pub level: String,
    pub spans: Vec<Span>,
    #[serde(default)]
    pub children: Vec<DiagnosticChild>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Span {
    pub file_name: String,
    pub line_start: u32,
    pub line_end: u32,
    pub column_start: u32,
    pub column_end: u32,
    #[serde(default)]
    pub is_primary: bool,
    pub label: Option<String>,
    pub text: Vec<SpanText>,

    #[serde(default)]
    pub suggested_replacement: Option<String>,
    #[serde(default)]
    pub suggestion_applicability: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SpanText {
    pub text: String,
}

/// Normalized rustc suggestion applicability, so transformations and JSON
/// consumers do not have to string-match compiler internals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    MachineApplicable,
    HasPlaceholders,
    MaybeIncorrect,
    Unspecified,
    Unknown,
}

#[allow(dead_code)]
impl Applicability {
    pub fn from_rustc(value: &str) -> Self {
        match value {
            "MachineApplicable" => Applicability::MachineApplicable,
            "HasPlaceholders" => Applicability::HasPlaceholders,
            "MaybeIncorrect" => Applicability::MaybeIncorrect,
            "Unspecified" => Applicability::Unspecified,
            _ => Applicability::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Applicability::MachineApplicable => "MachineApplicable",
            Applicability::HasPlaceholders => "HasPlaceholders",
            Applicability::MaybeIncorrect => "MaybeIncorrect",
            Applicability::Unspecified => "Unspecified",
            Applicability::Unknown => "Unknown",
        }
    }

    pub fn is_machine_applicable(self) -> bool {
        self == Applicability::MachineApplicable
    }
}

#[derive(Debug, Clone, Default)]
pub struct ParsedError {
    pub code: String,
    #[allow(dead_code)]
    pub level: String,
    pub raw_message: String,
    pub spans: Vec<Span>,
    pub suggestions: Vec<CompilerSuggestion>,
    #[allow(dead_code)]
    pub children: Vec<CompilerChild>,
    #[allow(dead_code)]
    pub help: Option<String>,
    #[allow(dead_code)]
    pub notes: Vec<String>,
    #[allow(dead_code)]
    pub rendered: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CompilerChild {
    pub message: String,
    pub level: String,
    pub spans: Vec<Span>,
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

impl CompilerSuggestion {
    pub fn applicability_enum(&self) -> Applicability {
        Applicability::from_rustc(&self.applicability)
    }
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
            span_to_suggestion(span, None, &mut suggestions);
        }

        let mut children = Vec::new();
        let mut notes = Vec::new();
        let mut help = None;

        for child in &msg.children {
            for span in &child.spans {
                span_to_suggestion(span, Some(&child.message), &mut suggestions);
            }

            for grandchild in &child.children {
                for span in &grandchild.spans {
                    span_to_suggestion(span, Some(&grandchild.message), &mut suggestions);
                }
            }

            match child.level.as_str() {
                "note" => {
                    if !child.message.is_empty() && !notes.contains(&child.message) {
                        notes.push(child.message.clone());
                    }
                }
                "help" if help.is_none() => {
                    help = Some(child.message.clone());
                }
                _ => {}
            }

            if !child.message.is_empty() {
                children.push(CompilerChild {
                    message: child.message.clone(),
                    level: child.level.clone(),
                    spans: child.spans.clone(),
                });
            }
        }

        suggestions.sort_by_key(|s| (s.line, s.column, s.replacement.clone()));
        suggestions.dedup_by(|a, b| {
            a.file == b.file
                && a.line == b.line
                && a.column == b.column
                && a.replacement == b.replacement
        });

        spans.sort_by_key(|span| (span.line_start, span.column_start));

        if let Some(first) = spans.first_mut().filter(|span| !span.is_primary) {
            first.is_primary = true;
        }

        Some(Self {
            code,
            level: msg.level.clone(),
            raw_message: msg.message.clone(),
            spans,
            suggestions,
            children,
            help,
            notes,
            rendered: msg.rendered.clone(),
        })
    }

    pub fn primary_spans(&self) -> impl Iterator<Item = &Span> {
        self.spans.iter().filter(|span| span.is_primary)
    }

    pub fn primary_span(&self) -> Option<&Span> {
        self.primary_spans().next()
    }
}

fn span_to_suggestion(span: &Span, parent_label: Option<&str>, out: &mut Vec<CompilerSuggestion>) {
    let Some(replacement) = &span.suggested_replacement else {
        return;
    };

    out.push(CompilerSuggestion {
        file: span.file_name.clone(),
        line: span.line_start,
        column: span.column_start,
        column_end: span.column_end,
        replacement: replacement.clone(),
        applicability: span
            .suggestion_applicability
            .clone()
            .unwrap_or_else(|| "Unknown".to_string()),
        label: parent_label
            .map(str::to_string)
            .or_else(|| span.label.clone()),
    });
}
