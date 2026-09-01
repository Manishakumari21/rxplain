use crate::diagnostics::{CompilerSuggestion, ParsedError};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum FixKind {
    CompilerSuggested,
    RequiresHumanJudgment,
}

#[derive(Debug)]
pub struct FixSuggestion {
    pub kind: FixKind,
    pub description: String,
    pub suggestion: Option<CompilerSuggestion>,
}

pub fn suggest_fix(err: &ParsedError) -> FixSuggestion {
    let Some(suggestion) = err
        .suggestions
        .iter()
        .find(|suggestion| suggestion.applicability == "MachineApplicable")
    else {
        return FixSuggestion {
            kind: FixKind::RequiresHumanJudgment,
            description: "The compiler did not provide an automatically applicable fix."
                .to_string(),
            suggestion: None,
        };
    };

    FixSuggestion {
        kind: FixKind::CompilerSuggested,
        description: "rustc provided a machine-applicable suggestion for this location."
            .to_string(),
        suggestion: Some(suggestion.clone()),
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn apply_fix(suggestion: &CompilerSuggestion, project_dir: &str) -> anyhow::Result<()> {
    apply_fixes(std::slice::from_ref(suggestion), project_dir)
}
pub fn apply_fixes(suggestions: &[CompilerSuggestion], project_dir: &str) -> anyhow::Result<()> {
    let suggestions: Vec<&CompilerSuggestion> = suggestions
        .iter()
        .filter(|suggestion| suggestion.applicability == "MachineApplicable")
        .collect();

    if suggestions.is_empty() {
        return Ok(());
    }

    let mut by_file: HashMap<PathBuf, Vec<&CompilerSuggestion>> = HashMap::new();

    for suggestion in suggestions {
        let path = Path::new(project_dir).join(&suggestion.file);
        by_file.entry(path).or_default().push(suggestion);
    }

    for (path, file_suggestions) in by_file {
        let mut file_suggestions = file_suggestions;
        file_suggestions.sort_by_key(|suggestion| (suggestion.line, suggestion.column));

        let new_source = apply_to_source(&path, &file_suggestions)?;

        fs::write(&path, new_source)?;
    }

    Ok(())
}

fn apply_to_source(path: &Path, suggestions: &[&CompilerSuggestion]) -> anyhow::Result<String> {
    let source = fs::read_to_string(path)?;

    let had_trailing_newline = source.ends_with('\n');

    let lines: Vec<&str> = source.lines().collect();

    let mut edits: Vec<(usize, usize, usize, &str)> = Vec::new();

    for suggestion in suggestions {
        let line_index = suggestion.line.saturating_sub(1) as usize;

        if line_index >= lines.len() {
            anyhow::bail!(
                "Fix location is outside the source file: {}:{}",
                suggestion.file,
                suggestion.line
            );
        }

        let line = lines[line_index];

        let start = suggestion.column.saturating_sub(1) as usize;

        let end = suggestion.column_end.saturating_sub(1) as usize;

        if start > end {
            anyhow::bail!(
                "Invalid fix range: {}:{}:{}-{}",
                suggestion.file,
                suggestion.line,
                suggestion.column,
                suggestion.column_end
            );
        }

        if start > line.len() {
            anyhow::bail!(
                "Fix start column is outside the source line: {}:{}:{}",
                suggestion.file,
                suggestion.line,
                suggestion.column
            );
        }

        if end > line.len() {
            anyhow::bail!(
                "Fix end column is outside the source line: {}:{}:{}-{}",
                suggestion.file,
                suggestion.line,
                suggestion.column,
                suggestion.column_end
            );
        }

        edits.push((line_index, start, end, &suggestion.replacement));
    }

    edits.sort_by_key(|(line, start, ..)| (*line, *start));

    for window in edits.windows(2) {
        let (first_line, _, first_end, ..) = window[0];
        let (second_line, second_start, ..) = window[1];

        if first_line == second_line && second_start < first_end {
            anyhow::bail!(
                "Conflicting fixes overlap on line {}; refusing to apply.",
                first_line + 1
            );
        }
    }

    let mut result = String::new();

    for (index, current_line) in lines.iter().enumerate() {
        if index > 0 {
            result.push('\n');
        }

        let line_edits: Vec<(usize, usize, &str)> = edits
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
            anyhow::bail!("Overlapping edits on the same line.");
        }

        result.push_str(&line[cursor..*start]);

        result.push_str(replacement);

        cursor = *end;
    }

    result.push_str(&line[cursor..]);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn applies_machine_applicable_fix() {
        let temp_dir = std::env::temp_dir().join("rxplain_fixer_test");

        fs::create_dir_all(temp_dir.join("src")).unwrap();

        let file = temp_dir.join("src/main.rs");

        fs::write(&file, "fn main() {\n    let name = \"Manisha\";\n}\n").unwrap();

        let suggestion = CompilerSuggestion {
            file: "src/main.rs".to_string(),
            line: 2,
            column: 25,
            column_end: 25,
            replacement: ".to_string()".to_string(),
            applicability: "MachineApplicable".to_string(),
            label: Some("convert to String".to_string()),
        };

        apply_fix(&suggestion, temp_dir.to_str().unwrap()).unwrap();

        let result = fs::read_to_string(&file).unwrap();

        assert_eq!(
            result,
            "fn main() {\n    let name = \"Manisha\".to_string();\n}\n"
        );

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn applies_multiple_non_overlapping_suggestions() {
        let temp_dir = std::env::temp_dir().join("rxplain_fixer_multi_test");

        fs::create_dir_all(temp_dir.join("src")).unwrap();

        let file = temp_dir.join("src/main.rs");

        fs::write(
            &file,
            "fn main() {\n    let a = 1;\n    let b = 2;\n    println!(\"{} {}\", a, b);\n}\n",
        )
        .unwrap();

        let first = CompilerSuggestion {
            file: "src/main.rs".to_string(),
            line: 2,
            column: 9,
            column_end: 9,
            replacement: "mut ".to_string(),
            applicability: "MachineApplicable".to_string(),
            label: None,
        };

        let second = CompilerSuggestion {
            file: "src/main.rs".to_string(),
            line: 3,
            column: 9,
            column_end: 9,
            replacement: "mut ".to_string(),
            applicability: "MachineApplicable".to_string(),
            label: None,
        };

        apply_fixes(&[first, second], temp_dir.to_str().unwrap()).unwrap();

        let result = fs::read_to_string(&file).unwrap();

        assert_eq!(
            result,
            "fn main() {\n    let mut a = 1;\n    let mut b = 2;\n    println!(\"{} {}\", a, b);\n}\n"
        );

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn rejects_overlapping_suggestions() {
        let temp_dir = std::env::temp_dir().join("rxplain_fixer_overlap_test");

        fs::create_dir_all(temp_dir.join("src")).unwrap();

        let file = temp_dir.join("src/main.rs");

        fs::write(&file, "fn main() {\n    let name = value;\n}\n").unwrap();

        let first = CompilerSuggestion {
            file: "src/main.rs".to_string(),
            line: 2,
            column: 10,
            column_end: 14,
            replacement: "other".to_string(),
            applicability: "MachineApplicable".to_string(),
            label: None,
        };

        let overlapping = CompilerSuggestion {
            file: "src/main.rs".to_string(),
            line: 2,
            column: 12,
            column_end: 16,
            replacement: "x".to_string(),
            applicability: "MachineApplicable".to_string(),
            label: None,
        };

        let result = apply_fixes(&[first, overlapping], temp_dir.to_str().unwrap());

        assert!(result.is_err());

        fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn ignores_non_machine_applicable_suggestions() {
        let temp_dir = std::env::temp_dir().join("rxplain_fixer_ignore_test");

        fs::create_dir_all(temp_dir.join("src")).unwrap();

        let file = temp_dir.join("src/main.rs");

        fs::write(&file, "fn main() {\n    let name = \"Manisha\";\n}\n").unwrap();

        let not_guaranteed = CompilerSuggestion {
            file: "src/main.rs".to_string(),
            line: 2,
            column: 25,
            column_end: 25,
            replacement: ".to_string()".to_string(),
            applicability: "MaybeIncorrect".to_string(),
            label: None,
        };

        apply_fixes(&[not_guaranteed], temp_dir.to_str().unwrap()).unwrap();

        let result = fs::read_to_string(&file).unwrap();

        assert_eq!(result, "fn main() {\n    let name = \"Manisha\";\n}\n");

        fs::remove_dir_all(temp_dir).ok();
    }
}
