use std::rc::Rc;

#[cfg(feature = "mobile")]
use dioxus::prelude::ReadableExt;
use dioxus::prelude::WritableExt;
#[cfg(not(feature = "mobile"))]
use simple_table_web_protocol::{WebWorkspaceReply, WebWorkspaceRequest};

use super::images::refresh_images;
use super::mutation::flush_pending_edits_locked;
use super::recovery;
use super::region_loader::RegionLoader;
use super::shared::{document_identity, document_name, unexpected_reply};
#[cfg(any(feature = "web", feature = "mobile"))]
use crate::model::LocalDocumentSummary;
use crate::model::{AppPorts, EditorStore, OpenDocumentView, SavedDocumentView, request_id};
use crate::protocol::{EditorCommand, EditorReply, EditorRequest};

#[cfg(feature = "mobile")]
const MOBILE_RECOVERY_ID: &str = "mobile-recovery";

pub async fn new_document(store: EditorStore, ports: Rc<AppPorts>) -> bool {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Creating workbook");
    match ports
        .editor
        .execute(EditorRequest::NewDocument {
            request_id: request_id("new"),
        })
        .await
    {
        Ok(reply) => {
            let opened = accept_document_reply(store, &ports.regions, reply);
            if opened {
                refresh_images(store, Rc::clone(&ports)).await;
                recovery::schedule(store, Rc::clone(&ports));
            }
            opened
        }
        Err(error) => {
            store.set_error(error);
            false
        }
    }
}

pub async fn open_bytes(
    store: EditorStore,
    ports: Rc<AppPorts>,
    file_name: String,
    bytes: Vec<u8>,
) -> bool {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Reading workbook");
    match ports
        .editor
        .execute_command(EditorCommand::with_attachment(
            EditorRequest::OpenDocument {
                request_id: request_id("open"),
                file_name,
            },
            bytes,
        ))
        .await
    {
        Ok(output) => {
            let opened = accept_document_reply(store, &ports.regions, output.reply);
            if opened {
                refresh_images(store, Rc::clone(&ports)).await;
                recovery::schedule(store, Rc::clone(&ports));
            }
            opened
        }
        Err(error) => {
            store.set_error(error);
            false
        }
    }
}

pub async fn open_local(store: EditorStore, ports: Rc<AppPorts>, document_key: String) -> bool {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Opening local workbook");

    #[cfg(feature = "mobile")]
    let response = if document_key == MOBILE_RECOVERY_ID {
        match ports.recovery.load().await {
            Ok(Some(recovery)) => ports
                .editor
                .execute_command(EditorCommand::with_attachment(
                    EditorRequest::OpenRecoveryDocument {
                        request_id: request_id("open-recovery"),
                        file_name: recovery.name,
                    },
                    recovery.bytes,
                ))
                .await
                .map(|output| output.reply),
            Ok(None) => Err(action_error(
                "mobile_recovery_missing",
                "the recovered workbook is no longer available",
            )),
            Err(error) => Err(error),
        }
    } else {
        Err(action_error(
            "mobile_recovery_missing",
            "the requested mobile workbook is unavailable",
        ))
    };

    #[cfg(not(feature = "mobile"))]
    let response = ports
        .workspace
        .execute(WebWorkspaceRequest::OpenLocalDocument {
            request_id: request_id("open-local"),
            document_key,
        })
        .await
        .and_then(|reply| match reply {
            WebWorkspaceReply::Document(value) => Ok(EditorReply::Document { value: Some(value) }),
            _ => Err(unexpected_reply("open local document")),
        });

    match response {
        Ok(reply) => {
            let opened = accept_document_reply(store, &ports.regions, reply);
            if opened {
                refresh_images(store, Rc::clone(&ports)).await;
                recovery::schedule(store, Rc::clone(&ports));
            }
            opened
        }
        Err(error) => {
            store.set_error(error);
            false
        }
    }
}

pub async fn load_local_documents(store: EditorStore, ports: Rc<AppPorts>) {
    #[cfg(feature = "web")]
    match ports
        .workspace
        .execute(WebWorkspaceRequest::ListLocalDocuments)
        .await
    {
        Ok(WebWorkspaceReply::LocalDocuments(documents)) => {
            let mut store = store;
            store.local_documents.set(
                documents
                    .into_iter()
                    .map(|document| LocalDocumentSummary {
                        id: document.id,
                        name: document.name,
                        updated_at_ms: document.updated_at_ms,
                        has_recovery: document.has_recovery,
                    })
                    .collect(),
            );
        }
        Err(error) if error.code != "client_not_hydrated" => store.set_error(error),
        _ => {}
    }

    #[cfg(feature = "mobile")]
    match ports.recovery.load().await {
        Ok(Some(recovery)) => {
            let mut store = store;
            store.local_documents.set(vec![LocalDocumentSummary {
                id: MOBILE_RECOVERY_ID.to_string(),
                name: recovery.name,
                updated_at_ms: recovery.updated_at_ms,
                has_recovery: true,
            }]);
        }
        Ok(None) => {
            let mut store = store;
            store.local_documents.set(Vec::new());
        }
        Err(error) => store.set_error(error),
    }

    #[cfg(not(any(feature = "web", feature = "mobile")))]
    let _ = (store, ports);
}

pub async fn delete_local_document(store: EditorStore, ports: Rc<AppPorts>, document_key: String) {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Removing local workbook");

    #[cfg(feature = "mobile")]
    let response = if document_key == MOBILE_RECOVERY_ID {
        ports.recovery.clear().await.map(|()| EditorReply::Empty)
    } else {
        Err(action_error(
            "mobile_recovery_missing",
            "the requested mobile workbook is unavailable",
        ))
    };

    #[cfg(not(feature = "mobile"))]
    let response = ports
        .workspace
        .execute(WebWorkspaceRequest::DeleteLocalDocument {
            document_key: document_key.clone(),
        })
        .await
        .and_then(|reply| match reply {
            WebWorkspaceReply::Empty => Ok(EditorReply::Empty),
            _ => Err(unexpected_reply("delete local document")),
        });

    match response {
        Ok(EditorReply::Empty) => {
            let mut store = store;
            store
                .local_documents
                .write()
                .retain(|document| document.id != document_key);
            store.status.set("Local workbook removed".to_string());
        }
        Ok(_) => store.set_error(unexpected_reply("delete local document")),
        Err(error) => store.set_error(error),
    }
}

pub async fn save_local(store: EditorStore, ports: Rc<AppPorts>) {
    save_local_to_target(store, ports, None).await;
}

#[cfg(feature = "mobile")]
pub async fn save_local_as(store: EditorStore, ports: Rc<AppPorts>, target_name: String) {
    save_local_to_target(store, ports, Some(target_name)).await;
}

async fn save_local_to_target(
    store: EditorStore,
    ports: Rc<AppPorts>,
    target_name: Option<String>,
) {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Saving workbook");
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    let target_name = target_name.unwrap_or_else(|| document_name(store));
    #[cfg(feature = "mobile")]
    let existing_target = document_path(store);
    #[cfg(feature = "web")]
    let result = ports
        .workspace
        .execute(WebWorkspaceRequest::SaveLocal {
            request_id: request_id("save-local"),
            document_id,
            base_revision,
            target_name,
        })
        .await
        .and_then(|reply| match reply {
            WebWorkspaceReply::Saved(value) => Ok(EditorReply::Saved { value }),
            WebWorkspaceReply::Empty => Ok(EditorReply::Empty),
            _ => Err(unexpected_reply("save local document")),
        });

    #[cfg(feature = "desktop")]
    let result = save_native(Rc::clone(&ports), document_id, base_revision, target_name).await;

    #[cfg(all(feature = "mobile", not(feature = "desktop")))]
    let result = save_mobile(
        Rc::clone(&ports),
        document_id,
        base_revision,
        target_name,
        existing_target,
    )
    .await;

    #[cfg(all(
        feature = "server",
        not(any(feature = "web", feature = "desktop", feature = "mobile"))
    ))]
    let result = {
        let _ = (document_id, base_revision, target_name);
        Err(crate::protocol::AppErrorDto {
            code: "client_not_hydrated".to_string(),
            message: "save is unavailable during SSR".to_string(),
        })
    };

    match result {
        Ok(EditorReply::Saved { value }) => {
            accept_saved_document(store, value.into());
            recovery::mark_healthy(store);
            #[cfg(feature = "mobile")]
            recovery::schedule_cleanup(store, Rc::clone(&ports));
            let mut store = store;
            store.status.set("Saved".to_string());
        }
        Ok(EditorReply::Empty) => {
            let mut store = store;
            store.status.set("Save cancelled".to_string());
        }
        Ok(_) => store.set_error(unexpected_reply("save")),
        Err(error) => store.set_error(error),
    }
}

#[cfg(feature = "desktop")]
async fn save_native(
    ports: Rc<AppPorts>,
    document_id: u64,
    base_revision: u64,
    target_name: String,
) -> Result<EditorReply, crate::protocol::AppErrorDto> {
    let Some(path) = ports
        .files
        .choose_document_path(target_name, crate::ports::file::DocumentDialogMode::Save)
        .await?
    else {
        return Ok(EditorReply::Empty);
    };
    let save_token = request_id("save-native");
    let prepared = ports
        .editor
        .execute_command(EditorCommand::new(EditorRequest::PrepareSave {
            request_id: save_token.clone(),
            document_id,
            base_revision,
            target_name: path.clone(),
        }))
        .await?;
    let EditorReply::SavePrepared {
        save_token,
        file_name,
    } = prepared.reply
    else {
        return Err(unexpected_reply("prepare save"));
    };
    let bytes = prepared
        .attachment
        .ok_or_else(|| unexpected_reply("prepare save bytes"))?;
    let path = path_for_prepared_name(path, &file_name);
    if let Err(error) = ports
        .files
        .write_document_to_path(path.clone(), bytes)
        .await
    {
        let _ = ports
            .editor
            .execute(EditorRequest::AbortSave { save_token })
            .await;
        return Err(error);
    }
    ports
        .editor
        .execute(EditorRequest::CommitSave { save_token, path })
        .await
}

#[cfg(all(feature = "mobile", not(feature = "desktop")))]
async fn save_mobile(
    ports: Rc<AppPorts>,
    document_id: u64,
    base_revision: u64,
    target_name: String,
    existing_target: Option<String>,
) -> Result<EditorReply, crate::protocol::AppErrorDto> {
    let save_token = request_id("save-mobile");
    let prepared = ports
        .editor
        .execute_command(EditorCommand::new(EditorRequest::PrepareSave {
            request_id: save_token.clone(),
            document_id,
            base_revision,
            target_name,
        }))
        .await?;
    let EditorReply::SavePrepared {
        save_token,
        file_name,
    } = prepared.reply
    else {
        return Err(unexpected_reply("prepare mobile save"));
    };
    let bytes = prepared
        .attachment
        .ok_or_else(|| unexpected_reply("prepare mobile save bytes"))?;
    let path = match ports
        .files
        .write_document_to_target(existing_target, file_name, bytes)
        .await
    {
        Ok(Some(path)) => path,
        Ok(None) => {
            let _ = ports
                .editor
                .execute(EditorRequest::AbortSave { save_token })
                .await;
            return Ok(EditorReply::Empty);
        }
        Err(error) => {
            let _ = ports
                .editor
                .execute(EditorRequest::AbortSave { save_token })
                .await;
            return Err(error);
        }
    };
    ports
        .editor
        .execute(EditorRequest::CommitSave { save_token, path })
        .await
}

pub async fn download_copy(mut store: EditorStore, ports: Rc<AppPorts>) {
    if store.busy() {
        return;
    }
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Preparing copy");
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    let suggested_name = document_name(store);
    #[cfg(feature = "desktop")]
    let target_name = match ports
        .files
        .choose_document_path(
            suggested_name,
            crate::ports::file::DocumentDialogMode::Export,
        )
        .await
    {
        Ok(Some(path)) => path,
        Ok(None) => {
            store.status.set("Download cancelled".to_string());
            return;
        }
        Err(error) => {
            store.set_error(error);
            return;
        }
    };
    #[cfg(not(feature = "desktop"))]
    let target_name = suggested_name;
    match ports
        .editor
        .execute_command(EditorCommand::new(EditorRequest::PrepareExport {
            document_id,
            base_revision,
            target_name: target_name.clone(),
        }))
        .await
    {
        Ok(output) => {
            let EditorReply::ExportPrepared { file_name } = output.reply else {
                store.set_error(unexpected_reply("download"));
                return;
            };
            let Some(bytes) = output.attachment else {
                store.set_error(unexpected_reply("download bytes"));
                return;
            };
            #[cfg(feature = "desktop")]
            let write = {
                let path = path_for_prepared_name(target_name, &file_name);
                ports
                    .files
                    .write_document_to_path(path.clone(), bytes)
                    .await
                    .map(|()| Some(path))
            };
            #[cfg(not(feature = "desktop"))]
            let write = ports.files.write_document(file_name, bytes).await;
            match write {
                Ok(Some(_)) => store.status.set("Copy downloaded".to_string()),
                #[cfg(feature = "mobile")]
                Ok(None) => store.status.set("Copy sent to device".to_string()),
                #[cfg(not(feature = "mobile"))]
                Ok(None) => {}
                Err(error) => store.set_error(error),
            }
        }
        Err(error) => store.set_error(error),
    }
}

pub async fn close_document(mut store: EditorStore, ports: Rc<AppPorts>) -> bool {
    let _operation = ports.operations.lock().await;
    if document_identity(store).is_none() {
        return true;
    }
    let _busy = store.begin_operation("Closing workbook");
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return false;
    }
    let Some((document_id, base_revision)) = document_identity(store) else {
        return true;
    };
    match ports
        .editor
        .execute(EditorRequest::CloseDocument {
            request_id: request_id("close"),
            document_id,
            base_revision,
        })
        .await
    {
        Ok(EditorReply::Closed) => {
            store.document.set(None);
            store.region_cache.write().clear();
            ports.regions.reset();
            store.images.set(Rc::new(Vec::new()));
            store
                .image_assets
                .set(Rc::new(std::collections::HashMap::new()));
            store.pending_edits.write().clear();
            #[cfg(feature = "mobile")]
            recovery::schedule_cleanup(store, Rc::clone(&ports));
            #[cfg(not(feature = "mobile"))]
            recovery::mark_healthy(store);
            true
        }
        Ok(_) => {
            store.set_error(unexpected_reply("close"));
            false
        }
        Err(error) => {
            store.set_error(error);
            false
        }
    }
}

fn accept_document_reply(
    mut store: EditorStore,
    regions: &RegionLoader,
    reply: EditorReply,
) -> bool {
    let EditorReply::Document { value } = reply else {
        store.set_error(unexpected_reply("document"));
        return false;
    };
    let Some(value) = value else {
        regions.reset();
        store.document.set(None);
        store.region_cache.write().clear();
        return false;
    };
    regions.reset();
    store.accept_document(value.into());
    true
}

#[cfg(feature = "mobile")]
fn document_path(store: EditorStore) -> Option<String> {
    store
        .document
        .read()
        .as_ref()
        .map(|document| document.document.path.clone())
        .filter(|path| {
            #[cfg(target_os = "android")]
            return path.starts_with("content://");

            #[cfg(not(target_os = "android"))]
            !path.is_empty()
        })
}

fn accept_saved_document(mut store: EditorStore, saved: SavedDocumentView) {
    if let Some(document) = store.document.write().as_mut().map(Rc::make_mut) {
        merge_saved_document(document, saved);
    }
}

fn merge_saved_document(document: &mut OpenDocumentView, saved: SavedDocumentView) {
    if let Some(manifest) = saved.document {
        document.document = manifest;
    }
    if let Some(identity) = saved.identity {
        document.document.path = identity.path;
        document.document.file_name = identity.file_name;
    }
    document.editor_session = saved.editor_session;
}

#[cfg(feature = "desktop")]
fn path_for_prepared_name(selected_path: String, prepared_name: &str) -> String {
    let mut path = std::path::PathBuf::from(&selected_path);
    if path.file_name().and_then(|name| name.to_str()) != Some(prepared_name) {
        path.set_file_name(prepared_name);
    }
    path.to_string_lossy().into_owned()
}

#[cfg(feature = "mobile")]
fn action_error(code: &str, message: &str) -> crate::protocol::AppErrorDto {
    crate::protocol::AppErrorDto {
        code: code.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(any(feature = "desktop", feature = "mobile"))]
    use std::cell::RefCell;

    use super::*;
    use crate::model::{
        DocumentManifestView, EditorSessionView, EditorStateView, SheetExtentView, SheetLayoutView,
        SheetManifestView,
    };
    #[cfg(any(feature = "desktop", feature = "mobile"))]
    use crate::ports::editor::{EditorPort, PortFuture};
    #[cfg(any(feature = "desktop", feature = "mobile"))]
    use crate::protocol::{EditorOutput, SavedDocumentResponse};

    fn session(revision: u64) -> EditorSessionView {
        EditorSessionView {
            document_id: 7,
            revision,
            editor_state: EditorStateView {
                can_undo: false,
                can_redo: false,
                is_dirty: false,
                history: Default::default(),
            },
            capabilities: Default::default(),
            formula_status: Default::default(),
            filters: Vec::new(),
        }
    }

    fn manifest(path: &str, file_name: &str) -> DocumentManifestView {
        DocumentManifestView {
            path: path.to_string(),
            file_name: file_name.to_string(),
            sheets: vec![SheetManifestView {
                name: "Sheet1".to_string(),
                extent: SheetExtentView {
                    row_count: 5,
                    column_count: 5,
                },
                layout: Rc::new(SheetLayoutView::default()),
            }],
        }
    }

    #[test]
    fn saved_identity_updates_document_name_path_and_session() {
        let mut document = OpenDocumentView {
            document: manifest("/tmp/old.xlsx", "old.xlsx"),
            editor_session: session(1),
            initial_region: None,
        };
        merge_saved_document(
            &mut document,
            SavedDocumentView {
                document: None,
                identity: Some(crate::model::SavedDocumentIdentityView {
                    path: "/tmp/new.csv".to_string(),
                    file_name: "new.csv".to_string(),
                }),
                editor_session: session(2),
            },
        );

        assert_eq!(document.document.path, "/tmp/new.csv");
        assert_eq!(document.document.file_name, "new.csv");
        assert_eq!(document.editor_session.revision, 2);
    }

    #[cfg(feature = "desktop")]
    struct RecordingSaveEditor {
        prepared_target: Rc<RefCell<Option<String>>>,
        committed_path: Rc<RefCell<Option<String>>>,
    }

    #[cfg(feature = "desktop")]
    impl EditorPort for RecordingSaveEditor {
        fn execute(
            &self,
            request: EditorRequest,
        ) -> PortFuture<Result<EditorReply, crate::protocol::AppErrorDto>> {
            let response = match request {
                EditorRequest::CommitSave { path, .. } => {
                    self.committed_path.replace(Some(path));
                    Ok(EditorReply::Saved {
                        value: saved_response(),
                    })
                }
                request => panic!("unexpected request: {request:?}"),
            };
            Box::pin(async move { response })
        }

        fn execute_command(
            &self,
            command: EditorCommand,
        ) -> PortFuture<crate::protocol::EditorResponse> {
            let EditorCommand {
                request,
                attachment,
            } = command;
            assert!(attachment.is_none());
            let EditorRequest::PrepareSave { target_name, .. } = request else {
                panic!("unexpected command request: {request:?}");
            };
            self.prepared_target.replace(Some(target_name));
            Box::pin(async {
                Ok(EditorOutput::with_attachment(
                    EditorReply::SavePrepared {
                        save_token: "save-token".to_string(),
                        file_name: "selected.xlsx".to_string(),
                    },
                    vec![1, 2, 3],
                ))
            })
        }
    }

    #[cfg(any(feature = "desktop", feature = "mobile"))]
    fn saved_response() -> SavedDocumentResponse {
        SavedDocumentResponse {
            document: None,
            identity: None,
            editor_session: crate::protocol::EditorSessionInfo {
                document_id: 7,
                revision: 2,
                editor_state: crate::protocol::EditorStateInfo {
                    can_undo: false,
                    can_redo: false,
                    is_dirty: false,
                    history: Default::default(),
                },
                capabilities: Default::default(),
                formula_status: crate::protocol::FormulaStatus::Ready {
                    diagnostics: Default::default(),
                },
                filters: Vec::new(),
            },
        }
    }

    #[cfg(feature = "desktop")]
    struct RecordingFilePort {
        written_path: Rc<RefCell<Option<String>>>,
    }

    #[cfg(feature = "desktop")]
    impl crate::ports::file::FilePort for RecordingFilePort {
        fn choose_document_path(
            &self,
            _suggested_name: String,
            _mode: crate::ports::file::DocumentDialogMode,
        ) -> crate::ports::file::FileFuture<Result<Option<String>, crate::protocol::AppErrorDto>>
        {
            Box::pin(async { Ok(Some("/tmp/selected".to_string())) })
        }

        fn write_document_to_path(
            &self,
            path: String,
            bytes: Vec<u8>,
        ) -> crate::ports::file::FileFuture<Result<(), crate::protocol::AppErrorDto>> {
            assert_eq!(bytes, vec![1, 2, 3]);
            self.written_path.replace(Some(path));
            Box::pin(async { Ok(()) })
        }
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn native_save_prepares_and_commits_the_selected_target() {
        let prepared_target = Rc::new(RefCell::new(None));
        let committed_path = Rc::new(RefCell::new(None));
        let written_path = Rc::new(RefCell::new(None));
        let editor: Rc<dyn EditorPort> = Rc::new(RecordingSaveEditor {
            prepared_target: Rc::clone(&prepared_target),
            committed_path: Rc::clone(&committed_path),
        });
        let ports = Rc::new(AppPorts {
            regions: RegionLoader::new(Rc::clone(&editor)),
            editor,
            files: Rc::new(RecordingFilePort {
                written_path: Rc::clone(&written_path),
            }),
            update: crate::ports::update::platform_update_port(),
            window: crate::ports::window::platform_window_port(),
            workspace: crate::ports::workspace::platform_workspace_port(),
            operations: Rc::new(futures::lock::Mutex::new(())),
        });

        futures::executor::block_on(save_native(ports, 7, 1, "old.csv".to_string())).unwrap();

        assert_eq!(prepared_target.borrow().as_deref(), Some("/tmp/selected"));
        assert_eq!(written_path.borrow().as_deref(), Some("/tmp/selected.xlsx"));
        assert_eq!(
            committed_path.borrow().as_deref(),
            Some("/tmp/selected.xlsx")
        );
    }

    #[cfg(feature = "mobile")]
    struct RecordingMobileSaveEditor {
        prepared_target: Rc<RefCell<Option<String>>>,
        committed_path: Rc<RefCell<Option<String>>>,
    }

    #[cfg(feature = "mobile")]
    impl EditorPort for RecordingMobileSaveEditor {
        fn execute(
            &self,
            request: EditorRequest,
        ) -> PortFuture<Result<EditorReply, crate::protocol::AppErrorDto>> {
            let response = match request {
                EditorRequest::CommitSave { path, .. } => {
                    self.committed_path.replace(Some(path));
                    Ok(EditorReply::Saved {
                        value: saved_response(),
                    })
                }
                request => panic!("unexpected request: {request:?}"),
            };
            Box::pin(async move { response })
        }

        fn execute_command(
            &self,
            command: EditorCommand,
        ) -> PortFuture<crate::protocol::EditorResponse> {
            let EditorCommand {
                request,
                attachment,
            } = command;
            assert!(attachment.is_none());
            let EditorRequest::PrepareSave { target_name, .. } = request else {
                panic!("unexpected command request: {request:?}");
            };
            self.prepared_target.replace(Some(target_name));
            Box::pin(async {
                Ok(EditorOutput::with_attachment(
                    EditorReply::SavePrepared {
                        save_token: "save-token".to_string(),
                        file_name: "quarterly plan.xlsx".to_string(),
                    },
                    vec![1, 2, 3],
                ))
            })
        }
    }

    #[cfg(feature = "mobile")]
    struct RecordingMobileFilePort {
        written_name: Rc<RefCell<Option<String>>>,
    }

    #[cfg(feature = "mobile")]
    impl crate::ports::file::FilePort for RecordingMobileFilePort {
        fn pick_file(
            &self,
            _kind: crate::ports::file::MobileFileKind,
        ) -> crate::ports::file::FileFuture<
            Result<Option<crate::ports::file::PickedFile>, crate::protocol::AppErrorDto>,
        > {
            Box::pin(async { Ok(None) })
        }

        fn write_document(
            &self,
            _suggested_name: String,
            _bytes: Vec<u8>,
        ) -> crate::ports::file::FileFuture<Result<Option<String>, crate::protocol::AppErrorDto>>
        {
            Box::pin(async { Ok(None) })
        }

        fn write_document_to_target(
            &self,
            existing_target: Option<String>,
            suggested_name: String,
            bytes: Vec<u8>,
        ) -> crate::ports::file::FileFuture<Result<Option<String>, crate::protocol::AppErrorDto>>
        {
            assert_eq!(existing_target, None);
            assert_eq!(bytes, vec![1, 2, 3]);
            self.written_name.replace(Some(suggested_name));
            Box::pin(async { Ok(Some("content://documents/quarterly-plan".to_string())) })
        }
    }

    #[cfg(feature = "mobile")]
    #[test]
    fn mobile_save_uses_the_requested_name_and_commits_the_written_target() {
        let prepared_target = Rc::new(RefCell::new(None));
        let committed_path = Rc::new(RefCell::new(None));
        let written_name = Rc::new(RefCell::new(None));
        let editor: Rc<dyn EditorPort> = Rc::new(RecordingMobileSaveEditor {
            prepared_target: Rc::clone(&prepared_target),
            committed_path: Rc::clone(&committed_path),
        });
        let ports = Rc::new(AppPorts {
            regions: RegionLoader::new(Rc::clone(&editor)),
            editor,
            files: Rc::new(RecordingMobileFilePort {
                written_name: Rc::clone(&written_name),
            }),
            update: crate::ports::update::platform_update_port(),
            window: crate::ports::window::platform_window_port(),
            recovery: crate::ports::recovery::platform_recovery_port(),
            operations: Rc::new(futures::lock::Mutex::new(())),
        });

        futures::executor::block_on(save_mobile(ports, 7, 1, "quarterly plan".to_string(), None))
            .unwrap();

        assert_eq!(prepared_target.borrow().as_deref(), Some("quarterly plan"));
        assert_eq!(
            written_name.borrow().as_deref(),
            Some("quarterly plan.xlsx")
        );
        assert_eq!(
            committed_path.borrow().as_deref(),
            Some("content://documents/quarterly-plan")
        );
    }
}
