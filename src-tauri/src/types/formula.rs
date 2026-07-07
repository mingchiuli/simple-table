use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FormulaDiagnostics {
    pub invalid_formula_count: usize,
    pub volatile_formula_count: usize,
    pub unsupported_dependency_count: usize,
    pub large_range_dependency_count: usize,
    pub skipped_reference_rewrite_count: usize,
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
}
