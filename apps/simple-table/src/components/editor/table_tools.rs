use std::rc::Rc;

use dioxus::prelude::*;
use simple_table_components::icons::{ArrowDownAZ, ArrowDownZA, Funnel};
use simple_table_components::{
    Button, ButtonVariant, ContentAlign, ContentSide, Input, Label, PopoverContent, PopoverRoot,
    PopoverTrigger,
};

use super::super::grid::column_label;
use crate::actions;
use crate::model::{AppPorts, EditorStore, FilterConditionView, FilterOperatorView};
use crate::protocol::{FilterOperatorDto, SortDirectionDto};

#[derive(Props, Clone, PartialEq)]
pub(crate) struct TableDataToolsProps {
    document_id: u64,
    sheet_index: usize,
    selected: (usize, usize),
    #[props(default)]
    compact: bool,
}

#[component]
pub(crate) fn TableDataTools(props: TableDataToolsProps) -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let (active_condition, has_filters, can_edit_cells, blocked_reason) = {
        let document = store.document.read();
        let filter = document
            .as_ref()
            .and_then(|document| {
                document
                    .editor_session
                    .filters
                    .iter()
                    .find(|filter| filter.sheet_index == props.sheet_index)
            })
            .cloned();
        let condition = filter.as_ref().and_then(|filter| {
            filter
                .conditions
                .iter()
                .find(|condition| condition.col == props.selected.1)
                .cloned()
        });
        let capabilities = document
            .as_ref()
            .and_then(|document| {
                document
                    .editor_session
                    .capabilities
                    .sheets
                    .get(props.sheet_index)
            })
            .cloned()
            .unwrap_or_default();
        let blocked_reason = capabilities
            .blocked_edit_reasons
            .first()
            .cloned()
            .unwrap_or_else(|| "Table operations are unavailable for this sheet".to_string());
        (
            condition,
            filter.is_some(),
            capabilities.can_edit_cells,
            blocked_reason,
        )
    };
    let active = active_condition.is_some();
    let draft_key = filter_draft_key(
        props.document_id,
        props.sheet_index,
        props.selected.1,
        active_condition.as_ref(),
    );
    let root_class = if props.compact {
        "table-data-tools column-data-tools"
    } else {
        "table-data-tools"
    };
    let trigger_class = match (props.compact, active) {
        (true, true) => "column-filter-trigger active",
        (true, false) => "column-filter-trigger",
        (false, true) => "tool-button active",
        (false, false) => "tool-button",
    };
    let apply_filter = Callback::new({
        let ports = Rc::clone(&ports);
        move |(operator, value)| {
            let ports = Rc::clone(&ports);
            spawn(async move {
                actions::run_mutation(
                    store,
                    ports,
                    actions::MutationIntent::SetFilter {
                        sheet_index: props.sheet_index,
                        anchor_row: props.selected.0,
                        col: props.selected.1,
                        operator,
                        value,
                    },
                )
                .await;
            });
        }
    });
    let clear_filter = Callback::new({
        let ports = Rc::clone(&ports);
        move |_| {
            let ports = Rc::clone(&ports);
            spawn(async move {
                actions::run_mutation(
                    store,
                    ports,
                    actions::MutationIntent::ClearFilter {
                        sheet_index: props.sheet_index,
                        col: Some(props.selected.1),
                    },
                )
                .await;
            });
        }
    });
    let clear_filters = Callback::new({
        let ports = Rc::clone(&ports);
        move |_| {
            let ports = Rc::clone(&ports);
            spawn(async move {
                actions::run_mutation(
                    store,
                    ports,
                    actions::MutationIntent::ClearFilter {
                        sheet_index: props.sheet_index,
                        col: None,
                    },
                )
                .await;
            });
        }
    });

    rsx! {
        PopoverRoot { class: root_class, is_modal: false,
            PopoverTrigger {
                class: trigger_class,
                title: if can_edit_cells {
                    "Sort and filter selected column"
                } else {
                    blocked_reason.as_str()
                },
                aria_label: "Sort and filter selected column",
                aria_disabled: (!can_edit_cells).then_some("true"),
                Funnel { size: 18 }
            }
            PopoverContent {
                class: "table-data-popover",
                side: ContentSide::Bottom,
                align: ContentAlign::Start,
                div { class: "table-data-heading",
                    h2 { "Column {column_label(props.selected.1)}" }
                }
                div { class: "table-sort-actions",
                    Button {
                        variant: ButtonVariant::Ghost,
                        disabled: store.busy() || !can_edit_cells,
                        title: (!can_edit_cells).then_some(blocked_reason.as_str()),
                        onclick: {
                            let ports = Rc::clone(&ports);
                            move |_| {
                                let ports = Rc::clone(&ports);
                                spawn(async move {
                                    actions::run_mutation(
                                        store,
                                        ports,
                                        actions::MutationIntent::SortRows {
                                            sheet_index: props.sheet_index,
                                            anchor_row: props.selected.0,
                                            anchor_col: props.selected.1,
                                            direction: SortDirectionDto::Ascending,
                                        },
                                    ).await;
                                });
                            }
                        },
                        ArrowDownAZ { size: 17 }
                        "Sort ascending"
                    }
                    Button {
                        variant: ButtonVariant::Ghost,
                        disabled: store.busy() || !can_edit_cells,
                        title: (!can_edit_cells).then_some(blocked_reason.as_str()),
                        onclick: {
                            let ports = Rc::clone(&ports);
                            move |_| {
                                let ports = Rc::clone(&ports);
                                spawn(async move {
                                    actions::run_mutation(
                                        store,
                                        ports,
                                        actions::MutationIntent::SortRows {
                                            sheet_index: props.sheet_index,
                                            anchor_row: props.selected.0,
                                            anchor_col: props.selected.1,
                                            direction: SortDirectionDto::Descending,
                                        },
                                    ).await;
                                });
                            }
                        },
                        ArrowDownZA { size: 17 }
                        "Sort descending"
                    }
                }
                for (draft_key, active_condition) in [(draft_key, active_condition)] {
                    TableFilterForm {
                        key: "{draft_key}",
                        active_condition,
                        has_filters,
                        enabled: can_edit_cells,
                        blocked_reason: blocked_reason.clone(),
                        on_apply: apply_filter,
                        on_clear: clear_filter,
                        on_clear_all: clear_filters,
                    }
                }
            }
        }
    }
}

#[derive(Props, Clone, PartialEq)]
struct TableFilterFormProps {
    active_condition: Option<FilterConditionView>,
    has_filters: bool,
    enabled: bool,
    blocked_reason: String,
    on_apply: Callback<(FilterOperatorDto, String)>,
    on_clear: Callback<()>,
    on_clear_all: Callback<()>,
}

#[component]
fn TableFilterForm(props: TableFilterFormProps) -> Element {
    let store = use_context::<EditorStore>();
    let initial_value = props
        .active_condition
        .as_ref()
        .map(|condition| condition.value.clone())
        .unwrap_or_default();
    let initial_operator = props
        .active_condition
        .as_ref()
        .map_or("contains", |condition| {
            filter_operator_value(condition.operator)
        })
        .to_string();
    let mut filter_value = use_signal(move || initial_value);
    let mut operator = use_signal(move || initial_operator);
    let active = props.active_condition.is_some();

    rsx! {
        div { class: "table-filter-form",
            Label { html_for: "table-filter-operator", "Filter" }
            select {
                id: "table-filter-operator",
                value: operator,
                disabled: !props.enabled,
                title: (!props.enabled).then_some(props.blocked_reason.as_str()),
                onchange: move |event| operator.set(event.value()),
                option { value: "contains", "Contains" }
                option { value: "equals", "Equals" }
                option { value: "not-equals", "Does not equal" }
                option { value: "blank", "Is blank" }
                option { value: "not-blank", "Is not blank" }
            }
            if !matches!(operator().as_str(), "blank" | "not-blank") {
                Input {
                    aria_label: "Filter value",
                    placeholder: "Value",
                    value: filter_value,
                    disabled: !props.enabled,
                    title: (!props.enabled).then_some(props.blocked_reason.as_str()),
                    oninput: move |event: Event<FormData>| filter_value.set(event.value()),
                }
            }
            div { class: "table-filter-actions",
                Button {
                    disabled: store.busy() || !props.enabled,
                    title: (!props.enabled).then_some(props.blocked_reason.as_str()),
                    onclick: move |_| {
                        let operator = match operator().as_str() {
                            "equals" => FilterOperatorDto::Equals,
                            "not-equals" => FilterOperatorDto::NotEquals,
                            "blank" => FilterOperatorDto::Blank,
                            "not-blank" => FilterOperatorDto::NotBlank,
                            _ => FilterOperatorDto::Contains,
                        };
                        props.on_apply.call((operator, filter_value()));
                    },
                    "Apply"
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    disabled: store.busy() || !active || !props.enabled,
                    title: (!props.enabled).then_some(props.blocked_reason.as_str()),
                    onclick: move |_| props.on_clear.call(()),
                    "Clear column"
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    disabled: store.busy() || !props.has_filters || !props.enabled,
                    title: (!props.enabled).then_some(props.blocked_reason.as_str()),
                    onclick: move |_| props.on_clear_all.call(()),
                    "Clear all"
                }
            }
        }
    }
}

fn filter_draft_key(
    document_id: u64,
    sheet_index: usize,
    col: usize,
    condition: Option<&FilterConditionView>,
) -> String {
    match condition {
        Some(condition) => format!(
            "{document_id}:{sheet_index}:{col}:{}:{}",
            filter_operator_value(condition.operator),
            condition.value
        ),
        None => format!("{document_id}:{sheet_index}:{col}:none"),
    }
}

fn filter_operator_value(operator: FilterOperatorView) -> &'static str {
    match operator {
        FilterOperatorView::Equals => "equals",
        FilterOperatorView::NotEquals => "not-equals",
        FilterOperatorView::Contains => "contains",
        FilterOperatorView::Blank => "blank",
        FilterOperatorView::NotBlank => "not-blank",
    }
}

#[derive(Props, Clone, PartialEq)]
pub(super) struct DimensionControlsProps {
    sheet_index: usize,
    selected: (usize, usize),
    width: u32,
    height: u32,
    enabled: bool,
    blocked_reason: Option<String>,
}

#[component]
pub(super) fn DimensionControls(props: DimensionControlsProps) -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    rsx! {
        div { class: "dimension-controls",
            Label {
                html_for: "selected-column-width",
                title: props.blocked_reason.as_deref().unwrap_or("Selected column width"),
                span { "W" }
                Input {
                    id: "selected-column-width",
                    r#type: "number",
                    min: 24,
                    max: 600,
                    value: props.width,
                    aria_label: "Selected column width",
                    disabled: store.busy() || !props.enabled,
                    onchange: {
                        let ports = Rc::clone(&ports);
                        move |event: Event<FormData>| {
                            let Ok(width) = event.value().parse::<u32>() else { return; };
                            let ports = Rc::clone(&ports);
                            spawn(async move {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    actions::MutationIntent::SetColumnWidth {
                                        sheet_index: props.sheet_index,
                                        col_index: props.selected.1,
                                        width: Some(width.clamp(24, 600)),
                                    },
                                )
                                .await;
                            });
                        }
                    }
                }
            }
            Label {
                html_for: "selected-row-height",
                title: props.blocked_reason.as_deref().unwrap_or("Selected row height"),
                span { "H" }
                Input {
                    id: "selected-row-height",
                    r#type: "number",
                    min: 18,
                    max: 300,
                    value: props.height,
                    aria_label: "Selected row height",
                    disabled: store.busy() || !props.enabled,
                    onchange: {
                        let ports = Rc::clone(&ports);
                        move |event: Event<FormData>| {
                            let Ok(height) = event.value().parse::<u32>() else { return; };
                            let ports = Rc::clone(&ports);
                            spawn(async move {
                                actions::run_mutation(
                                    store,
                                    ports,
                                    actions::MutationIntent::SetRowHeight {
                                        sheet_index: props.sheet_index,
                                        row_index: props.selected.0,
                                        height: Some(height.clamp(18, 300)),
                                    },
                                )
                                .await;
                            });
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_draft_key_uses_authoritative_condition_identity() {
        let condition = FilterConditionView {
            col: 2,
            operator: FilterOperatorView::Contains,
            value: "north".to_string(),
        };

        let original = filter_draft_key(7, 1, 2, Some(&condition));
        assert_eq!(original, filter_draft_key(7, 1, 2, Some(&condition)));
        assert_ne!(original, filter_draft_key(7, 1, 3, None));

        let changed = FilterConditionView {
            value: "south".to_string(),
            ..condition
        };
        assert_ne!(original, filter_draft_key(7, 1, 2, Some(&changed)));
    }
}
