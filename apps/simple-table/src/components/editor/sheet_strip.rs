use std::rc::Rc;

use dioxus::prelude::*;
use simple_table_components::icons::{Plus, Trash2};
use simple_table_components::{
    Button, ButtonSize, ButtonVariant, TabList, TabTrigger, Tabs, TabsVariant,
};

use super::blocked_tooltip;
use super::formula::FormulaStatusPopover;
use crate::actions;
use crate::model::{AppPorts, EditorStore};

#[component]
pub(super) fn SheetStrip() -> Element {
    let store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let document = store.document.read();
    let Some(document) = document.as_ref() else {
        return rsx! {};
    };
    let sheet_index = store
        .active_sheet()
        .min(document.document.sheets.len().saturating_sub(1));
    let sheet_count = document.document.sheets.len();
    let capabilities = &document.editor_session.capabilities;
    let sheet_structure_tooltip = blocked_tooltip(
        capabilities.structure.can_insert_delete_sheets,
        &capabilities.structure.blocked_sheet_structure_reasons,
        "Sheet structure changes are unavailable for this workbook",
    );
    let sheet_names = document
        .document
        .sheets
        .iter()
        .map(|sheet| sheet.name.clone())
        .collect::<Vec<_>>();
    let formula_status = document.editor_session.formula_status.clone();

    rsx! {
        div { class: "sheet-strip",
            Tabs {
                class: "sheet-tabs",
                variant: TabsVariant::Ghost,
                value: Some(document.document.sheets[sheet_index].name.clone()),
                default_value: document.document.sheets[sheet_index].name.clone(),
                horizontal: true,
                on_value_change: {
                    let sheet_names = sheet_names.clone();
                    let ports = Rc::clone(&ports);
                    move |name: String| {
                        if let Some(index) = sheet_names.iter().position(|sheet| sheet == &name) {
                            let ports = Rc::clone(&ports);
                            spawn(async move { actions::select_sheet(store, ports, index).await });
                        }
                    }
                },
                TabList { class: "sheet-tab-list",
                    for (index, sheet) in document.document.sheets.iter().enumerate() {
                        TabTrigger {
                            key: "{sheet.name}-{index}",
                            class: "sheet-tab",
                            value: sheet.name.clone(),
                            index,
                            "{sheet.name}"
                        }
                    }
                }
            }
            Button {
                class: "icon-button add-sheet",
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                aria_label: "Add sheet",
                title: sheet_structure_tooltip.clone(),
                disabled: store.busy() || !capabilities.structure.can_insert_delete_sheets,
                onclick: {
                    let ports = Rc::clone(&ports);
                    move |_| {
                        let ports = Rc::clone(&ports);
                        spawn(async move {
                            actions::run_mutation(store, ports, actions::MutationIntent::AddSheet)
                                .await;
                        });
                    }
                },
                Plus { size: 17 }
            }
            Button {
                class: "icon-button",
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconSm,
                aria_label: "Delete current sheet",
                title: sheet_structure_tooltip,
                disabled: store.busy()
                    || sheet_count <= 1
                    || !capabilities.structure.can_insert_delete_sheets,
                onclick: {
                    let ports = Rc::clone(&ports);
                    move |_| {
                        let ports = Rc::clone(&ports);
                        spawn(async move {
                            actions::run_mutation(
                                store,
                                ports,
                                actions::MutationIntent::DeleteSheet { sheet_index },
                            )
                            .await;
                        });
                    }
                },
                Trash2 { size: 16 }
            }
            div { class: "sheet-strip-spacer" }
            FormulaStatusPopover {
                status: formula_status,
                sheet_names,
                active_sheet: sheet_index,
            }
            span { class: "status-text", "{store.status}" }
        }
    }
}
