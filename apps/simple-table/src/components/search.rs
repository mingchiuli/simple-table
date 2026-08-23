use std::rc::Rc;

use dioxus::prelude::*;
use simple_table_components::icons::{Search, X};
use simple_table_components::{
    Button, ButtonSize, ButtonVariant, Input, Item, ItemContent, ItemDescription, ItemGroup,
    ItemSize, ItemTitle, ScrollArea, ScrollDirection, Switch,
};

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
                Button {
                    class: "icon-button",
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
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
                    Input {
                        value: query,
                        placeholder: "Search cells",
                        aria_label: "Search cells",
                        oninput: move |event: Event<FormData>| query.set(event.value()),
                    }
                }
                div { class: "search-scope",
                    span { "All sheets" }
                    Switch {
                        checked: Some(all_sheets()),
                        on_checked_change: move |checked| all_sheets.set(checked),
                        aria_label: "Search all sheets",
                    }
                }
                Button { class: "search-submit", r#type: "submit", "Search" }
            }

            ScrollArea { class: "search-results", direction: ScrollDirection::Vertical,
                if let Some(response) = response {
                    div { class: "result-count",
                        "{response.results.len()} results"
                        if response.truncated { span { " (limited)" } }
                    }
                    ItemGroup {
                    for result in response.results {
                        {
                            let ports = Rc::clone(&ports);
                            let sheet_index = result.sheet_index;
                            let row = result.row;
                            let col = result.col;
                            let sheet_name = result.sheet_name;
                            let cell_position = result.cell_position;
                            let value = result.value;
                            rsx! {
                                Item {
                                    size: ItemSize::Sm,
                                    r#as: move |attributes: Vec<Attribute>| {
                                        let ports = Rc::clone(&ports);
                                        rsx! {
                                            button {
                                                class: "search-result",
                                                onclick: move |_| {
                                                    let ports = Rc::clone(&ports);
                                                    spawn(async move {
                                                        actions::select_search_result(
                                                            store,
                                                            ports,
                                                            sheet_index,
                                                            row,
                                                            col,
                                                        )
                                                        .await;
                                                    });
                                                },
                                                ..attributes,
                                                ItemContent {
                                                    ItemTitle { class: "result-location", "{sheet_name} · {cell_position}" }
                                                    ItemDescription { class: "result-value", "{value}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    }
                } else {
                    p { class: "search-empty", "Enter a value to search the workbook." }
                }
            }
        }
    }
}
