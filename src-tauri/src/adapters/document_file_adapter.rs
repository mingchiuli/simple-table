use crate::application::document_codec_port::OpenDocumentSource;
use crate::application::document_open_service::{self, DocumentOpenService};
use crate::application::document_save_service::{self, DocumentSaveService};
use crate::error::AppError;
#[cfg(desktop)]
use crate::io::platform::desktop::{self, DesktopFileRuntime};
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::io::platform::mobile;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use crate::io::platform::mobile::MobileFileRuntime;
#[cfg(desktop)]
use crate::recent::store::RecentStore;
#[cfg(desktop)]
use crate::types::DesktopOpenFileInfo;
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::types::PickedFileInfo;
use crate::types::{PreparedOpenDocument, SavedDocumentResponse};

#[derive(Clone)]
pub struct DocumentFileAdapter {
    opens: DocumentOpenService,
    saves: DocumentSaveService,
    #[cfg(desktop)]
    recent_files: RecentStore,
    #[cfg(desktop)]
    desktop_files: DesktopFileRuntime,
    #[cfg(any(target_os = "android", target_os = "ios", test))]
    mobile_files: MobileFileRuntime,
}

impl DocumentFileAdapter {
    pub(crate) fn new(
        opens: DocumentOpenService,
        saves: DocumentSaveService,
        #[cfg(desktop)] recent_files: RecentStore,
        #[cfg(desktop)] desktop_files: DesktopFileRuntime,
        #[cfg(any(target_os = "android", target_os = "ios", test))] mobile_files: MobileFileRuntime,
    ) -> Self {
        Self {
            opens,
            saves,
            #[cfg(desktop)]
            recent_files,
            #[cfg(desktop)]
            desktop_files,
            #[cfg(any(target_os = "android", target_os = "ios", test))]
            mobile_files,
        }
    }

    #[cfg(desktop)]
    pub fn authorize_open_target(&self, target: &str) {
        desktop::authorize_open_target(&self.desktop_files, target);
    }

    #[cfg(desktop)]
    pub fn pick_open_file(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Option<DesktopOpenFileInfo>, AppError> {
        desktop::pick_open_file(&self.desktop_files, app)
    }

    #[cfg(desktop)]
    pub fn discard_open_file_selection(&self, path: &str) {
        desktop::discard_open_file_selection(&self.desktop_files, path);
    }

    #[cfg(desktop)]
    pub fn prepare_open_file(&self, path: &str) -> Result<PreparedOpenDocument, AppError> {
        prepare_open_file_desktop(&self.opens, &self.desktop_files, path)
    }

    #[cfg(desktop)]
    pub fn prepare_recent_file(
        &self,
        app: &tauri::AppHandle,
        id: &str,
    ) -> Result<PreparedOpenDocument, AppError> {
        prepare_recent_file_desktop(&self.opens, &self.recent_files, app, id)
    }

    #[cfg(desktop)]
    pub fn pick_save_location(
        &self,
        app: &tauri::AppHandle,
        default_name: &str,
    ) -> Result<Option<String>, AppError> {
        desktop::pick_save_location(&self.desktop_files, app, default_name)
    }

    #[cfg(desktop)]
    pub fn discard_save_location(&self, path: &str) {
        desktop::discard_save_location(&self.desktop_files, path);
    }

    #[cfg(desktop)]
    pub fn save_file(
        &self,
        path: &str,
        document_id: u64,
        base_revision: u64,
    ) -> Result<SavedDocumentResponse, AppError> {
        save_file_desktop(
            &self.saves,
            &self.desktop_files,
            path,
            document_id,
            base_revision,
        )
    }

    #[cfg(desktop)]
    pub fn export_file(
        &self,
        app: &tauri::AppHandle,
        default_name: &str,
        document_id: u64,
        base_revision: u64,
    ) -> Result<Option<String>, AppError> {
        export_file_desktop(&self.saves, app, default_name, document_id, base_revision)
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub fn reconcile_transient_files(&self, app: &tauri::AppHandle) -> Result<(), AppError> {
        mobile::reconcile_transient_files(&self.mobile_files, app)
    }

    #[cfg(target_os = "android")]
    pub fn pick_open_file_android(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Option<PickedFileInfo>, AppError> {
        crate::io::platform::android::pick_file_info(&self.mobile_files, app)
    }

    #[cfg(target_os = "ios")]
    pub fn pick_open_file_ios(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Option<PickedFileInfo>, AppError> {
        crate::io::platform::ios::pick_file_info(&self.mobile_files, app)
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub fn discard_open_file_selection_mobile(
        &self,
        app: &tauri::AppHandle,
        path: &str,
    ) -> Result<(), AppError> {
        mobile::discard_transient_file(
            &self.mobile_files,
            app,
            path,
            crate::io::transient_files::TransientFilePurpose::OpenSelection,
        )
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub fn discard_save_location_mobile(
        &self,
        app: &tauri::AppHandle,
        path: &str,
    ) -> Result<(), AppError> {
        mobile::discard_transient_file(
            &self.mobile_files,
            app,
            path,
            crate::io::transient_files::TransientFilePurpose::SaveLocation,
        )
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub fn prepare_open_file_mobile(
        &self,
        app: &tauri::AppHandle,
        path: &str,
    ) -> Result<PreparedOpenDocument, AppError> {
        prepare_open_file_mobile(&self.opens, &self.mobile_files, app, path)
    }

    #[cfg(target_os = "android")]
    pub fn pick_save_location_android(
        &self,
        app: &tauri::AppHandle,
        default_name: &str,
    ) -> Result<Option<String>, AppError> {
        Ok(Some(crate::io::platform::android::pick_save_location(
            &self.mobile_files,
            app,
            default_name,
        )?))
    }

    #[cfg(target_os = "ios")]
    pub fn pick_save_location_ios(
        &self,
        app: &tauri::AppHandle,
        default_name: &str,
    ) -> Result<Option<String>, AppError> {
        Ok(Some(mobile::reserve_save_location(
            &self.mobile_files,
            app,
            default_name,
        )?))
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub fn save_file_mobile(
        &self,
        app: &tauri::AppHandle,
        path: &str,
        document_id: u64,
        base_revision: u64,
    ) -> Result<SavedDocumentResponse, AppError> {
        save_file_mobile(
            &self.saves,
            &self.mobile_files,
            app,
            path,
            document_id,
            base_revision,
        )
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub fn export_file_mobile(
        &self,
        app: &tauri::AppHandle,
        default_name: &str,
        document_id: u64,
        base_revision: u64,
    ) -> Result<Option<String>, AppError> {
        export_file_mobile(
            &self.saves,
            &self.mobile_files,
            app,
            default_name,
            document_id,
            base_revision,
        )
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        #[cfg(desktop)]
        let desktop_isolated = !self.desktop_files.is_same_instance(&other.desktop_files);
        #[cfg(not(desktop))]
        let desktop_isolated = true;
        let mobile_isolated = self.mobile_files.is_isolated_from(&other.mobile_files);
        self.opens.is_isolated_from(&other.opens)
            && self.saves.is_isolated_from(&other.saves)
            && desktop_isolated
            && mobile_isolated
    }
}

#[cfg(desktop)]
pub fn prepare_open_file_desktop(
    service: &DocumentOpenService,
    files: &crate::io::platform::desktop::DesktopFileRuntime,
    path: &str,
) -> Result<PreparedOpenDocument, AppError> {
    let input = crate::io::platform::desktop::read_open_file(files, path)?;
    document_open_service::prepare_open_input(service, into_open_document_source(input))
}

#[cfg(desktop)]
pub fn prepare_recent_file_desktop(
    service: &DocumentOpenService,
    recent_files: &crate::recent::store::RecentStore,
    app: &tauri::AppHandle,
    id: &str,
) -> Result<PreparedOpenDocument, AppError> {
    let input = crate::io::platform::desktop::read_recent_file(recent_files, app, id)?;
    document_open_service::prepare_open_input(service, into_open_document_source(input))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn prepare_open_file_mobile(
    service: &DocumentOpenService,
    files: &crate::io::platform::mobile::MobileFileRuntime,
    app: &tauri::AppHandle,
    path: &str,
) -> Result<PreparedOpenDocument, AppError> {
    let input = crate::io::platform::mobile::read_open_file(files, app, path)?;
    document_open_service::prepare_open_input(service, into_open_document_source(input))
}

fn into_open_document_source(
    input: crate::io::open_file_input::OpenFileInput,
) -> OpenDocumentSource {
    OpenDocumentSource {
        path: input.path,
        bytes: input.bytes,
        file_name: input.file_name,
    }
}

#[cfg(desktop)]
pub fn save_file_desktop(
    service: &DocumentSaveService,
    files: &crate::io::platform::desktop::DesktopFileRuntime,
    path: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<SavedDocumentResponse, AppError> {
    use std::path::Path;

    use crate::io::atomic_file::{
        cleanup_temp_file, replace_temp_file, write_temp_file_for_target,
    };
    use crate::io::platform::desktop;

    let current_path = document_save_service::current_document_path_for_command(
        service,
        document_id,
        base_revision,
    )?;
    desktop::ensure_save_path_authorized(files, path, &current_path)?;
    let prepared = document_save_service::prepare_current_file_save(
        service,
        document_id,
        base_revision,
        path,
    )?;
    let target = Path::new(path);
    let temp_path = match write_temp_file_for_target(target, &prepared.bytes) {
        Ok(temp_path) => temp_path,
        Err(error) => {
            document_save_service::abort_prepared_file_save(prepared);
            return Err(error);
        }
    };

    let result = document_save_service::commit_current_file_save(
        service,
        path.to_string(),
        prepared,
        || replace_temp_file(&temp_path, target),
    );
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

#[cfg(desktop)]
pub fn export_file_desktop(
    service: &DocumentSaveService,
    app: &tauri::AppHandle,
    default_name: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<Option<String>, AppError> {
    use crate::io::platform::desktop;

    let Some(target) = desktop::pick_export_target(app, default_name)? else {
        return Ok(None);
    };
    let prepared = document_save_service::prepare_current_file_export(
        service,
        document_id,
        base_revision,
        &target.target_path_or_name,
    )?;
    desktop::write_export_target(&target, &prepared.bytes)?;
    Ok(Some(target.path_string))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn save_file_mobile(
    service: &DocumentSaveService,
    files: &crate::io::platform::mobile::MobileFileRuntime,
    app: &tauri::AppHandle,
    path: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<SavedDocumentResponse, AppError> {
    use std::path::Path;

    use crate::io::atomic_file::{
        cleanup_temp_file, replace_temp_file, write_temp_file_for_target,
    };
    use crate::io::managed_documents;
    use crate::io::platform::mobile;

    let target = mobile::validated_mobile_files_path(files, app, Path::new(path))?;
    let current_path = document_save_service::current_document_path_for_command(
        service,
        document_id,
        base_revision,
    )?;
    mobile::ensure_save_target_authorized(files, &target, &current_path)?;
    let target_path = target.to_string_lossy().to_string();
    let prepared = document_save_service::prepare_current_file_save(
        service,
        document_id,
        base_revision,
        &target_path,
    )?;
    managed_documents::validate_managed_save(
        files.managed_documents(),
        &target,
        prepared.bytes.len() as u64,
    )?;
    let temp_path = match write_temp_file_for_target(&target, &prepared.bytes) {
        Ok(temp_path) => temp_path,
        Err(error) => {
            document_save_service::abort_prepared_file_save(prepared);
            return Err(error);
        }
    };

    let managed_file_name = prepared.output_name.clone();
    let result =
        document_save_service::commit_current_file_save(service, target_path, prepared, || {
            replace_temp_file(&temp_path, &target)?;
            managed_documents::adopt_completed_save(
                files.managed_documents(),
                files.transient_files(),
                &target,
                &managed_file_name,
            )
        });
    if result.is_err() {
        cleanup_temp_file(&temp_path);
    }
    result
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn export_file_mobile(
    service: &DocumentSaveService,
    files: &crate::io::platform::mobile::MobileFileRuntime,
    app: &tauri::AppHandle,
    default_name: &str,
    document_id: u64,
    base_revision: u64,
) -> Result<Option<String>, AppError> {
    use crate::io::platform::mobile;

    let Some(target) = mobile::pick_export_target(app, default_name)? else {
        return Ok(None);
    };
    let prepared = document_save_service::prepare_current_file_export(
        service,
        document_id,
        base_revision,
        &target.target_path_or_name,
    )?;
    mobile::write_export_target(app, &target, &prepared.bytes)?;
    Ok(Some(target.destination_string))
}
