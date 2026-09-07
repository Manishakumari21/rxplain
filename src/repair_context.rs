use crate::analyzer::DiagnosticAnalysis;
use crate::diagnostics::{ParsedError, Span};
use std::collections::BTreeMap;

/// Structured context handed to repair transformations.
///
/// It bundles the parsed diagnostic, the derived analysis, and the relevant
/// source text so a transformation can reason about the program location
/// without re-reading files. The source is intentionally stored as plain
/// text; a structured syntax representation can be added later without
/// changing this interface.
#[derive(Debug)]
pub struct RepairContext<'a> {
    pub error: &'a ParsedError,
    #[allow(dead_code)]
    pub analysis: &'a DiagnosticAnalysis,
    sources: BTreeMap<String, String>,
}

/// A compiler-reported location with its label.
#[derive(Debug, Clone)]
pub struct LocatedSpan {
    pub file: String,
    pub line: u32,
    #[allow(dead_code)]
    pub column: u32,
    #[allow(dead_code)]
    pub label: String,
}

/// Evidence that a value was moved and then used again.
#[derive(Debug, Clone)]
pub struct MovedVar {
    pub name: String,
    pub move_site: Option<LocatedSpan>,
    pub use_after_move: Option<LocatedSpan>,
}

/// The `let`/`let mut` declaration of a binding.
#[derive(Debug, Clone)]
pub struct Binding {
    pub file: String,
    pub line: u32,
    /// Character column (1-based) of the variable name.
    #[allow(dead_code)]
    pub name_column: u32,
    /// Character column (1-based) immediately after `let ` where `mut ` is
    /// inserted.
    pub mut_insert_column: u32,
    pub is_mut: bool,
}

impl<'a> RepairContext<'a> {
    pub fn new(
        error: &'a ParsedError,
        analysis: &'a DiagnosticAnalysis,
        sources: BTreeMap<String, String>,
    ) -> Self {
        Self {
            error,
            analysis,
            sources,
        }
    }

    /// Convenience constructor with a single source file. The file name comes
    /// from the first reported span.
    pub fn from_source(
        error: &'a ParsedError,
        analysis: &'a DiagnosticAnalysis,
        source_text: &'a str,
    ) -> Self {
        let mut sources = BTreeMap::new();
        if let Some(file) = error.spans.first().map(|s| s.file_name.clone()) {
            sources.insert(file, source_text.to_string());
        }
        Self::new(error, analysis, sources)
    }

    pub fn primary_span(&self) -> Option<&Span> {
        self.error
            .primary_span()
            .or_else(|| self.error.spans.first())
    }

    pub fn primary_file(&self) -> Option<&str> {
        self.primary_span().map(|span| span.file_name.as_str())
    }

    pub fn line(&self, file: &str, line_number: u32) -> Option<&str> {
        let source = self.sources.get(file)?;
        source.lines().nth(line_number.saturating_sub(1) as usize)
    }

    /// A value that the compiler reports as moved and still in use afterwards.
    ///
    /// This derives evidence purely from the diagnostic message shape and span
    /// labels — never from an error code — so it stays applicable to any
    /// ownership diagnostic that reports the same evidence.
    pub fn moved_var(&self) -> Option<MovedVar> {
        let name = extract_moved_var_name(self.error)?;

        let mut move_site = None;
        let mut use_after_move = None;

        for span in &self.error.spans {
            let Some(label) = &span.label else {
                continue;
            };
            let l = label.to_lowercase();

            if l.contains("moved here") || l.contains("move occurs") {
                if move_site.is_none() {
                    move_site = Some(LocatedSpan::from_span(span));
                }
            } else if l.contains("used here after move") || l.contains("borrowed here after move") {
                use_after_move = Some(LocatedSpan::from_span(span));
            }
        }

        if move_site.is_none() {
            for span in &self.error.spans {
                if self.is_call_argument(&name, span.file_name.as_str(), span.line_start) {
                    move_site = Some(LocatedSpan::from_span(span));
                    break;
                }
            }
        }

        Some(MovedVar {
            name,
            move_site,
            use_after_move,
        })
    }

    /// Is the token at the reported location a bare argument of a call on the
    /// same line?
    pub fn is_call_argument(&self, token: &str, file: &str, line_number: u32) -> bool {
        let Some(line_text) = self.line(file, line_number) else {
            return false;
        };
        let Some(byte) = find_token_byte(line_text, token) else {
            return false;
        };
        let before = &line_text[..byte];
        let after = &line_text[byte + token.len()..];
        let before_ok = before.contains('(')
            && before.chars().last().is_some_and(|c| c != '&')
            && !after.starts_with(".clone()");
        before_ok && after.contains(')')
    }

    /// Find the `let`/`let mut` declaration of `name`, scanning upward from
    /// `near_line` within the same file.
    pub fn binding(&self, name: &str, near_line: u32) -> Option<Binding> {
        let file = self.primary_file()?;
        let depth = 60u32.min(near_line);
        for line_number in near_line.saturating_sub(depth)..near_line {
            let line_text = self.line(file, line_number)?;
            let Some(let_byte) = find_token_byte(line_text, "let") else {
                continue;
            };
            let after_let = line_text[let_byte + "let".len()..].trim_start();
            let pattern_start_byte = line_text.len() - after_let.len();
            let is_mut = after_let.starts_with("mut ") || after_let.starts_with("mut\t");
            let pattern = if is_mut {
                after_let["mut".len()..].trim_start()
            } else {
                after_let
            };
            if find_token_byte(pattern, name).is_none() {
                continue;
            }
            return Some(Binding {
                file: file.to_string(),
                line: line_number,
                name_column: byte_to_char_column(line_text, pattern_start_byte),
                mut_insert_column: byte_to_char_column(line_text, pattern_start_byte),
                is_mut,
            });
        }
        None
    }
}

impl LocatedSpan {
    fn from_span(span: &Span) -> Self {
        Self {
            file: span.file_name.clone(),
            line: span.line_start,
            column: span.column_start,
            label: span.label.clone().unwrap_or_default(),
        }
    }
}

fn extract_moved_var_name(error: &ParsedError) -> Option<String> {
    for prefix in [
        "use of moved value: `",
        "borrow of moved value: `",
        "move out of `",
        "cannot move out of `",
    ] {
        if let Some(message) = error.raw_message.strip_prefix(prefix)
            && let Some(end) = message.find('`')
        {
            let name = message[..end].to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }

    for span in &error.spans {
        let Some(label) = &span.label else {
            continue;
        };
        let l = label.to_lowercase();
        let relevant = l.contains("moved here")
            || l.contains("borrowed here after move")
            || l.contains("used here after move")
            || l.contains("value moved");
        if !relevant {
            continue;
        }
        if let Some(name) = extract_backticked_identifier(label) {
            return Some(name);
        }
    }

    None
}

fn extract_backticked_identifier(label: &str) -> Option<String> {
    let start = label.find('`')?;
    let rest = &label[start + 1..];
    let end = rest.find('`')?;
    let name = rest[..end].trim();
    if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        Some(name.to_string())
    } else {
        None
    }
}

/// Find a whole-word byte offset of `token` in `line`.
pub fn find_token_byte(line: &str, token: &str) -> Option<usize> {
    if token.is_empty() {
        return None;
    }
    let token_len = token.len();
    let line_len = line.len();
    let mut search_from = 0;
    while let Some(relative) = line[search_from..].find(token) {
        let offset = search_from + relative;
        let before_ok = offset == 0 || !is_word_char(line.as_bytes().get(offset - 1));
        let after_pos = offset + token_len;
        let after_ok = after_pos >= line_len
            || (after_pos < line_len && !is_word_char(line.as_bytes().get(after_pos)));
        if before_ok && after_ok {
            return Some(offset);
        }
        search_from = offset + token_len;
    }
    None
}

fn is_word_char(byte: Option<&u8>) -> bool {
    byte.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

/// Convert a byte offset within `line` into a 1-based character column.
pub fn byte_to_char_column(line: &str, byte_offset: usize) -> u32 {
    if byte_offset >= line.len() {
        return line.chars().count() as u32 + 1;
    }
    if byte_offset == 0 {
        return 1;
    }
    let char_count = line[..byte_offset].chars().count() as u32;
    char_count + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Span, SpanText};
    use std::collections::BTreeMap;

    fn error_with_spans(message: &str, spans: Vec<(u32, bool, &str)>) -> ParsedError {
        let parsed_spans: Vec<Span> = spans
            .iter()
            .map(|(line, is_primary, label)| Span {
                file_name: "src/main.rs".to_string(),
                line_start: *line,
                line_end: *line,
                column_start: 5,
                column_end: 10,
                is_primary: *is_primary,
                label: Some(label.to_string()),
                text: vec![SpanText {
                    text: "code".to_string(),
                }],
                suggested_replacement: None,
                suggestion_applicability: None,
            })
            .collect();
        ParsedError {
            code: String::new(),
            raw_message: message.to_string(),
            spans: parsed_spans,
            ..Default::default()
        }
    }

    fn ctx<'a>(
        error: &'a ParsedError,
        analysis: &'a crate::analyzer::DiagnosticAnalysis,
        source: &'a str,
    ) -> RepairContext<'a> {
        let mut sources = BTreeMap::new();
        sources.insert("src/main.rs".to_string(), source.to_string());
        RepairContext::new(error, analysis, sources)
    }

    #[test]
    fn moved_var_from_message() {
        let source = "fn main() {\n    let value = String::from(\"x\");\n    drop(value);\n}\n";
        let error = error_with_spans(
            "borrow of moved value: `value`",
            vec![(3, true, "value moved here")],
        );
        let analysis = crate::analyzer::analyze(&error);
        let rctx = ctx(&error, &analysis, source);
        let moved = rctx.moved_var().unwrap();
        assert_eq!(moved.name, "value");
        assert_eq!(moved.move_site.as_ref().unwrap().line, 3);
    }

    #[test]
    fn moved_var_detects_use_after_move() {
        let source = "fn main() {\n    let value = String::from(\"x\");\n    drop(value);\n    println!(\"{}\", value);\n}\n";
        let error = error_with_spans(
            "use of moved value: `value`",
            vec![
                (3, false, "value moved here"),
                (4, true, "value used here after move"),
            ],
        );
        let analysis = crate::analyzer::analyze(&error);
        let rctx = ctx(&error, &analysis, source);
        let moved = rctx.moved_var().unwrap();
        assert_eq!(moved.use_after_move.as_ref().unwrap().line, 4);
    }

    #[test]
    fn moved_var_without_evidence_is_none() {
        let source = "fn main() {}\n";
        let error = error_with_spans("mismatched types", vec![(1, true, "expected i32")]);
        let analysis = crate::analyzer::analyze(&error);
        let rctx = ctx(&error, &analysis, source);
        assert!(rctx.moved_var().is_none());
    }

    #[test]
    fn call_argument_detection() {
        let source = "fn main() {\n    drop(value);\n}\n";
        let error = error_with_spans("use of moved value: `value`", vec![(2, true, "moved here")]);
        let analysis = crate::analyzer::analyze(&error);
        let rctx = ctx(&error, &analysis, source);
        assert!(rctx.is_call_argument("value", "src/main.rs", 2));
        assert!(!rctx.is_call_argument("value", "src/main.rs", 1));
    }

    #[test]
    fn binding_reports_mutability() {
        let source = "fn main() {\n    let x = 10;\n    x += 5;\n}\n";
        let error = error_with_spans(
            "cannot assign twice to immutable variable `x`",
            vec![(3, true, "cannot assign")],
        );
        let analysis = crate::analyzer::analyze(&error);
        let rctx = ctx(&error, &analysis, source);
        let binding = rctx.binding("x", 3).unwrap();
        assert!(!binding.is_mut);
        assert_eq!(binding.line, 2);
    }

    #[test]
    fn binding_reports_mut() {
        let source = "fn main() {\n    let mut x = 10;\n    x += 5;\n}\n";
        let error = error_with_spans(
            "cannot assign twice to immutable variable `x`",
            vec![(3, true, "cannot assign")],
        );
        let analysis = crate::analyzer::analyze(&error);
        let rctx = ctx(&error, &analysis, source);
        let binding = rctx.binding("x", 3).unwrap();
        assert!(binding.is_mut);
    }

    #[test]
    fn find_token_byte_finds_whole_word() {
        let line = "    drop(value);";
        let pos = find_token_byte(line, "value").unwrap();
        assert_eq!(&line[pos..pos + 5], "value");
    }

    #[test]
    fn find_token_byte_ignores_partial_match() {
        let line = "    let myvalue = 1;";
        assert!(find_token_byte(line, "value").is_none());
    }

    #[test]
    fn byte_to_char_column_works_unicode() {
        let line = "    let 名前 = 1;";
        assert_eq!(byte_to_char_column(line, 0), 1);
        assert_eq!(byte_to_char_column(line, 4), 5);
        let name_start = line.find('名').unwrap();
        assert_eq!(byte_to_char_column(line, name_start), 9);
    }
}
