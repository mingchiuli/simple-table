use std::rc::Rc;

use crate::ui::icons::{Search, X};
use dioxus::prelude::*;
use dioxus_primitives::switch::{Switch, SwitchThumb};

use crate::actions;
use crate::model::{AppPorts, EditorStore};

#[component]
pub fn SearchPanel() -> Element {
    let mut store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let mut query = use_signal(String::new);
    let mut all_sheets = use_signal(|| true);
    let response = store.search.read().clone();

    if !store.search_open() {
        return rsx! {};
    }

    rsx! {
        aside { class: "search-panel", aria_label: "Search workbook",
            header {
                h2 { "Find" }
                button {
                    class: "icon-button",
                    title: "Close search",
                    aria_label: "Close search",
                    onclick: move |_| store.search_open.set(false),
                    X { size: 17 }
                }
            }
            form {
                class: "search-form",
                onsubmit: {
                    let ports = Rc::clone(&ports);
                    move |event| {
                        event.prevent_default();
                        let ports = Rc::clone(&ports);
                        let query = query();
                        spawn(async move { actions::search(store, ports, query, all_sheets()).await });
                    }
                },
                div { class: "search-input-wrap",
                    Search { size: 17 }
                    input {
                        value: query,
                        placeholder: "Search cells",
                        aria_label: "Search cells",
                        oninput: move |event| query.set(event.value()),
                    }
                }
                div { class: "search-scope",
                    span { "All sheets" }
                    Switch {
                        class: "switch-control",
                        checked: Some(all_sheets()),
                        on_checked_change: move |checked| all_sheets.set(checked),
                        aria_label: "Search all sheets",
                        SwitchThumb { class: "switch-thumb" }
                    }
                }
                button { class: "search-submit", r#type: "submit", "Search" }
            }

            div { class: "search-results",
                if let Some(response) = response {
                    div { class: "result-count",
                        "{response.results.len()} results"
                        if response.truncated { span { " (limited)" } }
                    }
                    for result in response.results {
                        button {
                            class: "search-result",
                            onclick: {
                                let ports = Rc::clone(&ports);
                                move |_| {
                                    let ports = Rc::clone(&ports);
                                    spawn(async move {
                                        actions::select_sheet(store, Rc::clone(&ports), result.sheet_index).await;
                                        actions::refresh_region(
                                            store,
                                            ports,
                                            result.row.saturating_sub(6),
                                            result.row.saturating_add(24),
                                            result.col.saturating_sub(4),
                                            result.col.saturating_add(12),
                                        )
                                        .await;
                                        store.selected_cell.set((result.row, result.col));
                                    });
                                }
                            },
                            span { class: "result-location", "{result.sheet_name} · {result.cell_position}" }
                            span { class: "result-value", "{result.value}" }
                        }
                    }
                } else {
                    p { class: "search-empty", "Enter a value to search the workbook." }
                }
            }
        }
    }
}
