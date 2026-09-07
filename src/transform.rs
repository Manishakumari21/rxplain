use crate::analyzer::DiagnosticAnalysis;
use crate::diagnostics::{Applicability, ParsedError};
use crate::patch::{Edit, Patch};
use crate::repair::{Confidence, RepairCandidate, RepairKind};
use crate::repair_context::{LocatedSpan, RepairContext, byte_to_char_column, find_token_byte};

/// A reusable repair transformation.
///
/// A transformation decides whether it is applicable from the evidence in the
/// [`RepairContext`] — spans, labels, message shape, and source text — not from
/// an error code.
pub trait Transform {
    fn name(&self) -> &'static str;
    fn applicable(&self, ctx: &RepairContext) -> bool;
    fn generate(&self, ctx: &RepairContext) -> Vec<RepairCandidate>;
}

/// Generate repair candidates for an error by running every registered
/// transformation over the context. A diagnostic may yield zero, one, or
/// several candidates; all are valid outcomes.
pub fn generate_candidates(
    error: &ParsedError,
    analysis: &DiagnosticAnalysis,
    source_text: &str,
) -> Vec<RepairCandidate> {
    let ctx = RepairContext::from_source(error, analysis, source_text);
    let transforms: Vec<Box<dyn Transform>> = vec![
        Box::new(CompilerSuggestionTransform),
        Box::new(MutabilityTransform),
        Box::new(BorrowTransform),
        Box::new(CloneTransform),
    ];

    let mut candidates = Vec::new();

    for transform in &transforms {
        if transform.applicable(&ctx) {
            let generated = transform.generate(&ctx);
            for mut candidate in generated {
                stamp_edit_anchors(&mut candidate, source_text);
                candidate
                    .evidence
                    .insert(0, format!("Transformation: {}", transform.name()));
                candidates.push(candidate);
            }
        }
    }

    deduplicate_candidates(candidates)
}

/// Record the trimmed source line each edit was generated against so a later
/// apply can detect that the file changed (stale patch).
fn stamp_edit_anchors(candidate: &mut RepairCandidate, source_text: &str) {
    let lines: Vec<&str> = source_text.lines().collect();

    for edit in &mut candidate.patch.edits {
        if edit.anchor.is_some() {
            continue;
        }
        let Some(line) = lines.get(edit.line.saturating_sub(1) as usize) else {
            continue;
        };
        edit.anchor = Some(line.trim().to_string());
    }
}

fn deduplicate_candidates(candidates: Vec<RepairCandidate>) -> Vec<RepairCandidate> {
    let mut seen = std::collections::BTreeSet::new();
    let mut unique = Vec::new();

    for candidate in candidates {
        let signature = crate::repair::patch_signature(&candidate.patch);
        if seen.insert(signature) {
            unique.push(candidate);
        }
    }

    unique
}

pub fn source_text_for(error: &ParsedError, project_dir: &str) -> Option<String> {
    let file = error.spans.first()?.file_name.clone();
    let path = crate::context::resolve_project_path(project_dir, &file)?;
    std::fs::read_to_string(path).ok()
}

/// 1. Apply a compiler-provided replacement.
///
/// This is the highest-confidence source of repair: rustc inspected the code
/// and produced a concrete, span-anchored replacement.
pub struct CompilerSuggestionTransform;

impl Transform for CompilerSuggestionTransform {
    fn name(&self) -> &'static str {
        "compiler_suggestion"
    }

    fn applicable(&self, ctx: &RepairContext) -> bool {
        !ctx.error.suggestions.is_empty()
    }

    fn generate(&self, ctx: &RepairContext) -> Vec<RepairCandidate> {
        if ctx.error.suggestions.is_empty() {
            return Vec::new();
        }

        // rustc's fixes are often composed of several coordinated edits (for
        // example the parts of a lifetime `<'a>` change). Group every
        // suggestion of this diagnostic into a single candidate so the patch
        // applies atomically; each individual edit is rarely sufficient alone.
        // Verification still guards the result, so an over-broad grouping is
        // merely rejected rather than applied incorrectly.
        let mut edits = Vec::new();
        let mut evidence = Vec::new();

        let worst_confidence = ctx
            .error
            .suggestions
            .iter()
            .map(|suggestion| suggestion.applicability_enum())
            .map(|applicability| match applicability {
                Applicability::MachineApplicable => Confidence::High,
                Applicability::MaybeIncorrect => Confidence::Low,
                _ => Confidence::Medium,
            })
            .min_by_key(|confidence| match confidence {
                Confidence::High => 0,
                Confidence::Medium => 1,
                Confidence::Low => 2,
            })
            .unwrap_or(Confidence::Medium);

        for suggestion in &ctx.error.suggestions {
            edits.push(Edit {
                file: suggestion.file.clone(),
                line: suggestion.line,
                start_col: suggestion.column,
                end_col: suggestion.column_end,
                replacement: suggestion.replacement.clone(),
                label: suggestion.label.clone(),
                anchor: None,
            });
            evidence.push(
                suggestion
                    .label
                    .clone()
                    .unwrap_or_else(|| format!("Applicability: {}", suggestion.applicability)),
            );
            evidence.push(format!(
                "Replace {}:{}-{} with `{}`",
                suggestion.file, suggestion.line, suggestion.column, suggestion.replacement
            ));
        }

        let primary = &ctx.error.suggestions[0];
        vec![RepairCandidate {
            kind: RepairKind::CompilerSuggested,
            confidence: worst_confidence,
            description: format!(
                "Apply {} compiler-suggested change{} at {}:{}.",
                ctx.error.suggestions.len(),
                if ctx.error.suggestions.len() == 1 {
                    ""
                } else {
                    "s"
                },
                primary.file,
                primary.line
            ),
            evidence,
            patch: Patch::new(edits),
        }]
    }
}

/// 3. Add mutability to a binding.
///
/// Evidence: the diagnostic reports assignment or mutation of a binding, and
/// the source shows that binding declared with `let` and not `let mut`. If the
/// compiler already proposed a mutability change for the same binding, that
/// candidate is preferred and this transformation stays silent (no duplicate).
pub struct MutabilityTransform;

impl Transform for MutabilityTransform {
    fn name(&self) -> &'static str {
        "mutability"
    }

    fn applicable(&self, ctx: &RepairContext) -> bool {
        let Some(target) = mutation_target(ctx) else {
            return false;
        };
        let Some(primary) = ctx.primary_span() else {
            return false;
        };
        let Some(binding) = ctx.binding(&target, primary.line_start) else {
            return false;
        };
        if binding.is_mut {
            return false;
        }
        !has_compiler_mutability_suggestion(ctx, &binding)
    }

    fn generate(&self, ctx: &RepairContext) -> Vec<RepairCandidate> {
        let Some(target) = mutation_target(ctx) else {
            return Vec::new();
        };
        let Some(primary) = ctx.primary_span() else {
            return Vec::new();
        };
        let Some(binding) = ctx.binding(&target, primary.line_start) else {
            return Vec::new();
        };
        if binding.is_mut {
            return Vec::new();
        }

        vec![RepairCandidate {
            kind: RepairKind::Mutability,
            confidence: Confidence::Medium,
            description: format!("Declare `{}` as mutable.", target),
            evidence: vec![
                format!(
                    "The compiler reports mutation of `{}` at {}:{}",
                    target, binding.file, primary.line_start
                ),
                format!(
                    "`{}` is declared with `let` at {}:{} and lacks `mut`",
                    target, binding.file, binding.line
                ),
                "Insert `mut ` into the binding declaration".to_string(),
            ],
            patch: Patch::new(vec![Edit {
                file: binding.file,
                line: binding.line,
                start_col: binding.mut_insert_column,
                end_col: binding.mut_insert_column,
                replacement: "mut ".to_string(),
                label: Some(format!("make `{}` mutable", target)),
                anchor: None,
            }]),
        }]
    }
}

fn mutation_target(ctx: &RepairContext) -> Option<String> {
    let message = &ctx.error.raw_message;
    let looks_like_mutation = message.contains("cannot assign")
        || message.contains("immutable variable")
        || (message.contains("cannot borrow") && message.contains("mutable"));

    if looks_like_mutation && let Some(name) = extract_backticked_identifier(message) {
        return Some(name);
    }

    for span in &ctx.error.spans {
        let Some(label) = &span.label else {
            continue;
        };
        let l = label.to_lowercase();
        let label_is_mutation = l.contains("cannot assign")
            || l.contains("assignment to")
            || l.contains("immutable variable")
            || l.contains("cannot borrow") && l.contains("mutable");
        if label_is_mutation {
            if let Some(name) = extract_backticked_identifier(label) {
                return Some(name);
            }
            if let Some(name) = extract_bare_identifier(label) {
                return Some(name);
            }
        }
    }

    None
}

fn extract_backticked_identifier(text: &str) -> Option<String> {
    let start = text.find('`')?;
    let rest = &text[start + 1..];
    let end = rest.find('`')?;
    let name = rest[..end].trim();
    if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        Some(name.to_string())
    } else {
        None
    }
}

fn extract_bare_identifier(label: &str) -> Option<String> {
    let lower = label.to_lowercase();
    let start = lower.find("variable ")? + "variable ".len();
    let rest = &label[start..];
    let candidate: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if candidate.is_empty() {
        None
    } else {
        Some(candidate)
    }
}

fn has_compiler_mutability_suggestion(
    ctx: &RepairContext,
    binding: &crate::repair_context::Binding,
) -> bool {
    ctx.error.suggestions.iter().any(|suggestion| {
        suggestion.file == binding.file
            && suggestion.line == binding.line
            && suggestion.replacement.contains("mut")
    })
}

/// 2. Borrow the value instead of moving it.
///
/// Evidence: a value is reported as moved at a call argument, then used again
/// afterwards. The surrounding source shows the move site accepts a bare
/// value, so inserting `&` yields a borrow.
pub struct BorrowTransform;

impl Transform for BorrowTransform {
    fn name(&self) -> &'static str {
        "borrow"
    }

    fn applicable(&self, ctx: &RepairContext) -> bool {
        let Some(moved) = ctx.moved_var() else {
            return false;
        };
        if moved.use_after_move.is_none() {
            return false;
        }
        let Some(site) = moved.move_site.as_ref() else {
            return false;
        };
        let Some(line_text) = ctx.line(&site.file, site.line) else {
            return false;
        };
        let Some(byte) = find_token_byte(line_text, &moved.name) else {
            return false;
        };
        if byte > 0 && line_text[..byte].ends_with('&') {
            return false;
        }
        let after = line_text[byte + moved.name.len()..].trim_start();
        if after.starts_with(".clone()") {
            return false;
        }
        ctx.is_call_argument(&moved.name, &site.file, site.line)
    }

    fn generate(&self, ctx: &RepairContext) -> Vec<RepairCandidate> {
        let Some(moved) = ctx.moved_var() else {
            return Vec::new();
        };
        let Some(site) = moved.move_site.as_ref() else {
            return Vec::new();
        };
        let Some((file, line, col, _)) = var_columns(ctx, &moved.name, site) else {
            return Vec::new();
        };

        vec![RepairCandidate {
            kind: RepairKind::Borrow,
            confidence: Confidence::Medium,
            description: format!("Borrow `{}` instead of moving it.", moved.name),
            evidence: vec![
                format!("`{}` is moved at {}:{}", moved.name, file, line),
                if let Some(use_site) = &moved.use_after_move {
                    format!(
                        "`{}` is used again later at {}:{}",
                        moved.name, use_site.file, use_site.line
                    )
                } else {
                    "the value is used again after the move".to_string()
                },
                format!("Insert `&` before `{}` at {}:{}", moved.name, file, line),
                "Requires the called function to accept a reference".to_string(),
            ],
            patch: Patch::new(vec![Edit {
                file,
                line,
                start_col: col,
                end_col: col,
                replacement: "&".to_string(),
                label: Some(format!("borrow `{}` instead of move", moved.name)),
                anchor: None,
            }]),
        }]
    }
}

/// 4. Clone the value before moving it.
///
/// Evidence: a value is reported as moved at a call argument and used again
/// afterwards; the move site does not already borrow or clone. Only meaningful
/// when cloning is possible — always must be confirmed by verification.
pub struct CloneTransform;

impl Transform for CloneTransform {
    fn name(&self) -> &'static str {
        "clone"
    }

    fn applicable(&self, ctx: &RepairContext) -> bool {
        let Some(moved) = ctx.moved_var() else {
            return false;
        };
        if moved.use_after_move.is_none() {
            return false;
        }
        let Some(site) = moved.move_site.as_ref() else {
            return false;
        };
        let Some(line_text) = ctx.line(&site.file, site.line) else {
            return false;
        };
        if let Some(byte) = find_token_byte(line_text, &moved.name) {
            let after = line_text[byte + moved.name.len()..].trim_start();
            if after.starts_with(".clone()") {
                return false;
            }
            if after.starts_with("(&") || after.starts_with("(& ") {
                return false;
            }
            if byte > 0 && line_text[..byte].ends_with('&') {
                return false;
            }
        }
        ctx.is_call_argument(&moved.name, &site.file, site.line)
    }

    fn generate(&self, ctx: &RepairContext) -> Vec<RepairCandidate> {
        let Some(moved) = ctx.moved_var() else {
            return Vec::new();
        };
        let Some(site) = moved.move_site.as_ref() else {
            return Vec::new();
        };
        let Some((file, line, _, col_end)) = var_columns(ctx, &moved.name, site) else {
            return Vec::new();
        };

        vec![RepairCandidate {
            kind: RepairKind::Clone,
            confidence: Confidence::Medium,
            description: format!("Clone `{}` to avoid moving it.", moved.name),
            evidence: vec![
                format!("`{}` is moved at {}:{}", moved.name, file, line),
                if let Some(use_site) = &moved.use_after_move {
                    format!(
                        "`{}` is used again later at {}:{}",
                        moved.name, use_site.file, use_site.line
                    )
                } else {
                    "the value is used again after the move".to_string()
                },
                format!("Insert `.clone()` at {}:{} column {}", file, line, col_end),
                "Requires the value's type to implement `Clone`".to_string(),
            ],
            patch: Patch::new(vec![Edit {
                file,
                line,
                start_col: col_end,
                end_col: col_end,
                replacement: ".clone()".to_string(),
                label: Some(format!("clone `{}` to avoid move", moved.name)),
                anchor: None,
            }]),
        }]
    }
}

fn var_columns(
    ctx: &RepairContext,
    var: &str,
    site: &LocatedSpan,
) -> Option<(String, u32, u32, u32)> {
    let line_text = ctx.line(&site.file, site.line)?;
    let byte = find_token_byte(line_text, var)?;
    let col_start = byte_to_char_column(line_text, byte);
    let col_end = byte_to_char_column(line_text, byte + var.len());
    Some((site.file.clone(), site.line, col_start, col_end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{CompilerSuggestion, Span, SpanText};

    fn make_error(
        message: &str,
        spans: Vec<(u32, bool, &str)>,
        suggestions: Vec<CompilerSuggestion>,
    ) -> ParsedError {
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
            suggestions,
            ..Default::default()
        }
    }

    const MOVED_SOURCE: &str = "fn main() {\n    let value = String::from(\"hello\");\n    drop(value);\n    println!(\"{}\", value);\n}\n";

    fn moved_error() -> ParsedError {
        make_error(
            "borrow of moved value: `value`",
            vec![
                (3, false, "value moved here"),
                (4, true, "value borrowed here after move"),
            ],
            vec![],
        )
    }

    #[test]
    fn clone_transform_requires_move_evidence_not_error_code() {
        let error = moved_error();
        let source = MOVED_SOURCE;
        assert_eq!(error.code, "", "evidence must not depend on the error code");
        let analysis = crate::analyzer::analyze(&error);
        let ctx = RepairContext::from_source(&error, &analysis, source);
        assert!(CloneTransform.applicable(&ctx));
        let candidates = CloneTransform.generate(&ctx);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].kind, RepairKind::Clone);
        assert_eq!(candidates[0].patch.edits[0].replacement, ".clone()");
    }

    #[test]
    fn borrow_transform_requires_move_evidence_not_error_code() {
        let error = moved_error();
        let analysis = crate::analyzer::analyze(&error);
        let ctx = RepairContext::from_source(&error, &analysis, MOVED_SOURCE);
        assert!(BorrowTransform.applicable(&ctx));
        let candidates = BorrowTransform.generate(&ctx);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].kind, RepairKind::Borrow);
        assert_eq!(candidates[0].patch.edits[0].replacement, "&");
    }

    #[test]
    fn transforms_not_applicable_without_move_evidence() {
        let source = "fn main() {\n    let number: i32 = \"hello\";\n}\n";
        let error = make_error(
            "mismatched types",
            vec![(2, true, "expected `i32`, found `&str`")],
            vec![],
        );
        let analysis = crate::analyzer::analyze(&error);
        let ctx = RepairContext::from_source(&error, &analysis, source);
        assert!(!CloneTransform.applicable(&ctx));
        assert!(!BorrowTransform.applicable(&ctx));
        assert_eq!(
            generate_candidates(&error, &analysis, source).len(),
            0,
            "a plain type mismatch must not produce fabrication"
        );
    }

    #[test]
    fn mutability_transform_generates_patch() {
        let source = "fn main() {\n    let x = 10;\n    x += 5;\n}\n";
        let error = make_error(
            "cannot assign twice to immutable variable `x`",
            vec![(3, true, "cannot assign")],
            vec![],
        );
        let analysis = crate::analyzer::analyze(&error);
        let ctx = RepairContext::from_source(&error, &analysis, source);
        assert!(MutabilityTransform.applicable(&ctx));
        let candidates = MutabilityTransform.generate(&ctx);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].kind, RepairKind::Mutability);
        let edit = &candidates[0].patch.edits[0];
        assert_eq!(edit.line, 2);
        assert_eq!(edit.replacement, "mut ");
    }

    #[test]
    fn mutability_transform_not_applicable_when_binding_is_mut() {
        let source = "fn main() {\n    let mut x = 10;\n    x += 5;\n}\n";
        let error = make_error(
            "cannot assign twice to immutable variable `x`",
            vec![(3, true, "cannot assign")],
            vec![],
        );
        let analysis = crate::analyzer::analyze(&error);
        let ctx = RepairContext::from_source(&error, &analysis, source);
        assert!(!MutabilityTransform.applicable(&ctx));
    }

    #[test]
    fn mutability_transform_skips_when_compiler_suggests_mut() {
        let source = "fn main() {\n    let x = 10;\n    x += 5;\n}\n";
        let error = make_error(
            "cannot assign twice to immutable variable `x`",
            vec![(3, true, "cannot assign")],
            vec![CompilerSuggestion {
                file: "src/main.rs".to_string(),
                line: 2,
                column: 9,
                column_end: 9,
                replacement: "mut ".to_string(),
                applicability: "MachineApplicable".to_string(),
                label: Some("make mutable".to_string()),
            }],
        );
        let analysis = crate::analyzer::analyze(&error);
        let candidates = generate_candidates(&error, &analysis, source);
        assert_eq!(
            candidates.len(),
            1,
            "duplicate mut candidate must be suppressed"
        );
        assert_eq!(candidates[0].kind, RepairKind::CompilerSuggested);
    }

    #[test]
    fn compiler_suggestion_transform_scores_applicability() {
        let error = make_error(
            "cannot assign",
            vec![],
            vec![CompilerSuggestion {
                file: "src/main.rs".to_string(),
                line: 1,
                column: 5,
                column_end: 6,
                replacement: "&mut ".to_string(),
                applicability: "MaybeIncorrect".to_string(),
                label: None,
            }],
        );
        let analysis = crate::analyzer::analyze(&error);
        let ctx = RepairContext::from_source(&error, &analysis, "");
        assert!(CompilerSuggestionTransform.applicable(&ctx));
        let candidates = CompilerSuggestionTransform.generate(&ctx);
        assert_eq!(candidates[0].confidence, Confidence::Low);
    }

    #[test]
    fn generate_candidates_deduplicates_overlapping_transforms() {
        let error = moved_error();
        let analysis = crate::analyzer::analyze(&error);
        let candidates = generate_candidates(&error, &analysis, MOVED_SOURCE);
        let signatures: std::collections::BTreeSet<String> = candidates
            .iter()
            .map(|c| crate::repair::patch_signature(&c.patch))
            .collect();
        assert_eq!(
            signatures.len(),
            candidates.len(),
            "no two candidates may share a patch signature"
        );
    }
}
