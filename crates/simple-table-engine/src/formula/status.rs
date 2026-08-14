#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FormulaIssueKind {
    InvalidFormula,
    VolatileFormula,
    UnsupportedDependency,
    LargeRangeDependency,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FormulaIssue {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub kind: FormulaIssueKind,
    pub message: String,
}

impl FormulaIssue {
    pub(crate) fn new(
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FormulaDiagnostics {
    pub invalid_formula_count: usize,
    pub volatile_formula_count: usize,
    pub unsupported_dependency_count: usize,
    pub large_range_dependency_count: usize,
    pub skipped_reference_rewrite_count: usize,
    pub issues: Vec<FormulaIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FormulaStatus {
    Ready {
        diagnostics: FormulaDiagnostics,
    },
    Degraded {
        message: String,
        diagnostics: FormulaDiagnostics,
    },
}

impl FormulaStatus {
    pub(crate) fn ready(diagnostics: FormulaDiagnostics) -> Self {
        Self::Ready { diagnostics }
    }

    pub(crate) fn degraded(message: String, diagnostics: FormulaDiagnostics) -> Self {
        Self::Degraded {
            message,
            diagnostics,
        }
    }
}
