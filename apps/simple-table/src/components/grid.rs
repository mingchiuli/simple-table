mod geometry;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use crate::protocol::{EditorRequest, ImageAnchorDto};
use dioxus::prelude::*;
use dioxus_sdk_time::sleep;
use simple_table_components::icons::{Trash2, X};
use simple_table_components::{Button, ButtonSize, ButtonVariant, Dialog, DialogTitle};

use self::geometry::SparseAxisGeometry;
use crate::actions;
use crate::model::{
    AppPorts, CellPresentation, EditorStore, GridRenderWindow, MergeRangeView, SheetExtentView,
    SheetRegionBoundsView, request_id,
};

const DEFAULT_ROW_HEIGHT: f64 = 30.0;
const DEFAULT_COLUMN_WIDTH: f64 = 120.0;
const HEADER_HEIGHT: f64 = 31.0;
const HEADER_WIDTH: f64 = 52.0;
const OVERSCAN_PIXELS: f64 = 240.0;
const SCROLL_FRAME: Duration = Duration::from_millis(16);
const EMU_PER_PIXEL: f64 = 9_525.0;

#[derive(Clone, Copy, Debug, Default)]
struct ScrollSnapshot {
    top: f64,
    left: f64,
    width: f64,
    height: f64,
}

struct GridViewport<'a> {
    sheet_index: usize,
    extent: SheetExtentView,
    physical_extent: SheetExtentView,
    visible_rows: &'a [usize],
    rows: &'a SparseAxisGeometry,
    columns: &'a SparseAxisGeometry,
}

#[component]
pub fn SpreadsheetGrid() -> Element {
    let mut store = use_context::<EditorStore>();
    let ports = use_context::<Rc<AppPorts>>();
    let mut viewport_element = use_signal(|| None::<Rc<MountedData>>);
    let mut editing_cell = use_signal(|| None::<(usize, usize, usize)>);
    let mut previewed_image = use_signal(|| None::<String>);
    let last_scroll = use_hook(|| Rc::new(RefCell::new(ScrollSnapshot::default())));
    let pending_scroll = use_hook(|| Rc::new(RefCell::new(ScrollSnapshot::default())));
    let scroll_scheduled = use_hook(|| Rc::new(Cell::new(false)));
    let visible_rows = use_memo(move || {
        let document = store.document.read();
        let Some(document) = document.as_ref() else {
            return Rc::new(vec![0]);
        };
        let sheet_index = store
            .active_sheet()
            .min(document.document.sheets.len().saturating_sub(1));
        let row_count = document.document.sheets[sheet_index]
            .extent
            .row_count
            .max(1);
        Rc::new(store.visible_rows(sheet_index, row_count))
    });

    let document = store.document.read().clone();
    let Some(document) = document else {
        return rsx! { div { class: "grid-loading", "No workbook open" } };
    };
    let sheet_index = store
        .active_sheet()
        .min(document.document.sheets.len().saturating_sub(1));
    let sheet = &document.document.sheets[sheet_index];
    let physical_extent = SheetExtentView {
        row_count: sheet.extent.row_count.max(1),
        column_count: sheet.extent.column_count.max(1),
    };
    let visible_rows = visible_rows();
    let filtered = store.sheet_filter(sheet_index).is_some();
    let visible_row_heights = visible_rows
        .iter()
        .enumerate()
        .filter_map(|(ordinal, physical)| {
            sheet
                .layout
                .row_heights
                .get(physical)
                .copied()
                .map(|height| (ordinal, height))
        })
        .collect::<HashMap<_, _>>();
    let extent = SheetExtentView {
        row_count: visible_rows.len().max(1),
        column_count: physical_extent.column_count,
    };
    let row_geometry = Rc::new(SparseAxisGeometry::new(
        DEFAULT_ROW_HEIGHT,
        &visible_row_heights,
    ));
    let column_geometry = Rc::new(SparseAxisGeometry::new(
        DEFAULT_COLUMN_WIDTH,
        &sheet.layout.column_widths,
    ));

    use_effect({
        let ports = Rc::clone(&ports);
        let row_geometry = Rc::clone(&row_geometry);
        let column_geometry = Rc::clone(&column_geometry);
        let last_scroll = Rc::clone(&last_scroll);
        let visible_rows = Rc::clone(&visible_rows);
        move || {
            let Some(request) = *store.grid_scroll_request.read() else {
                return;
            };
            if request.sheet_index != sheet_index {
                return;
            }
            let Some(element) = viewport_element.read().clone() else {
                return;
            };
            let physical_target = if request.focus {
                store.normalize_cell(request.sheet_index, request.row, request.col)
            } else {
                (request.row, request.col)
            };
            let target_row = visible_rows
                .binary_search(&physical_target.0)
                .unwrap_or_else(|index| index.min(visible_rows.len().saturating_sub(1)));
            let target = (target_row, physical_target.1);
            let left = scroll_offset(target.1, HEADER_WIDTH, &column_geometry);
            let top = scroll_offset(target.0, HEADER_HEIGHT, &row_geometry);
            store.grid_scroll_request.set(None);
            if request.focus {
                let physical_row = visible_rows[target.0];
                store.select_cell(sheet_index, physical_row, target.1);
                editing_cell.set(Some((sheet_index, physical_row, target.1)));
            }
            let mut snapshot = *last_scroll.borrow();
            snapshot.left = left;
            snapshot.top = top;
            *last_scroll.borrow_mut() = snapshot;
            update_render_window(
                store,
                &ports,
                &GridViewport {
                    sheet_index,
                    extent,
                    physical_extent,
                    visible_rows: &visible_rows,
                    rows: &row_geometry,
                    columns: &column_geometry,
                },
                snapshot,
                true,
            );
            spawn(async move {
                let _ = element
                    .scroll(
                        dioxus::html::geometry::PixelsVector2D::new(left, top),
                        ScrollBehavior::Instant,
                    )
                    .await;
            });
        }
    });

    let window =
        store
            .render_window
            .read()
            .clamped(sheet_index, extent.row_count, extent.column_count);
    let row_ordinals = (window.row_start..window.row_end).collect::<Vec<_>>();
    let rows = visible_rows
        .get(window.row_start..window.row_end)
        .unwrap_or_default()
        .to_vec();
    let columns = (window.col_start..window.col_end).collect::<Vec<_>>();
    let bounds = SheetRegionBoundsView {
        sheet_index,
        row_start: rows.first().copied().unwrap_or(0),
        row_end: rows.last().copied().map_or(1, |row| row.saturating_add(1)),
        col_start: window.col_start,
        col_end: window.col_end,
    };
    let mut projection = store
        .region_cache
        .read()
        .projection(sheet_index, Some(bounds));
    for ((pending_sheet, row, col), (_, value)) in store.pending_edits.read().iter() {
        let is_visible_merge_anchor = projection
            .merges
            .iter()
            .any(|merge| merge.anchor() == (*row, *col));
        if *pending_sheet == sheet_index
            && (contains(bounds, *row, *col) || is_visible_merge_anchor)
        {
            projection.cells.insert(
                (*row, *col),
                CellPresentation {
                    display_text: Rc::clone(value),
                    edit_text: Rc::clone(value),
                    formula_error: None,
                },
            );
        }
    }
    let merges = if filtered {
        &[][..]
    } else {
        projection.merges.as_slice()
    };
    let cell_items = build_grid_cell_items_mapped(
        &rows,
        window.row_start,
        window.col_start,
        window.col_end,
        merges,
        &row_geometry,
        &column_geometry,
    );
    let selection = *store.selection.read();
    let editing = *editing_cell.read();
    let canvas_width = HEADER_WIDTH + column_geometry.total_size(extent.column_count);
    let canvas_height = HEADER_HEIGHT + row_geometry.total_size(extent.row_count);
    let column_window_left = HEADER_WIDTH + column_geometry.offset(window.col_start);
    let row_window_top = row_geometry.offset(window.row_start);
    let column_template = columns
        .iter()
        .map(|col| format!("{}px", column_geometry.size(*col)))
        .collect::<Vec<_>>()
        .join(" ");
    let row_template = row_ordinals
        .iter()
        .map(|row| format!("{}px", row_geometry.size(*row)))
        .collect::<Vec<_>>()
        .join(" ");
    let preview_source = previewed_image
        .read()
        .as_ref()
        .and_then(|image_id| store.image_assets.read().get(image_id).cloned());

    rsx! {
        div {
            class: "grid-viewport",
            role: "grid",
            aria_label: "Spreadsheet cells",
            onmounted: {
                let ports = Rc::clone(&ports);
                let row_geometry = Rc::clone(&row_geometry);
                let column_geometry = Rc::clone(&column_geometry);
                let last_scroll = Rc::clone(&last_scroll);
                let visible_rows = Rc::clone(&visible_rows);
                move |event| {
                    let element = event.data();
                    viewport_element.set(Some(Rc::clone(&element)));
                    let ports = Rc::clone(&ports);
                    let row_geometry = Rc::clone(&row_geometry);
                    let column_geometry = Rc::clone(&column_geometry);
                    let last_scroll = Rc::clone(&last_scroll);
                    let visible_rows = Rc::clone(&visible_rows);
                    spawn(async move {
                        if let Ok(rect) = element.get_client_rect().await {
                            let snapshot = ScrollSnapshot {
                                width: rect.size.width,
                                height: rect.size.height,
                                ..ScrollSnapshot::default()
                            };
                            *last_scroll.borrow_mut() = snapshot;
                            update_render_window(
                                store,
                                &ports,
                                &GridViewport {
                                    sheet_index,
                                    extent,
                                    physical_extent,
                                    visible_rows: &visible_rows,
                                    rows: &row_geometry,
                                    columns: &column_geometry,
                                },
                                snapshot,
                                true,
                            );
                        }
                    });
                }
            },
            onscroll: {
                let ports = Rc::clone(&ports);
                let row_geometry = Rc::clone(&row_geometry);
                let column_geometry = Rc::clone(&column_geometry);
                let last_scroll = Rc::clone(&last_scroll);
                let pending_scroll = Rc::clone(&pending_scroll);
                let scroll_scheduled = Rc::clone(&scroll_scheduled);
                let visible_rows = Rc::clone(&visible_rows);
                move |event: Event<ScrollData>| {
                    let snapshot = ScrollSnapshot {
                        top: event.data().scroll_top(),
                        left: event.data().scroll_left(),
                        width: f64::from(event.data().client_width()),
                        height: f64::from(event.data().client_height()),
                    };
                    *pending_scroll.borrow_mut() = snapshot;
                    *last_scroll.borrow_mut() = snapshot;
                    if scroll_scheduled.replace(true) {
                        return;
                    }
                    let ports = Rc::clone(&ports);
                    let row_geometry = Rc::clone(&row_geometry);
                    let column_geometry = Rc::clone(&column_geometry);
                    let pending_scroll = Rc::clone(&pending_scroll);
                    let scroll_scheduled = Rc::clone(&scroll_scheduled);
                    let visible_rows = Rc::clone(&visible_rows);
                    spawn(async move {
                        sleep(SCROLL_FRAME).await;
                        let snapshot = *pending_scroll.borrow();
                        update_render_window(
                            store,
                            &ports,
                            &GridViewport {
                                sheet_index,
                                extent,
                                physical_extent,
                                visible_rows: &visible_rows,
                                rows: &row_geometry,
                                columns: &column_geometry,
                            },
                            snapshot,
                            false,
                        );
                        scroll_scheduled.set(false);
                    });
                }
            },
            div {
                class: "grid-canvas",
                style: "width: {canvas_width}px; height: {canvas_height}px;",
                div {
                    class: "grid-column-header-track",
                    div {
                        class: "corner-header",
                        aria_hidden: "true"
                    }
                    div {
                        class: "grid-column-header-window",
                        style: "left: {column_window_left}px; grid-template-columns: {column_template};",
                        for (column_index, col) in columns.iter().copied().enumerate() {
                            div {
                                key: "column-header-{col}",
                                class: "column-header has-data-tools",
                                role: "columnheader",
                                style: "grid-column: {column_index + 1};",
                                span { "{column_label(col)}" }
                                super::editor::TableDataTools {
                                    document_id: document.editor_session.document_id,
                                    revision: document.editor_session.revision,
                                    sheet_index,
                                    selected: (selection.row, col),
                                    compact: true,
                                }
                            }
                        }
                    }
                }
                div {
                    class: "grid-row-header-window",
                    style: "margin-top: {row_window_top}px; grid-template-rows: {row_template};",
                    for (row_index, row) in rows.iter().copied().enumerate() {
                        div {
                            key: "row-header-{row}",
                            class: "row-header",
                            role: "rowheader",
                            style: "grid-row: {row_index + 1};",
                            "{row + 1}"
                        }
                    }
                }
                for item in cell_items.iter().copied() {
                    {
                        let row = item.row;
                        let col = item.col;
                        let presentation = projection.cells.get(&(row, col)).cloned().unwrap_or_default();
                        let is_selected = selection.sheet_index == sheet_index
                            && (selection.row, selection.col) == (row, col);
                        let is_editing = is_editing_cell(editing, sheet_index, row, col);
                        let has_formula_error = presentation.formula_error.is_some();
                        let class = cell_class(is_selected, has_formula_error, item.merge.is_some(), is_editing);
                        let address = item.address();
                        let aria_label = presentation.formula_error.as_ref().map_or_else(
                            || address.clone(),
                            |error| format!("{address}, formula error: {error}"),
                        );
                        let aria_rowspan = item.merge.map(|merge| merge.row_span().to_string());
                        let aria_colspan = item.merge.map(|merge| merge.col_span().to_string());
                        let style = item.style();
                        let enter_target = cell_after_enter_visible(
                            row,
                            col,
                            &visible_rows,
                            item.merge,
                            merges,
                        );
                        let enter_value = enter_target
                            .and_then(|next| projection.cells.get(&next))
                            .map(|cell| cell.edit_text.to_string())
                            .unwrap_or_default();
                        rsx! {
                            if is_editing {
                                input {
                                    key: "editor-{sheet_index}-{row}-{col}",
                                    class,
                                    style,
                                    role: "gridcell",
                                    aria_label,
                                    aria_rowspan,
                                    aria_colspan,
                                    aria_invalid: has_formula_error.then_some("true"),
                                    title: presentation.formula_error.as_deref(),
                                    disabled: store.busy(),
                                    value: presentation.edit_text.as_ref(),
                                    onmounted: move |event| {
                                        let element = event.data();
                                        spawn(async move {
                                            let _ = element.set_focus(true).await;
                                        });
                                    },
                                    onblur: move |_| {
                                        if *editing_cell.read() == Some((sheet_index, row, col)) {
                                            editing_cell.set(None);
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
                                                store.select_cell(sheet_index, next.0, next.1);
                                                store.selected_image.set(None);
                                                store.formula_text.set(enter_value.clone());
                                                if contains(bounds, next.0, next.1) {
                                                    editing_cell.set(Some((sheet_index, next.0, next.1)));
                                                } else {
                                                    editing_cell.set(None);
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
                                        } else if event.key() == Key::Escape {
                                            editing_cell.set(None);
                                        }
                                    }
                                }
                            } else {
                                div {
                                    key: "cell-{sheet_index}-{row}-{col}",
                                    class,
                                    style,
                                    role: "gridcell",
                                    aria_label,
                                    aria_rowspan,
                                    aria_colspan,
                                    aria_invalid: has_formula_error.then_some("true"),
                                    title: presentation.formula_error.as_deref(),
                                    tabindex: if is_selected { 0 } else { -1 },
                                    onclick: move |_| {
                                        store.select_cell(sheet_index, row, col);
                                        store.selected_image.set(None);
                                        store.formula_text.set(presentation.edit_text.to_string());
                                        editing_cell.set(Some((sheet_index, row, col)));
                                    },
                                    "{presentation.display_text}"
                                }
                            }
                        }
                    }
                }
                if !filtered {
                    ImageLayer {
                        sheet_index,
                        document_id: document.editor_session.document_id,
                        revision: document.editor_session.revision,
                        row_geometry: Rc::clone(&row_geometry),
                        column_geometry: Rc::clone(&column_geometry),
                        on_preview: move |image_id| previewed_image.set(Some(image_id)),
                    }
                }
            }
        }
        if let Some(source) = preview_source {
            ImagePreview {
                source,
                on_close: move |_| previewed_image.set(None),
            }
        }
    }
}

fn update_render_window(
    mut store: EditorStore,
    ports: &AppPorts,
    viewport: &GridViewport<'_>,
    scroll: ScrollSnapshot,
    force: bool,
) {
    if scroll.width <= 0.0 || scroll.height <= 0.0 {
        return;
    }
    let row_start_px = (scroll.top - HEADER_HEIGHT).max(0.0);
    let row_end_px = (scroll.top + scroll.height - HEADER_HEIGHT).max(row_start_px);
    let col_start_px = (scroll.left - HEADER_WIDTH).max(0.0);
    let col_end_px = (scroll.left + scroll.width - HEADER_WIDTH).max(col_start_px);
    let (visible_row_start, visible_row_end) =
        viewport
            .rows
            .range_for_pixels(row_start_px, row_end_px, viewport.extent.row_count);
    let (visible_col_start, visible_col_end) =
        viewport
            .columns
            .range_for_pixels(col_start_px, col_end_px, viewport.extent.column_count);
    let current = *store.render_window.peek();
    let visible_is_retained = current.sheet_index == viewport.sheet_index
        && current.row_start <= visible_row_start
        && current.row_end >= visible_row_end
        && current.col_start <= visible_col_start
        && current.col_end >= visible_col_end;
    if !force && visible_is_retained {
        return;
    }

    let (row_start, row_end) = viewport.rows.range_for_pixels(
        (row_start_px - OVERSCAN_PIXELS).max(0.0),
        row_end_px + OVERSCAN_PIXELS,
        viewport.extent.row_count,
    );
    let (col_start, col_end) = viewport.columns.range_for_pixels(
        (col_start_px - OVERSCAN_PIXELS).max(0.0),
        col_end_px + OVERSCAN_PIXELS,
        viewport.extent.column_count,
    );
    let next = GridRenderWindow {
        sheet_index: viewport.sheet_index,
        row_start,
        row_end,
        col_start,
        col_end,
    };
    if next != current {
        store.render_window.set(next);
    }
    if viewport.visible_rows.len() == viewport.physical_extent.row_count {
        ports
            .regions
            .schedule_viewport(store, next.bounds(), viewport.physical_extent);
    } else {
        ports.regions.schedule_visible_rows(
            store,
            viewport.sheet_index,
            viewport
                .visible_rows
                .get(row_start..row_end)
                .unwrap_or_default(),
            col_start,
            col_end,
            viewport.physical_extent,
        );
    }
}

fn cell_after_enter_visible(
    row: usize,
    col: usize,
    visible_rows: &[usize],
    current_merge: Option<MergeRangeView>,
    merges: &[MergeRangeView],
) -> Option<(usize, usize)> {
    let current_end = current_merge.map_or(row, |merge| merge.end_row);
    let next_row = visible_rows
        .iter()
        .copied()
        .find(|candidate| *candidate > current_end)?;
    Some(
        merges
            .iter()
            .copied()
            .find(|merge| merge.contains(next_row, col))
            .map_or((next_row, col), MergeRangeView::anchor),
    )
}

#[cfg(test)]
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
    rows: &SparseAxisGeometry,
    columns: &SparseAxisGeometry,
) -> Vec<GridCellItem> {
    let visible_merges = merges
        .iter()
        .copied()
        .filter(|merge| merge.intersects(row_start, row_end, col_start, col_end))
        .collect::<Vec<_>>();
    let mut covered = HashSet::new();
    for merge in &visible_merges {
        for row in merge.start_row.max(row_start)..=merge.end_row.min(row_end.saturating_sub(1)) {
            for col in merge.start_col.max(col_start)..=merge.end_col.min(col_end.saturating_sub(1))
            {
                covered.insert((row, col));
            }
        }
    }
    let mut items = Vec::with_capacity(
        row_end
            .saturating_sub(row_start)
            .saturating_mul(col_end.saturating_sub(col_start)),
    );
    for row in row_start..row_end {
        for col in col_start..col_end {
            if !covered.contains(&(row, col)) {
                items.push(grid_cell_item(row, col, None, rows, columns));
            }
        }
    }
    items.extend(
        visible_merges.into_iter().map(|merge| {
            grid_cell_item(merge.start_row, merge.start_col, Some(merge), rows, columns)
        }),
    );
    items
}

fn build_grid_cell_items_mapped(
    physical_rows: &[usize],
    ordinal_start: usize,
    col_start: usize,
    col_end: usize,
    merges: &[MergeRangeView],
    rows: &SparseAxisGeometry,
    columns: &SparseAxisGeometry,
) -> Vec<GridCellItem> {
    if physical_rows
        .iter()
        .enumerate()
        .all(|(offset, row)| *row == ordinal_start + offset)
    {
        return build_grid_cell_items(
            ordinal_start,
            ordinal_start + physical_rows.len(),
            col_start,
            col_end,
            merges,
            rows,
            columns,
        );
    }
    let mut items = Vec::with_capacity(
        physical_rows
            .len()
            .saturating_mul(col_end.saturating_sub(col_start)),
    );
    for (offset, physical_row) in physical_rows.iter().copied().enumerate() {
        let ordinal = ordinal_start + offset;
        for col in col_start..col_end {
            items.push(GridCellItem {
                row: physical_row,
                col,
                left: HEADER_WIDTH + columns.offset(col),
                top: HEADER_HEIGHT + rows.offset(ordinal),
                width: columns.offset(col.saturating_add(1)) - columns.offset(col),
                height: rows.offset(ordinal.saturating_add(1)) - rows.offset(ordinal),
                merge: None,
            });
        }
    }
    items
}

fn grid_cell_item(
    row: usize,
    col: usize,
    merge: Option<MergeRangeView>,
    rows: &SparseAxisGeometry,
    columns: &SparseAxisGeometry,
) -> GridCellItem {
    let end_row = merge.map_or(row, |merge| merge.end_row);
    let end_col = merge.map_or(col, |merge| merge.end_col);
    GridCellItem {
        row,
        col,
        left: HEADER_WIDTH + columns.offset(col),
        top: HEADER_HEIGHT + rows.offset(row),
        width: columns.offset(end_col.saturating_add(1)) - columns.offset(col),
        height: rows.offset(end_row.saturating_add(1)) - rows.offset(row),
        merge,
    }
}

fn cell_class(selected: bool, formula_error: bool, merged: bool, editing: bool) -> &'static str {
    match (selected, formula_error, merged, editing) {
        (_, true, true, true) => "grid-cell grid-cell-editor selected formula-error merged-cell",
        (_, true, false, true) => "grid-cell grid-cell-editor selected formula-error",
        (_, false, true, true) => "grid-cell grid-cell-editor selected merged-cell",
        (_, false, false, true) => "grid-cell grid-cell-editor selected",
        (true, true, true, false) => "grid-cell selected formula-error merged-cell",
        (true, true, false, false) => "grid-cell selected formula-error",
        (true, false, true, false) => "grid-cell selected merged-cell",
        (true, false, false, false) => "grid-cell selected",
        (false, true, true, false) => "grid-cell formula-error merged-cell",
        (false, true, false, false) => "grid-cell formula-error",
        (false, false, true, false) => "grid-cell merged-cell",
        (false, false, false, false) => "grid-cell",
    }
}

fn scroll_offset(index: usize, header_size: f64, geometry: &SparseAxisGeometry) -> f64 {
    if index == 0 {
        0.0
    } else {
        header_size + geometry.offset(index)
    }
}

fn contains(bounds: SheetRegionBoundsView, row: usize, col: usize) -> bool {
    row >= bounds.row_start
        && row < bounds.row_end
        && col >= bounds.col_start
        && col < bounds.col_end
}

fn is_editing_cell(
    editing: Option<(usize, usize, usize)>,
    sheet_index: usize,
    row: usize,
    col: usize,
) -> bool {
    editing == Some((sheet_index, row, col))
}

#[derive(Props, Clone, PartialEq)]
struct ImageLayerProps {
    sheet_index: usize,
    document_id: u64,
    revision: u64,
    row_geometry: Rc<SparseAxisGeometry>,
    column_geometry: Rc<SparseAxisGeometry>,
    on_preview: EventHandler<String>,
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
                    let rect = image_rect(
                        &image.anchor,
                        &props.row_geometry,
                        &props.column_geometry,
                    );
                    let image_id = image.id.clone();
                    let select_id = image_id.clone();
                    let delete_id = image_id.clone();
                    let preview_id = image_id.clone();
                    let keyboard_id = image_id.clone();
                    let is_selected = selected.as_deref() == Some(image_id.as_str());
                    let source = assets.get(&image_id).cloned();
                    let can_preview = source.is_some();
                    let click_preview = props.on_preview;
                    let keyboard_preview = props.on_preview;
                    rsx! {
                        div {
                            key: "{image_id}",
                            class: if is_selected { "sheet-image selected" } else { "sheet-image" },
                            role: "button",
                            tabindex: 0,
                            aria_label: "Embedded image",
                            style: "left: {rect.left}px; top: {rect.top}px; width: {rect.width}px; height: {rect.height}px; z-index: {image.z_index + 10};",
                            onclick: move |event| {
                                event.stop_propagation();
                                store.selected_image.set(Some(select_id.clone()));
                            },
                            ondoubleclick: move |event| {
                                event.stop_propagation();
                                if can_preview {
                                    click_preview.call(preview_id.clone());
                                }
                            },
                            onkeydown: move |event: Event<KeyboardData>| {
                                if event.key() == Key::Enter && can_preview {
                                    event.prevent_default();
                                    keyboard_preview.call(keyboard_id.clone());
                                }
                            },
                            if let Some(source) = source {
                                img {
                                    src: source.as_ref(),
                                    alt: "Embedded workbook image",
                                    draggable: false,
                                }
                            } else {
                                div { class: "image-placeholder", "Image" }
                            }
                            if is_selected {
                                Button {
                                    class: "image-delete",
                                    variant: ButtonVariant::Destructive,
                                    size: ButtonSize::IconXs,
                                    aria_label: "Delete image",
                                    onclick: {
                                        let ports = Rc::clone(&ports);
                                        move |event: Event<MouseData>| {
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

#[derive(Props, Clone, PartialEq)]
struct ImagePreviewProps {
    source: Rc<str>,
    on_close: EventHandler<()>,
}

#[component]
fn ImagePreview(props: ImagePreviewProps) -> Element {
    rsx! {
        Dialog {
            class: "image-preview-dialog",
            open: Some(true),
            on_open_change: move |open: bool| {
                if !open {
                    props.on_close.call(());
                }
            },
            DialogTitle { class: "visually-hidden", "Image preview" }
            Button {
                class: "icon-button image-preview-close",
                variant: ButtonVariant::Ghost,
                size: ButtonSize::IconLg,
                aria_label: "Close image preview",
                onclick: move |_| props.on_close.call(()),
                X { size: 20 }
            }
            div {
                class: "image-preview-content",
                img {
                    src: props.source.as_ref(),
                    alt: "Workbook image preview",
                    draggable: false,
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

fn image_rect(
    anchor: &ImageAnchorDto,
    rows: &SparseAxisGeometry,
    columns: &SparseAxisGeometry,
) -> ImageRect {
    match anchor {
        ImageAnchorDto::OneCell {
            from,
            width_emu,
            height_emu,
        } => ImageRect {
            left: HEADER_WIDTH
                + columns.offset(from.col as usize)
                + f64::from(from.col_offset_emu) / EMU_PER_PIXEL,
            top: HEADER_HEIGHT
                + rows.offset(from.row as usize)
                + f64::from(from.row_offset_emu) / EMU_PER_PIXEL,
            width: (f64::from(*width_emu) / EMU_PER_PIXEL).max(24.0),
            height: (f64::from(*height_emu) / EMU_PER_PIXEL).max(24.0),
        },
        ImageAnchorDto::TwoCell { from, to } => {
            let left = HEADER_WIDTH
                + columns.offset(from.col as usize)
                + f64::from(from.col_offset_emu) / EMU_PER_PIXEL;
            let top = HEADER_HEIGHT
                + rows.offset(from.row as usize)
                + f64::from(from.row_offset_emu) / EMU_PER_PIXEL;
            let right = HEADER_WIDTH
                + columns.offset(to.col as usize)
                + f64::from(to.col_offset_emu) / EMU_PER_PIXEL;
            let bottom = HEADER_HEIGHT
                + rows.offset(to.row as usize)
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
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn enter_moves_down_until_the_last_row() {
        assert_eq!(cell_after_enter(2, 4, 5, None, &[]), Some((3, 4)));
        assert_eq!(cell_after_enter(4, 4, 5, None, &[]), None);
    }

    #[test]
    fn merged_cell_uses_the_full_range_geometry_across_a_clipped_viewport() {
        let columns = SparseAxisGeometry::new(120.0, &HashMap::from([(0, 80), (1, 140)]));
        let rows = SparseAxisGeometry::new(30.0, &HashMap::from([(0, 20), (1, 40)]));
        let merge = merge(0, 0, 1, 1);

        let items = build_grid_cell_items(1, 2, 1, 2, &[merge], &rows, &columns);

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
        let rows = SparseAxisGeometry::new(30.0, &HashMap::new());
        let columns = SparseAxisGeometry::new(120.0, &HashMap::new());

        let items = build_grid_cell_items(0, 3, 0, 3, &[merge], &rows, &columns);

        assert_eq!(items.len(), 6);
        assert_eq!(items.iter().filter(|item| item.merge.is_some()).count(), 1);
        assert!(
            !items
                .iter()
                .any(|item| item.merge.is_none() && merge.contains(item.row, item.col))
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

    #[test]
    fn filtered_grid_uses_visible_ordinals_with_physical_cell_coordinates() {
        let rows = SparseAxisGeometry::new(30.0, &HashMap::new());
        let columns = SparseAxisGeometry::new(120.0, &HashMap::new());
        let visible_rows = [0, 3, 7];

        let items = build_grid_cell_items_mapped(&visible_rows, 0, 0, 1, &[], &rows, &columns);

        assert_eq!(
            items.iter().map(|item| item.row).collect::<Vec<_>>(),
            visible_rows
        );
        assert_eq!(items[1].top, HEADER_HEIGHT + 30.0);
        assert_eq!(
            cell_after_enter_visible(0, 0, &visible_rows, None, &[]),
            Some((3, 0))
        );
    }

    #[test]
    fn only_the_active_coordinate_renders_as_an_editor() {
        let cells = [(0, 0), (0, 1), (1, 0), (1, 1)];
        let editor_count = cells
            .into_iter()
            .filter(|(row, col)| is_editing_cell(Some((2, 1, 0)), 2, *row, *col))
            .count();

        assert_eq!(editor_count, 1);
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
