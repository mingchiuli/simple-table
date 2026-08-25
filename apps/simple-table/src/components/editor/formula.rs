use std::rc::Rc;

use dioxus::prelude::*;
use simple_table_components::icons::Sigma;
use simple_table_components::{
    ContentAlign, ContentSide, Input, PopoverContent, PopoverRoot, PopoverTrigger,
};

use super::super::grid::column_label;
use super::blocked_tooltip;
use crate::actions;
use crate::model::{
    AppPorts, EditorStore, FormulaIssueKindView, FormulaIssueView, FormulaStatusView,
};

#[component]
pub(super) fn FormulaBar() -> Element {
    let mut store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let document = store.document.read();
    let Some(document) = document.as_ref() else {
        return rsx! {};
    };
    let sheet_index = store
        .active_sheet()
        .min(document.document.sheets.len().saturating_sub(1));
    let sheet_capabilities = document
        .editor_session
        .capabilities
        .sheets
        .get(sheet_index)
        .cloned()
        .unwrap_or_default();
    let selection = *store.selection.read();
    let selected = (selection.row, selection.col);
    let selected_address = selection.merge.map_or_else(
        || format!("{}{}", column_label(selection.col), selection.row + 1),
        |merge| {
            format!(
                "{}{}:{}{}",
                column_label(merge.start_col),
                merge.start_row + 1,
                column_label(merge.end_col),
                merge.end_row + 1
            )
        },
    );

    rsx! {
        div { class: "formula-bar",
            span { class: "cell-address", title: selected_address, "{selected_address}" }
            span { class: "formula-symbol", "fx" }
            Input {
                aria_label: "Cell value or formula",
                title: blocked_tooltip(
                    sheet_capabilities.can_edit_cells,
                    &sheet_capabilities.blocked_edit_reasons,
                    "Cell editing is unavailable for this sheet",
                ),
                disabled: store.busy() || !sheet_capabilities.can_edit_cells,
                value: store.formula_text,
                oninput: {
                    let ports = Rc::clone(&ports);
                    move |event: Event<FormData>| {
                        let text = event.value();
                        store.formula_text.set(text.clone());
                        actions::queue_cell_edit(
                            store,
                            Rc::clone(&ports),
                            sheet_index,
                            selected.0,
                            selected.1,
                            text,
                        );
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
pub(super) struct FormulaStatusPopoverProps {
    pub status: FormulaStatusView,
    pub sheet_names: Vec<String>,
    pub active_sheet: usize,
}

#[component]
pub(super) fn FormulaStatusPopover(props: FormulaStatusPopoverProps) -> Element {
    let diagnostics = props.status.diagnostics();
    let total = diagnostics.total_count();
    let invalid_count = diagnostics.invalid_formula_count;
    let volatile_count = diagnostics.volatile_formula_count;
    let unsupported_count = diagnostics.unsupported_dependency_count;
    let large_range_count = diagnostics.large_range_dependency_count;
    let skipped_rewrite_count = diagnostics.skipped_reference_rewrite_count;
    let degraded_message = props.status.degraded_message().map(str::to_string);
    let samples = props.status.sample_issues(props.active_sheet, 5);
    let (state_class, state_label, trigger_label) = if degraded_message.is_some() {
        (
            "degraded",
            "Degraded",
            format!("Formula calculation degraded, {total} diagnostics"),
        )
    } else if total > 0 {
        (
            "warning",
            "Warnings",
            format!("Formula calculation has {total} diagnostics"),
        )
    } else {
        ("ready", "Ready", "Formula calculation ready".to_string())
    };
    let trigger_class = format!("formula-status-trigger {state_class}");

    rsx! {
        PopoverRoot { class: "formula-status", is_modal: false,
            PopoverTrigger {
                class: trigger_class,
                title: trigger_label.clone(),
                aria_label: trigger_label,
                Sigma { size: 15 }
                if total > 0 {
                    span { class: "formula-status-count", "{total}" }
                }
            }
            PopoverContent {
                class: "formula-status-popover",
                side: ContentSide::Top,
                align: ContentAlign::End,
                div { class: "formula-status-heading",
                    h2 { "Formula status" }
                    span { class: "formula-state {state_class}", "{state_label}" }
                }
                if let Some(message) = degraded_message {
                    p { class: "formula-degraded-message", "{message}" }
                } else if total == 0 {
                    p { class: "formula-ready-message", "Formulas are calculating normally." }
                }
                dl { class: "formula-diagnostic-counts",
                    FormulaDiagnosticCount { label: "Invalid", value: invalid_count }
                    FormulaDiagnosticCount { label: "Volatile", value: volatile_count }
                    FormulaDiagnosticCount { label: "Dependencies", value: unsupported_count }
                    FormulaDiagnosticCount { label: "Large ranges", value: large_range_count }
                    FormulaDiagnosticCount { label: "Skipped rewrites", value: skipped_rewrite_count }
                }
                if !samples.is_empty() {
                    div { class: "formula-issues",
                        h3 { "Examples" }
                        ul {
                            for (index, issue) in samples.iter().enumerate() {
                                li { key: "{issue.sheet_index}-{issue.row}-{issue.col}-{index}",
                                    div { class: "formula-issue-meta",
                                        strong { "{formula_issue_location(issue, &props.sheet_names)}" }
                                        span { "{formula_issue_kind_label(issue.kind)}" }
                                    }
                                    p { "{issue.message}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct FormulaDiagnosticCountProps {
    label: &'static str,
    value: usize,
}

#[component]
fn FormulaDiagnosticCount(props: FormulaDiagnosticCountProps) -> Element {
    rsx! {
        div {
            dt { "{props.label}" }
            dd { "{props.value}" }
        }
    }
}

fn formula_issue_location(issue: &FormulaIssueView, sheet_names: &[String]) -> String {
    let sheet_name = sheet_names
        .get(issue.sheet_index)
        .cloned()
        .unwrap_or_else(|| format!("Sheet {}", issue.sheet_index + 1));
    format!("{sheet_name}!{}{}", column_label(issue.col), issue.row + 1)
}

fn formula_issue_kind_label(kind: FormulaIssueKindView) -> &'static str {
    match kind {
        FormulaIssueKindView::InvalidFormula => "Invalid formula",
        FormulaIssueKindView::VolatileFormula => "Volatile formula",
        FormulaIssueKindView::UnsupportedDependency => "Unsupported dependency",
        FormulaIssueKindView::LargeRangeDependency => "Large range",
    }
}
