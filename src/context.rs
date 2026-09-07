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
        let path = resolve_project_path(project_dir, &span.file_name)?;

        let source = fs::read_to_string(&path).ok()?;
        let source_lines: Vec<&str> = source.lines().collect();

        if source_lines.is_empty() {
            return None;
        }

        let context_radius = 2;

        let span_end_line = span.line_end.max(span.line_start);

        let start_line = (span.line_start.saturating_sub(context_radius)).max(1);

        let end_line = (span_end_line + context_radius).min(source_lines.len() as u32);

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

pub fn resolve_project_path(project_dir: &str, file_name: &str) -> Option<PathBuf> {
    let file_path = Path::new(file_name);

    if file_path.is_absolute() && file_path.exists() {
        return Some(file_path.to_path_buf());
    }

    find_in_project(Path::new(project_dir), file_path, 0)
}

fn find_in_project(dir: &Path, relative: &Path, depth: u32) -> Option<PathBuf> {
    if depth > 6 {
        return None;
    }

    let direct = dir.join(relative);

    if direct.exists() {
        return Some(direct);
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return None;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if matches!(name.as_ref(), "target" | ".git" | "node_modules") {
            continue;
        }

        let path = entry.path();

        if path.is_dir() {
            let nested = path.join(relative);

            if nested.exists() {
                return Some(nested);
            }

            if let Some(found) = find_in_project(&path, relative, depth + 1) {
                return Some(found);
            }
        }
    }

    None
}
