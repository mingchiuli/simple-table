use std::collections::HashMap;
use std::rc::Rc;

use crate::protocol::{EditorRequest, ImageAnchorDto};
use crate::ui::icons::Trash2;
use dioxus::prelude::*;

use crate::actions;
use crate::model::{AppPorts, EditorStore, SheetLayoutView, request_id};

const DEFAULT_ROW_HEIGHT: f64 = 30.0;
const DEFAULT_COLUMN_WIDTH: f64 = 120.0;
const HEADER_HEIGHT: f64 = 31.0;
const HEADER_WIDTH: f64 = 52.0;
const VISIBLE_ROWS: usize = 36;
const VISIBLE_COLUMNS: usize = 16;
const EMU_PER_PIXEL: f64 = 9_525.0;

#[component]
pub fn SpreadsheetGrid() -> Element {
    let mut store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let mut viewport_element = use_signal(|| None::<Rc<MountedData>>);
    let display_cells = use_memo(move || Rc::new(store.display_cell_map(store.active_sheet())));

    use_effect(move || {
        let Some(request) = *store.grid_scroll_request.read() else {
            return;
        };
        let Some(element) = viewport_element.read().clone() else {
            return;
        };
        let document = store.document.read().clone();
        let Some(sheet) = document
            .as_ref()
            .and_then(|document| document.document.sheets.get(request.sheet_index))
        else {
            return;
        };
        if store.active_sheet() != request.sheet_index {
            return;
        }
        let left = scroll_offset(
            request.col,
            HEADER_WIDTH,
            DEFAULT_COLUMN_WIDTH,
            &sheet.layout.column_widths,
        );
        let top = scroll_offset(
            request.row,
            HEADER_HEIGHT,
            DEFAULT_ROW_HEIGHT,
            &sheet.layout.row_heights,
        );
        store.grid_scroll_request.set(None);
        spawn(async move {
            let _ = element
                .scroll(
                    dioxus::html::geometry::PixelsVector2D::new(left, top),
                    ScrollBehavior::Instant,
                )
                .await;
        });
    });

    let document = store.document.read().clone();
    let Some(document) = document else {
        return rsx! { div { class: "grid-loading", "No workbook open" } };
    };
    let sheet_index = store
        .active_sheet()
        .min(document.document.sheets.len().saturating_sub(1));
    let sheet = &document.document.sheets[sheet_index];
    let row_count = sheet.extent.row_count.max(1);
    let column_count = sheet.extent.column_count.max(1);
    let layout = Rc::clone(&sheet.layout);
    let viewport = *store.viewport.read();
    let row_start = viewport.row_start.min(row_count.saturating_sub(1));
    let col_start = viewport.col_start.min(column_count.saturating_sub(1));
    let row_end = row_start.saturating_add(VISIBLE_ROWS).min(row_count);
    let col_end = col_start.saturating_add(VISIBLE_COLUMNS).min(column_count);
    let rows: Vec<_> = (row_start..row_end).collect();
    let columns: Vec<_> = (col_start..col_end).collect();
    let display_cells = display_cells();
    let selected = store.selected_cell();
    let canvas_width =
        HEADER_WIDTH + axis_offset(column_count, DEFAULT_COLUMN_WIDTH, &layout.column_widths);
    let canvas_height =
        HEADER_HEIGHT + axis_offset(row_count, DEFAULT_ROW_HEIGHT, &layout.row_heights);
    let window_left = axis_offset(col_start, DEFAULT_COLUMN_WIDTH, &layout.column_widths);
    let window_top = axis_offset(row_start, DEFAULT_ROW_HEIGHT, &layout.row_heights);
    let column_template = format!(
        "{HEADER_WIDTH}px {}",
        columns
            .iter()
            .map(|col| format!(
                "{}px",
                axis_size(*col, DEFAULT_COLUMN_WIDTH, &layout.column_widths)
            ))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let row_template = format!(
        "{HEADER_HEIGHT}px {}",
        rows.iter()
            .map(|row| format!(
                "{}px",
                axis_size(*row, DEFAULT_ROW_HEIGHT, &layout.row_heights)
            ))
            .collect::<Vec<_>>()
            .join(" ")
    );

    rsx! {
        div {
            class: "grid-viewport",
            role: "grid",
            aria_label: "Spreadsheet cells",
            onmounted: move |event| viewport_element.set(Some(event.data())),
            onscroll: {
                let ports = Rc::clone(&ports);
                let layout = Rc::clone(&layout);
                move |event: Event<ScrollData>| {
                    let next_row = axis_index_at(
                        (event.data().scroll_top() - HEADER_HEIGHT).max(0.0),
                        row_count,
                        DEFAULT_ROW_HEIGHT,
                        &layout.row_heights,
                    );
                    let next_col = axis_index_at(
                        (event.data().scroll_left() - HEADER_WIDTH).max(0.0),
                        column_count,
                        DEFAULT_COLUMN_WIDTH,
                        &layout.column_widths,
                    );
                    let viewport = *store.viewport.read();
                    if next_row != viewport.row_start || next_col != viewport.col_start {
                        let ports = Rc::clone(&ports);
                        spawn(async move {
                            actions::refresh_region(
                                store,
                                ports,
                                next_row,
                                next_row.saturating_add(VISIBLE_ROWS),
                                next_col,
                                next_col.saturating_add(VISIBLE_COLUMNS),
                            )
                            .await;
                        });
                    }
                }
            },
            div {
                class: "grid-canvas",
                style: "width: {canvas_width}px; height: {canvas_height}px;",
                div {
                    class: "grid-window",
                    style: "left: {window_left}px; top: {window_top}px; grid-template-columns: {column_template}; grid-template-rows: {row_template};",
                    div { class: "corner-header", aria_hidden: "true" }
                    for col in columns.iter().copied() {
                        div { class: "column-header", role: "columnheader", "{column_label(col)}" }
                    }

                    for row in rows.iter().copied() {
                        div { class: "row-header", role: "rowheader", "{row + 1}" }
                        for col in columns.iter().copied() {
                            {
                                let value = display_cells
                                    .get(&(row, col))
                                    .cloned()
                                    .unwrap_or_default();
                                let focus_value = value.clone();
                                let is_selected = selected == (row, col);
                                rsx! {
                                    input {
                                        key: "{sheet_index}-{row}-{col}",
                                        class: if is_selected { "grid-cell selected" } else { "grid-cell" },
                                        role: "gridcell",
                                        aria_label: "{column_label(col)}{row + 1}",
                                        disabled: store.busy(),
                                        value,
                                        onfocus: move |_| {
                                            store.selected_cell.set((row, col));
                                            store.selected_image.set(None);
                                            store.formula_text.set(focus_value.clone());
                                        },
                                        oninput: {
                                            let ports = Rc::clone(&ports);
                                            move |event: Event<FormData>| {
                                                let text = event.value();
                                                store.formula_text.set(text.clone());
                                                actions::queue_cell_edit(
                                                    store,
                                                    Rc::clone(&ports),
                                                    sheet_index,
                                                    row,
                                                    col,
                                                    text,
                                                );
                                            }
                                        },
                                        onkeydown: move |event: Event<KeyboardData>| {
                                            if event.key() == Key::Enter {
                                                event.prevent_default();
                                                store.selected_cell.set(((row + 1).min(row_count - 1), col));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                ImageLayer {
                    sheet_index,
                    document_id: document.editor_session.document_id,
                    revision: document.editor_session.revision,
                    layout: Rc::clone(&layout),
                }
            }
        }
    }
}

fn scroll_offset(
    index: usize,
    header_size: f64,
    default_size: f64,
    overrides: &HashMap<usize, u32>,
) -> f64 {
    if index == 0 {
        0.0
    } else {
        header_size + axis_offset(index, default_size, overrides)
    }
}

#[derive(Props, Clone, PartialEq)]
struct ImageLayerProps {
    sheet_index: usize,
    document_id: u64,
    revision: u64,
    layout: Rc<SheetLayoutView>,
}

#[component]
fn ImageLayer(props: ImageLayerProps) -> Element {
    let mut store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let images = store.images.read().clone();
    let assets = store.image_assets.read().clone();
    let selected = store.selected_image.read().clone();

    rsx! {
        div { class: "image-layer", aria_label: "Workbook images",
            for image in images.iter() {
                {
                    let rect = image_rect(&image.anchor, &props.layout);
                    let image_id = image.id.clone();
                    let select_id = image_id.clone();
                    let delete_id = image_id.clone();
                    let is_selected = selected.as_deref() == Some(image_id.as_str());
                    let source = assets.get(&image_id).cloned();
                    rsx! {
                        div {
                            key: "{image_id}",
                            class: if is_selected { "sheet-image selected" } else { "sheet-image" },
                            role: "button",
                            tabindex: 0,
                            aria_label: "Embedded image",
                            style: "left: {rect.left}px; top: {rect.top}px; width: {rect.width}px; height: {rect.height}px; z-index: {image.z_index + 10};",
                            onclick: move |_| store.selected_image.set(Some(select_id.clone())),
                            if let Some(source) = source {
                                img { src: source.as_ref(), alt: "Embedded workbook image" }
                            } else {
                                div { class: "image-placeholder", "Image" }
                            }
                            if is_selected {
                                button {
                                    class: "image-delete",
                                    title: "Delete image",
                                    aria_label: "Delete image",
                                    onclick: {
                                        let ports = Rc::clone(&ports);
                                        move |event| {
                                            event.stop_propagation();
                                            let ports = Rc::clone(&ports);
                                            let image_id = delete_id.clone();
                                            spawn(async move {
                                                actions::run_mutation(
                                                    store,
                                                    ports,
                                                    EditorRequest::DeleteImage {
                                                        request_id: request_id("delete-image"),
                                                        document_id: props.document_id,
                                                        base_revision: props.revision,
                                                        sheet_index: props.sheet_index,
                                                        image_id,
                                                    },
                                                )
                                                .await;
                                            });
                                        }
                                    },
                                    Trash2 { size: 15 }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct ImageRect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

fn image_rect(anchor: &ImageAnchorDto, layout: &SheetLayoutView) -> ImageRect {
    match anchor {
        ImageAnchorDto::OneCell {
            from,
            width_emu,
            height_emu,
        } => ImageRect {
            left: HEADER_WIDTH
                + axis_offset(
                    from.col as usize,
                    DEFAULT_COLUMN_WIDTH,
                    &layout.column_widths,
                )
                + f64::from(from.col_offset_emu) / EMU_PER_PIXEL,
            top: HEADER_HEIGHT
                + axis_offset(from.row as usize, DEFAULT_ROW_HEIGHT, &layout.row_heights)
                + f64::from(from.row_offset_emu) / EMU_PER_PIXEL,
            width: (f64::from(*width_emu) / EMU_PER_PIXEL).max(24.0),
            height: (f64::from(*height_emu) / EMU_PER_PIXEL).max(24.0),
        },
        ImageAnchorDto::TwoCell { from, to } => {
            let left = HEADER_WIDTH
                + axis_offset(
                    from.col as usize,
                    DEFAULT_COLUMN_WIDTH,
                    &layout.column_widths,
                )
                + f64::from(from.col_offset_emu) / EMU_PER_PIXEL;
            let top = HEADER_HEIGHT
                + axis_offset(from.row as usize, DEFAULT_ROW_HEIGHT, &layout.row_heights)
                + f64::from(from.row_offset_emu) / EMU_PER_PIXEL;
            let right = HEADER_WIDTH
                + axis_offset(to.col as usize, DEFAULT_COLUMN_WIDTH, &layout.column_widths)
                + f64::from(to.col_offset_emu) / EMU_PER_PIXEL;
            let bottom = HEADER_HEIGHT
                + axis_offset(to.row as usize, DEFAULT_ROW_HEIGHT, &layout.row_heights)
                + f64::from(to.row_offset_emu) / EMU_PER_PIXEL;
            ImageRect {
                left,
                top,
                width: (right - left).max(24.0),
                height: (bottom - top).max(24.0),
            }
        }
    }
}

fn axis_size(index: usize, default: f64, sizes: &HashMap<usize, u32>) -> f64 {
    sizes.get(&index).copied().map(f64::from).unwrap_or(default)
}

fn axis_offset(index: usize, default: f64, sizes: &HashMap<usize, u32>) -> f64 {
    let adjustment: f64 = sizes
        .iter()
        .filter(|(position, _)| **position < index)
        .map(|(_, size)| f64::from(*size) - default)
        .sum();
    index as f64 * default + adjustment
}

fn axis_index_at(offset: f64, count: usize, default: f64, sizes: &HashMap<usize, u32>) -> usize {
    let mut low = 0usize;
    let mut high = count;
    while low < high {
        let middle = low + (high - low) / 2;
        if axis_offset(middle, default, sizes) <= offset {
            low = middle.saturating_add(1);
        } else {
            high = middle;
        }
    }
    low.saturating_sub(1).min(count.saturating_sub(1))
}

pub fn column_label(mut col: usize) -> String {
    let mut label = String::new();
    loop {
        label.insert(0, (b'A' + (col % 26) as u8) as char);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variable_axis_offsets_and_lookup_are_inverse() {
        let sizes = HashMap::from([(1, 200), (4, 80)]);
        assert_eq!(axis_offset(3, 120.0, &sizes), 440.0);
        assert_eq!(axis_index_at(441.0, 10, 120.0, &sizes), 3);
    }
}
