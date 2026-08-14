use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::protocol::{EditorRequest, ImageAnchorDto};
use crate::ui::icons::Trash2;
use dioxus::prelude::*;

use crate::actions;
use crate::model::{AppPorts, EditorStore, MergeRangeView, SheetLayoutView, request_id};

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
    let mut focused_cell = use_signal(|| None::<(usize, usize, usize)>);
    let cell_elements = use_hook(|| {
        Rc::new(RefCell::new(HashMap::<
            (usize, usize, usize),
            Rc<MountedData>,
        >::new()))
    });
    let cell_presentations =
        use_memo(move || Rc::new(store.cell_presentation_map(store.active_sheet())));

    use_effect({
        let cell_elements = Rc::clone(&cell_elements);
        move || {
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
            let (target_row, target_col) = if request.focus {
                store.normalize_cell(request.sheet_index, request.row, request.col)
            } else {
                (request.row, request.col)
            };
            let left = scroll_offset(
                target_col,
                HEADER_WIDTH,
                DEFAULT_COLUMN_WIDTH,
                &sheet.layout.column_widths,
            );
            let top = scroll_offset(
                target_row,
                HEADER_HEIGHT,
                DEFAULT_ROW_HEIGHT,
                &sheet.layout.row_heights,
            );
            let target = request.focus.then(|| {
                cell_elements
                    .borrow()
                    .get(&(request.sheet_index, target_row, target_col))
                    .cloned()
            });
            if !request.focus || target.as_ref().is_some_and(Option::is_some) {
                store.grid_scroll_request.set(None);
            }
            spawn(async move {
                let _ = element
                    .scroll(
                        dioxus::html::geometry::PixelsVector2D::new(left, top),
                        ScrollBehavior::Instant,
                    )
                    .await;
            });
            if let Some(Some(cell)) = target {
                spawn(async move {
                    let _ = cell.set_focus(true).await;
                });
            }
        }
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
    let merge_ranges = store.merge_ranges(sheet_index);
    let cell_items = build_grid_cell_items(
        row_start,
        row_end,
        col_start,
        col_end,
        &merge_ranges,
        &layout,
    );
    let mounted_keys = cell_items
        .iter()
        .map(|item| (item.row, item.col))
        .collect::<HashSet<_>>();
    cell_elements
        .borrow_mut()
        .retain(|(mounted_sheet, row, col), _| {
            *mounted_sheet == sheet_index && mounted_keys.contains(&(*row, *col))
        });
    let cell_presentations = cell_presentations();
    let focused = *focused_cell.read();
    let selected = store.normalize_cell(
        sheet_index,
        store.selected_cell().0,
        store.selected_cell().1,
    );
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
                    div {
                        class: "corner-header",
                        style: "grid-row: 1; grid-column: 1;",
                        aria_hidden: "true"
                    }
                    for (column_index, col) in columns.iter().copied().enumerate() {
                        div {
                            class: "column-header",
                            role: "columnheader",
                            style: "grid-row: 1; grid-column: {column_index + 2};",
                            "{column_label(col)}"
                        }
                    }
                    for (row_index, row) in rows.iter().copied().enumerate() {
                        div {
                            class: "row-header",
                            role: "rowheader",
                            style: "grid-row: {row_index + 2}; grid-column: 1;",
                            "{row + 1}"
                        }
                    }
                }
                for item in cell_items.iter().copied() {
                    {
                        let row = item.row;
                        let col = item.col;
                        let presentation = cell_presentations
                            .get(&(row, col))
                            .cloned()
                            .unwrap_or_default();
                        let focus_value = presentation.edit_text.clone();
                        let value = if focused == Some((sheet_index, row, col)) {
                            presentation.edit_text.clone()
                        } else {
                            presentation.display_text.clone()
                        };
                        let is_selected = selected == (row, col);
                        let has_formula_error = presentation.formula_error.is_some();
                        let class = cell_class(is_selected, has_formula_error, item.merge.is_some());
                        let address = item.address();
                        let aria_label = presentation.formula_error.as_ref().map_or_else(
                            || address.clone(),
                            |error| format!("{address}, formula error: {error}"),
                        );
                        let aria_rowspan = item.merge.map(|merge| merge.row_span().to_string());
                        let aria_colspan = item.merge.map(|merge| merge.col_span().to_string());
                        let style = item.style();
                        let mounted_cells = Rc::clone(&cell_elements);
                        let enter_cells = Rc::clone(&cell_elements);
                        let enter_target = cell_after_enter(
                            row,
                            col,
                            row_count,
                            item.merge,
                            &merge_ranges,
                        );
                        let enter_value = enter_target
                            .and_then(|cell| cell_presentations.get(&cell))
                            .map(|cell| cell.edit_text.clone())
                            .unwrap_or_default();
                        rsx! {
                            input {
                                key: "{sheet_index}-{row}-{col}",
                                class,
                                style,
                                role: "gridcell",
                                aria_label,
                                aria_rowspan,
                                aria_colspan,
                                aria_invalid: has_formula_error.then_some("true"),
                                title: presentation.formula_error,
                                disabled: store.busy(),
                                value,
                                onmounted: move |event| {
                                    let cell = event.data();
                                    mounted_cells
                                        .borrow_mut()
                                        .insert((sheet_index, row, col), Rc::clone(&cell));
                                    let should_focus = store
                                        .grid_scroll_request
                                        .read()
                                        .is_some_and(|request| {
                                            let target = store.normalize_cell(
                                                request.sheet_index,
                                                request.row,
                                                request.col,
                                            );
                                            request.focus
                                                && request.sheet_index == sheet_index
                                                && target == (row, col)
                                        });
                                    if should_focus {
                                        store.grid_scroll_request.set(None);
                                        spawn(async move {
                                            let _ = cell.set_focus(true).await;
                                        });
                                    }
                                },
                                onfocus: move |_| {
                                    focused_cell.set(Some((sheet_index, row, col)));
                                    store.selected_cell.set((row, col));
                                    store.selected_image.set(None);
                                    store.formula_text.set(focus_value.clone());
                                },
                                onblur: move |_| {
                                    if *focused_cell.read() == Some((sheet_index, row, col)) {
                                        focused_cell.set(None);
                                    }
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
                                        if let Some(next) = enter_target {
                                            store.selected_cell.set(next);
                                            store.selected_image.set(None);
                                            store.formula_text.set(enter_value.clone());
                                            let target = enter_cells
                                                .borrow()
                                                .get(&(sheet_index, next.0, next.1))
                                                .cloned();
                                            if let Some(target) = target {
                                                spawn(async move {
                                                    let _ = target.set_focus(true).await;
                                                });
                                            } else {
                                                store.grid_scroll_request.set(Some(
                                                    crate::model::GridScrollRequest {
                                                        sheet_index,
                                                        row: next.0,
                                                        col: next.1,
                                                        focus: true,
                                                    },
                                                ));
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

fn cell_after_enter(
    row: usize,
    col: usize,
    row_count: usize,
    current_merge: Option<MergeRangeView>,
    merges: &[MergeRangeView],
) -> Option<(usize, usize)> {
    let next_row = current_merge.map_or(row.saturating_add(1), |merge| {
        merge.end_row.saturating_add(1)
    });
    if next_row >= row_count {
        return None;
    }
    Some(
        merges
            .iter()
            .copied()
            .find(|merge| merge.contains(next_row, col))
            .map_or((next_row, col), MergeRangeView::anchor),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GridCellItem {
    row: usize,
    col: usize,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    merge: Option<MergeRangeView>,
}

impl GridCellItem {
    fn style(self) -> String {
        format!(
            "left: {}px; top: {}px; width: {}px; height: {}px;",
            self.left, self.top, self.width, self.height
        )
    }

    fn address(self) -> String {
        self.merge.map_or_else(
            || format!("{}{}", column_label(self.col), self.row + 1),
            |merge| {
                format!(
                    "{}{}:{}{}",
                    column_label(merge.start_col),
                    merge.start_row + 1,
                    column_label(merge.end_col),
                    merge.end_row + 1
                )
            },
        )
    }
}

fn build_grid_cell_items(
    row_start: usize,
    row_end: usize,
    col_start: usize,
    col_end: usize,
    merges: &[MergeRangeView],
    layout: &SheetLayoutView,
) -> Vec<GridCellItem> {
    let mut items = Vec::with_capacity(
        row_end
            .saturating_sub(row_start)
            .saturating_mul(col_end.saturating_sub(col_start)),
    );
    for row in row_start..row_end {
        for col in col_start..col_end {
            if merges.iter().any(|merge| merge.contains(row, col)) {
                continue;
            }
            items.push(grid_cell_item(row, col, None, layout));
        }
    }
    items.extend(
        merges
            .iter()
            .copied()
            .filter(|merge| merge.intersects(row_start, row_end, col_start, col_end))
            .map(|merge| grid_cell_item(merge.start_row, merge.start_col, Some(merge), layout)),
    );
    items
}

fn grid_cell_item(
    row: usize,
    col: usize,
    merge: Option<MergeRangeView>,
    layout: &SheetLayoutView,
) -> GridCellItem {
    let end_row = merge.map_or(row, |merge| merge.end_row);
    let end_col = merge.map_or(col, |merge| merge.end_col);
    GridCellItem {
        row,
        col,
        left: HEADER_WIDTH + axis_offset(col, DEFAULT_COLUMN_WIDTH, &layout.column_widths),
        top: HEADER_HEIGHT + axis_offset(row, DEFAULT_ROW_HEIGHT, &layout.row_heights),
        width: axis_offset(
            end_col.saturating_add(1),
            DEFAULT_COLUMN_WIDTH,
            &layout.column_widths,
        ) - axis_offset(col, DEFAULT_COLUMN_WIDTH, &layout.column_widths),
        height: axis_offset(
            end_row.saturating_add(1),
            DEFAULT_ROW_HEIGHT,
            &layout.row_heights,
        ) - axis_offset(row, DEFAULT_ROW_HEIGHT, &layout.row_heights),
        merge,
    }
}

fn cell_class(selected: bool, formula_error: bool, merged: bool) -> &'static str {
    match (selected, formula_error, merged) {
        (true, true, true) => "grid-cell selected formula-error merged-cell",
        (true, true, false) => "grid-cell selected formula-error",
        (true, false, true) => "grid-cell selected merged-cell",
        (true, false, false) => "grid-cell selected",
        (false, true, true) => "grid-cell formula-error merged-cell",
        (false, true, false) => "grid-cell formula-error",
        (false, false, true) => "grid-cell merged-cell",
        (false, false, false) => "grid-cell",
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

    #[test]
    fn enter_moves_down_until_the_last_row() {
        assert_eq!(cell_after_enter(2, 4, 5, None, &[]), Some((3, 4)));
        assert_eq!(cell_after_enter(4, 4, 5, None, &[]), None);
    }

    #[test]
    fn merged_cell_uses_the_full_range_geometry_across_a_clipped_viewport() {
        let layout = SheetLayoutView {
            column_widths: HashMap::from([(0, 80), (1, 140)]),
            row_heights: HashMap::from([(0, 20), (1, 40)]),
        };
        let merge = merge(0, 0, 1, 1);

        let items = build_grid_cell_items(1, 2, 1, 2, &[merge], &layout);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].row, 0);
        assert_eq!(items[0].col, 0);
        assert_eq!(items[0].left, HEADER_WIDTH);
        assert_eq!(items[0].top, HEADER_HEIGHT);
        assert_eq!(items[0].width, 220.0);
        assert_eq!(items[0].height, 60.0);
        assert_eq!(items[0].address(), "A1:B2");
    }

    #[test]
    fn merged_cells_replace_all_covered_regular_cells() {
        let merge = merge(0, 0, 1, 1);

        let items = build_grid_cell_items(0, 3, 0, 3, &[merge], &SheetLayoutView::default());

        assert_eq!(items.len(), 6);
        assert_eq!(items.iter().filter(|item| item.merge.is_some()).count(), 1);
        assert!(
            !items
                .iter()
                .any(|item| { item.merge.is_none() && merge.contains(item.row, item.col) })
        );
    }

    #[test]
    fn enter_skips_the_current_merge_and_normalizes_the_destination() {
        let current = merge(1, 1, 3, 2);
        let destination = merge(4, 0, 5, 2);

        assert_eq!(
            cell_after_enter(1, 1, 8, Some(current), &[current, destination]),
            Some((4, 0))
        );
    }

    fn merge(start_row: usize, start_col: usize, end_row: usize, end_col: usize) -> MergeRangeView {
        MergeRangeView {
            start_row,
            start_col,
            end_row,
            end_col,
        }
    }
}
