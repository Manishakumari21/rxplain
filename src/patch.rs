use std::fmt;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub file: String,
    pub line: u32,
    pub start_col: u32,
    pub end_col: u32,
    pub replacement: String,
    pub label: Option<String>,
}

impl Edit {
    #[allow(dead_code)]
    pub fn new(
        file: impl Into<String>,
        line: u32,
        start_col: u32,
        end_col: u32,
        replacement: impl Into<String>,
    ) -> Self {
        Self {
            file: file.into(),
            line,
            start_col,
            end_col,
            replacement: replacement.into(),
            label: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct Patch {
    pub edits: Vec<Edit>,
}

impl Patch {
    pub fn new(edits: Vec<Edit>) -> Self {
        Self { edits }
    }

    pub fn preview(&self) -> String {
        if self.edits.is_empty() {
            return String::new();
        }

        let mut by_file: std::collections::BTreeMap<&str, Vec<&Edit>> =
            std::collections::BTreeMap::new();

        for edit in &self.edits {
            by_file.entry(&edit.file).or_default().push(edit);
        }

        let mut out = String::new();

        for (file, edits) in by_file {
            out.push_str(&format!("--- {}\n+++ {}\n", file, file));
            for edit in edits {
                let marker = if edit.replacement.is_empty() {
                    "-"
                } else {
                    "+"
                };
                let replacement = if edit.replacement.is_empty() {
                    " (remove)".to_string()
                } else {
                    edit.replacement.clone()
                };
                out.push_str(&format!(
                    "@@ -{}:{} @@\n{}{}\n",
                    edit.line, edit.start_col, marker, replacement
                ));
            }
        }

        out
    }

    pub fn validate(&self, root_dir: &str) -> anyhow::Result<()> {
        for edit in &self.edits {
            let path = crate::context::resolve_project_path(root_dir, &edit.file)
                .ok_or_else(|| anyhow::anyhow!("cannot locate source file: {}", edit.file))?;
            let source = fs::read_to_string(&path)?;
            validate_edit(edit, &source)?;
        }
        Ok(())
    }

    pub fn apply(&self, root_dir: &str) -> anyhow::Result<()> {
        let mut by_file: std::collections::BTreeMap<PathBuf, Vec<&Edit>> =
            std::collections::BTreeMap::new();

        for edit in &self.edits {
            let path = crate::context::resolve_project_path(root_dir, &edit.file)
                .ok_or_else(|| anyhow::anyhow!("cannot locate source file: {}", edit.file))?;
            validate_edit(edit, &fs::read_to_string(&path)?)?;
            by_file.entry(path).or_default().push(edit);
        }

        for (path, edits) in by_file {
            let source = fs::read_to_string(&path)?;
            let new_source = apply_edits_to_source(&source, &edits)?;
            fs::write(&path, new_source)?;
        }

        Ok(())
    }
}

fn validate_edit(edit: &Edit, source: &str) -> anyhow::Result<()> {
    let line = source
        .lines()
        .nth((edit.line.saturating_sub(1)) as usize)
        .ok_or_else(|| anyhow::anyhow!("edit line {} out of range", edit.line))?;

    let start = char_column_to_byte(line, edit.start_col)?;
    let end = char_column_to_byte(line, edit.end_col)?;

    if start > end {
        anyhow::bail!(
            "invalid edit range {}:{}:{}-{}",
            edit.file,
            edit.line,
            edit.start_col,
            edit.end_col
        );
    }

    Ok(())
}

fn apply_edits_to_source(source: &str, edits: &[&Edit]) -> anyhow::Result<String> {
    let had_trailing_newline = source.ends_with('\n');
    let lines: Vec<&str> = source.lines().collect();

    let mut byte_edits: Vec<(usize, usize, usize, &str)> = Vec::new();

    for edit in edits {
        let line_index = edit.line.saturating_sub(1) as usize;

        let line = lines
            .get(line_index)
            .ok_or_else(|| anyhow::anyhow!("edit line {} out of range", edit.line))?;

        let start = char_column_to_byte(line, edit.start_col)?;
        let end = char_column_to_byte(line, edit.end_col)?;

        if start > end {
            anyhow::bail!("invalid edit range");
        }

        byte_edits.push((line_index, start, end, edit.replacement.as_str()));
    }

    byte_edits.sort_by_key(|(line, start, ..)| (*line, *start));

    for window in byte_edits.windows(2) {
        if window[0].0 == window[1].0 && window[1].1 < window[0].2 {
            anyhow::bail!(
                "overlapping edits on line {}; refusing to apply",
                window[0].0 + 1
            );
        }
    }

    let mut result = String::new();

    for (index, current_line) in lines.iter().enumerate() {
        if index > 0 {
            result.push('\n');
        }

        let line_edits: Vec<(usize, usize, &str)> = byte_edits
            .iter()
            .filter(|(line, ..)| *line == index)
            .map(|(_, start, end, replacement)| (*start, *end, *replacement))
            .collect();

        result.push_str(&apply_line_edits(current_line, &line_edits)?);
    }

    if had_trailing_newline {
        result.push('\n');
    }

    Ok(result)
}

fn apply_line_edits(line: &str, edits: &[(usize, usize, &str)]) -> anyhow::Result<String> {
    let mut result = String::new();

    let mut cursor = 0;

    for (start, end, replacement) in edits {
        if *start < cursor {
            anyhow::bail!("overlapping edits on the same line");
        }

        result.push_str(&line[cursor..*start]);
        result.push_str(replacement);
        cursor = *end;
    }

    result.push_str(&line[cursor..]);

    Ok(result)
}

pub fn char_column_to_byte(line: &str, char_column: u32) -> anyhow::Result<usize> {
    if char_column == 0 {
        return Ok(0);
    }

    let target = (char_column - 1) as usize;

    if target == 0 {
        return Ok(0);
    }

    match line.char_indices().nth(target) {
        Some((byte_index, _)) => Ok(byte_index),
        None => {
            if target > line.chars().count() {
                anyhow::bail!("column {} exceeds line length", char_column);
            }
            Ok(line.len())
        }
    }
}

impl fmt::Display for Patch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.preview())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_insertion_edit_on_non_ascii_line() {
        let line = "    let 名前 = \"hi\";";

        let start = char_column_to_byte(line, 18).unwrap();
        let end = char_column_to_byte(line, 18).unwrap();

        let edited = apply_line_edits(line, &[(start, end, ".to_string()")]).unwrap();

        assert_eq!(edited, "    let 名前 = \"hi\".to_string();");
    }

    #[test]
    fn applies_replacement_edit() {
        let line = "let x = 42;";

        let start = char_column_to_byte(line, 5).unwrap();
        let end = char_column_to_byte(line, 6).unwrap();

        let edited = apply_line_edits(line, &[(start, end, "y")]).unwrap();

        assert_eq!(edited, "let y = 42;");
    }

    #[test]
    fn rejects_overlapping_edits_on_same_line() {
        let line = "let value = 1;";

        let a_start = char_column_to_byte(line, 5).unwrap();
        let a_end = char_column_to_byte(line, 10).unwrap();
        let b_start = char_column_to_byte(line, 7).unwrap();
        let b_end = char_column_to_byte(line, 12).unwrap();

        let result = apply_line_edits(line, &[(a_start, a_end, "x"), (b_start, b_end, "y")]);

        assert!(result.is_err());
    }

    #[test]
    fn applies_multiple_non_overlapping_edits_on_same_line() {
        let line = "let a = 1; let b = 2;";

        let first_start = char_column_to_byte(line, 5).unwrap();
        let second_start = char_column_to_byte(line, 16).unwrap();

        let edits = vec![
            (first_start, first_start, "mut "),
            (second_start, second_start, "mut "),
        ];

        let edited = apply_line_edits(line, &edits).unwrap();

        assert_eq!(edited, "let mut a = 1; let mut b = 2;");
    }

    #[test]
    fn patch_validate_rejects_out_of_range_line() {
        let temp_dir = std::env::temp_dir().join(format!(
            "rxplain_patch_validate_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        std::fs::create_dir_all(temp_dir.join("src")).unwrap();
        std::fs::write(temp_dir.join("src/main.rs"), "fn main() {}\n").unwrap();

        let edit = Edit::new("src/main.rs", 999, 1, 1, "x");
        let patch = Patch::new(vec![edit]);

        let err = patch
            .validate(temp_dir.to_str().unwrap())
            .err()
            .expect("should fail");

        assert!(err.to_string().contains("out of range"));

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn patch_preview_renders_edits() {
        let edit = Edit::new("src/main.rs", 2, 9, 9, "mut ");

        let patch = Patch::new(vec![edit]);

        let preview = patch.preview();

        assert!(preview.contains("src/main.rs"));
        assert!(preview.contains("+mut "));
    }
}
