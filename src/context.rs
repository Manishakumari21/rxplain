use crate::diagnostics::{ParsedError, Span};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SourceContext {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub lines: Vec<ContextLine>,
}

#[derive(Debug, Clone)]
pub struct ContextLine {
    pub line_number: u32,
    pub text: String,
    pub highlighted: bool,
    pub label: Option<String>,
}

impl SourceContext {
    pub fn from_error(error: &ParsedError, project_dir: &str) -> Vec<Self> {
        let mut contexts = Vec::new();

        for span in &error.spans {
            let Some(context) = Self::from_span(span, project_dir) else {
                continue;
            };

            if contexts.iter().any(|existing: &SourceContext| {
                existing.file == context.file
                    && existing.start_line == context.start_line
                    && existing.end_line == context.end_line
            }) {
                continue;
            }

            contexts.push(context);
        }

        contexts
    }

    fn from_span(span: &Span, project_dir: &str) -> Option<Self> {
        let path = resolve_source_path(project_dir, &span.file_name)?;

        let source = fs::read_to_string(&path).ok()?;
        let source_lines: Vec<&str> = source.lines().collect();

        if source_lines.is_empty() {
            return None;
        }

        let line = span.line_start.max(1);

        let context_radius = 2;

        let start_line = line.saturating_sub(context_radius).max(1);

        let end_line = (line + context_radius).min(source_lines.len() as u32);

        let mut lines = Vec::new();

        for line_number in start_line..=end_line {
            let text = source_lines
                .get((line_number - 1) as usize)
                .unwrap_or(&"")
                .to_string();

            lines.push(ContextLine {
                line_number,
                text,
                highlighted: line_number >= span.line_start && line_number <= span.line_end,
                label: if line_number == span.line_start {
                    span.label.clone()
                } else {
                    None
                },
            });
        }

        Some(SourceContext {
            file: path.to_string_lossy().to_string(),
            start_line,
            end_line,
            lines,
        })
    }
}

fn resolve_source_path(project_dir: &str, file_name: &str) -> Option<PathBuf> {
    let file_path = Path::new(file_name);

    if file_path.is_absolute() && file_path.exists() {
        return Some(file_path.to_path_buf());
    }

    let project_path = Path::new(project_dir);

    let candidate = project_path.join(file_path);

    if candidate.exists() {
        return Some(candidate);
    }

    let candidate = project_path.join("src").join(file_path);

    if candidate.exists() {
        return Some(candidate);
    }

    None
}
