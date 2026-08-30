use crate::diagnostics::ParsedError;

pub enum FixKind {
    AutoInsertClone,
    RequiresHumanJudgment,
}

pub struct FixSuggestion {
    pub kind: FixKind,
    pub description: String,
}

pub fn suggest_fix(err: &ParsedError) -> FixSuggestion {
    if err.code == "E0382"
        && err.suggested_replacement.as_deref() == Some(".clone()")
        && err.suggestion_applicability.as_deref() == Some("MachineApplicable")
    {
        return FixSuggestion {
            kind: FixKind::AutoInsertClone,
            description: "The compiler says `.clone()` is safe to insert here.".to_string(),
        };
    }

    FixSuggestion {
        kind: FixKind::RequiresHumanJudgment,
        description: "This error needs a developer to choose the correct fix.".to_string(),
    }
}