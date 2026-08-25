use std::rc::Rc;

use dioxus::prelude::ReadableExt;

use crate::model::{AppPorts, EditorStore};
use crate::protocol::{AppErrorDto, EditorReply, EditorRequest};

pub(super) async fn refresh_document(store: EditorStore, ports: Rc<AppPorts>) {
    match ports.editor.execute(EditorRequest::ActiveDocument).await {
        Ok(EditorReply::Document { value: Some(value) }) => store.refresh_document(value.into()),
        Ok(EditorReply::Document { .. }) => {}
        Ok(_) => store.set_error(unexpected_reply("document")),
        Err(error) => store.set_error(error),
    }
}

pub(super) fn document_identity(store: EditorStore) -> Option<(u64, u64)> {
    store.document.read().as_ref().map(|document| {
        (
            document.editor_session.document_id,
            document.editor_session.revision,
        )
    })
}

pub(super) fn document_name(store: EditorStore) -> String {
    store
        .document
        .read()
        .as_ref()
        .map(|document| document.document.file_name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "untitled.xlsx".to_string())
}

pub(super) fn active_sheet_name(store: EditorStore) -> Option<String> {
    let document = store.document.read();
    document
        .as_ref()?
        .document
        .sheets
        .get(store.active_sheet())
        .map(|sheet| sheet.name.clone())
}

pub(super) fn sheet_extent(
    store: EditorStore,
    sheet_index: usize,
) -> Option<crate::model::SheetExtentView> {
    store
        .document
        .peek()
        .as_ref()?
        .document
        .sheets
        .get(sheet_index)
        .map(|sheet| sheet.extent)
}

pub(super) fn schedule_current_window(store: EditorStore, ports: &AppPorts) {
    let sheet_index = store.active_sheet();
    let Some(extent) = sheet_extent(store, sheet_index) else {
        return;
    };
    let visible_rows = store.visible_rows(sheet_index, extent.row_count.max(1));
    let window =
        store
            .render_window
            .peek()
            .clamped(sheet_index, visible_rows.len(), extent.column_count);
    if visible_rows.len() == extent.row_count.max(1) {
        ports
            .regions
            .schedule_viewport(store, window.bounds(), extent);
    } else {
        ports.regions.schedule_visible_rows(
            store,
            sheet_index,
            visible_rows
                .get(window.row_start..window.row_end.min(visible_rows.len()))
                .unwrap_or_default(),
            window.col_start,
            window.col_end,
            extent,
        );
    }
}

pub(super) fn unexpected_reply(action: &str) -> AppErrorDto {
    AppErrorDto {
        code: "protocol_error".to_string(),
        message: format!("unexpected {action} response"),
    }
}
