use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum FormulaIssueKind {
    InvalidFormula,
    VolatileFormula,
    UnsupportedDependency,
    LargeRangeDependency,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FormulaIssue {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub kind: FormulaIssueKind,
    pub message: String,
}

impl FormulaIssue {
    pub fn new(
        sheet_index: usize,
        row: usize,
        col: usize,
        kind: FormulaIssueKind,
        message: impl Into<String>,
    ) -> Self {
        Self {
            sheet_index,
            row,
            col,
            kind,
            message: message.into(),
        }
    }
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FormulaDiagnostics {
    pub invalid_formula_count: usize,
    pub volatile_formula_count: usize,
    pub unsupported_dependency_count: usize,
    pub large_range_dependency_count: usize,
    pub skipped_reference_rewrite_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<FormulaIssue>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(tag = "state", rename_all = "camelCase")]
pub enum FormulaStatus {
    Ready {
        #[serde(default)]
        diagnostics: FormulaDiagnostics,
    },
    Degraded {
        message: String,
        #[serde(default)]
        diagnostics: FormulaDiagnostics,
    },
}

impl FormulaStatus {
    pub fn ready(diagnostics: FormulaDiagnostics) -> Self {
        Self::Ready { diagnostics }
    }

    pub fn degraded(message: String, diagnostics: FormulaDiagnostics) -> Self {
        Self::Degraded {
            message,
            diagnostics,
        }
    }

    pub fn bounded(mut self, maximum_issues: usize) -> Self {
        let diagnostics = match &mut self {
            Self::Ready { diagnostics } | Self::Degraded { diagnostics, .. } => diagnostics,
        };
        diagnostics.issues.truncate(maximum_issues);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_status_preserves_counts_and_limits_samples() {
        let diagnostics = FormulaDiagnostics {
            invalid_formula_count: 20,
            issues: (0..20)
                .map(|row| {
                    FormulaIssue::new(0, row, 0, FormulaIssueKind::InvalidFormula, "invalid")
                })
                .collect(),
            ..Default::default()
        };

        let FormulaStatus::Ready { diagnostics } = FormulaStatus::ready(diagnostics).bounded(3)
        else {
            panic!("ready status");
        };
        assert_eq!(diagnostics.invalid_formula_count, 20);
        assert_eq!(diagnostics.issues.len(), 3);
    }
}
