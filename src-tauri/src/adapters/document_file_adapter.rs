use std::path::{Path, PathBuf};

use crate::application::document_codec_port::OpenDocumentSource;
use crate::application::document_file_workflow::{
    DocumentExportTargetPort, DocumentOpenSourcePort, DocumentSaveTargetPort, StagedDocumentWrite,
};
use crate::application::prepared_source_port::{
    NoopPreparedSourceAdoption, PreparedSourceAdoption, PreparedSourceAdoptionPort,
};
use crate::error::AppError;
use crate::io::atomic_file::{cleanup_temp_file, replace_temp_file, write_temp_file_for_target};
#[cfg(any(desktop, target_os = "android", target_os = "ios"))]
use crate::io::open_file_input::{OpenFileInput, OpenFileSelection};
#[cfg(desktop)]
use crate::io::platform::desktop::{self, DesktopFileRuntime};
#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
use crate::io::platform::mobile::MobileFileRuntime;
#[cfg(any(target_os = "android", target_os = "ios"))]
use crate::io::platform::mobile::{self, MobileFileRuntime};
#[cfg(desktop)]
use crate::recent::store::RecentStore;

#[derive(Clone)]
pub struct PlatformFileAdapter {
    #[cfg(desktop)]
    recent_files: RecentStore,
    #[cfg(desktop)]
    desktop_files: DesktopFileRuntime,
    #[cfg(any(target_os = "android", target_os = "ios", test))]
    mobile_files: MobileFileRuntime,
}

impl PlatformFileAdapter {
    pub(crate) fn new(
        #[cfg(desktop)] recent_files: RecentStore,
        #[cfg(desktop)] desktop_files: DesktopFileRuntime,
        #[cfg(any(target_os = "android", target_os = "ios", test))] mobile_files: MobileFileRuntime,
    ) -> Self {
        Self {
            #[cfg(desktop)]
            recent_files,
            #[cfg(desktop)]
            desktop_files,
            #[cfg(any(target_os = "android", target_os = "ios", test))]
            mobile_files,
        }
    }

    #[cfg(desktop)]
    pub fn enqueue_open_target(&self, target: &str) -> bool {
        desktop::enqueue_open_target(&self.desktop_files, target)
    }

    #[cfg(desktop)]
    pub fn claim_pending_open_target(&self) -> Result<Option<desktop::OpenTargetClaim>, AppError> {
        desktop::claim_pending_open_target(&self.desktop_files)
    }

    #[cfg(desktop)]
    pub fn acknowledge_open_target(&self, claim_id: &str) -> Result<bool, AppError> {
        desktop::acknowledge_open_target(&self.desktop_files, claim_id)
    }

    #[cfg(desktop)]
    pub fn release_open_target(&self, claim_id: &str) -> Result<bool, AppError> {
        desktop::release_open_target(&self.desktop_files, claim_id)
    }

    #[cfg(desktop)]
    pub fn pick_open_file(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Option<OpenFileSelection>, AppError> {
        desktop::pick_open_file(&self.desktop_files, app)
    }

    #[cfg(desktop)]
    pub fn discard_open_file_selection(&self, path: &str) {
        desktop::discard_open_file_selection(&self.desktop_files, path);
    }

    #[cfg(desktop)]
    pub(crate) fn open_source(&self, path: String) -> Box<dyn DocumentOpenSourcePort> {
        let files = self.desktop_files.clone();
        boxed_open_source(move || desktop::read_open_file(&files, &path))
    }

    #[cfg(desktop)]
    pub(crate) fn recent_open_source(
        &self,
        app: tauri::AppHandle,
        id: String,
    ) -> Box<dyn DocumentOpenSourcePort> {
        let recent_files = self.recent_files.clone();
        boxed_open_source(move || {
            let recent = recent_files
                .get_all(&app)?
                .into_iter()
                .find(|file| file.id == id)
                .ok_or(AppError::FileNotFound(id))?;
            desktop::read_file_trusted(&recent.path)
        })
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
    pub(crate) fn save_target(&self, path: String) -> Box<dyn DocumentSaveTargetPort> {
        Box::new(DesktopSaveTarget {
            files: self.desktop_files.clone(),
            path,
        })
    }

    #[cfg(desktop)]
    pub(crate) fn pick_export_target(
        &self,
        app: &tauri::AppHandle,
        default_name: &str,
    ) -> Result<Option<Box<dyn DocumentExportTargetPort>>, AppError> {
        Ok(
            desktop::pick_export_target(app, default_name)?.map(|target| {
                let target_path_or_name = target.target_path_or_name.clone();
                Box::new(ClosureExportTarget {
                    target_path_or_name,
                    write: Box::new(move |bytes| {
                        desktop::write_export_target(&target, bytes)?;
                        Ok(target.path_string)
                    }),
                }) as Box<dyn DocumentExportTargetPort>
            }),
        )
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub fn reconcile_transient_files(&self, app: &tauri::AppHandle) -> Result<(), AppError> {
        mobile::reconcile_transient_files(&self.mobile_files, app)
    }

    #[cfg(target_os = "android")]
    pub fn pick_open_file_android(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Option<OpenFileSelection>, AppError> {
        crate::io::platform::android::pick_file_info(&self.mobile_files, app)
    }

    #[cfg(target_os = "ios")]
    pub fn pick_open_file_ios(
        &self,
        app: &tauri::AppHandle,
    ) -> Result<Option<OpenFileSelection>, AppError> {
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
    pub(crate) fn mobile_open_source(
        &self,
        app: tauri::AppHandle,
        path: String,
    ) -> Box<dyn DocumentOpenSourcePort> {
        let files = self.mobile_files.clone();
        boxed_open_source(move || mobile::read_open_file(&files, &app, &path))
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
    pub(crate) fn mobile_save_target(
        &self,
        app: tauri::AppHandle,
        path: String,
    ) -> Result<Box<dyn DocumentSaveTargetPort>, AppError> {
        let target =
            mobile::validated_mobile_files_path(&self.mobile_files, &app, Path::new(&path))?;
        Ok(Box::new(MobileSaveTarget {
            files: self.mobile_files.clone(),
            path: target.to_string_lossy().to_string(),
            target,
        }))
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub(crate) fn pick_mobile_export_target(
        &self,
        app: &tauri::AppHandle,
        default_name: &str,
    ) -> Result<Option<Box<dyn DocumentExportTargetPort>>, AppError> {
        let app = app.clone();
        Ok(
            mobile::pick_export_target(&app, default_name)?.map(|target| {
                let target_path_or_name = target.target_path_or_name.clone();
                Box::new(ClosureExportTarget {
                    target_path_or_name,
                    write: Box::new(move |bytes| {
                        mobile::write_export_target(&app, &target, bytes)?;
                        Ok(target.destination_string)
                    }),
                }) as Box<dyn DocumentExportTargetPort>
            }),
        )
    }

    #[cfg(test)]
    pub(crate) fn is_isolated_from(&self, other: &Self) -> bool {
        #[cfg(desktop)]
        let desktop_isolated = !self.desktop_files.is_same_instance(&other.desktop_files);
        #[cfg(not(desktop))]
        let desktop_isolated = true;
        desktop_isolated && self.mobile_files.is_isolated_from(&other.mobile_files)
    }
}

impl PreparedSourceAdoptionPort for PlatformFileAdapter {
    fn begin_adoption(
        &self,
        source_path: Option<&Path>,
        file_name: &str,
    ) -> Result<Box<dyn PreparedSourceAdoption>, AppError> {
        #[cfg(any(target_os = "android", target_os = "ios", test))]
        {
            let Some(source_path) = source_path else {
                return Ok(Box::new(NoopPreparedSourceAdoption));
            };
            let adoption = self
                .mobile_files
                .begin_transient_document_adoption(source_path, file_name)?;
            Ok(Box::new(ManagedPreparedSourceAdoption(Some(adoption))))
        }
        #[cfg(not(any(target_os = "android", target_os = "ios", test)))]
        {
            let _ = (source_path, file_name);
            Ok(Box::new(NoopPreparedSourceAdoption))
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios", test))]
struct ManagedPreparedSourceAdoption(Option<crate::io::managed_documents::ManagedDocumentAdoption>);

#[cfg(any(target_os = "android", target_os = "ios", test))]
impl PreparedSourceAdoption for ManagedPreparedSourceAdoption {
    fn commit(mut self: Box<Self>) {
        if let Some(adoption) = self.0.take() {
            adoption.commit();
        }
    }
}

#[cfg(any(desktop, target_os = "android", target_os = "ios"))]
fn boxed_open_source(
    read: impl FnOnce() -> Result<OpenFileInput, AppError> + Send + 'static,
) -> Box<dyn DocumentOpenSourcePort> {
    Box::new(ClosureOpenSource {
        read: Some(Box::new(read)),
    })
}

#[cfg(any(desktop, target_os = "android", target_os = "ios"))]
struct ClosureOpenSource {
    read: Option<Box<dyn FnOnce() -> Result<OpenFileInput, AppError> + Send>>,
}

#[cfg(any(desktop, target_os = "android", target_os = "ios"))]
impl DocumentOpenSourcePort for ClosureOpenSource {
    fn read(mut self: Box<Self>) -> Result<OpenDocumentSource, AppError> {
        let input =
            self.read.take().ok_or_else(|| {
                AppError::Internal("open source was already consumed".to_string())
            })?()?;
        Ok(OpenDocumentSource {
            path: input.path,
            bytes: input.bytes,
            file_name: input.file_name,
        })
    }
}

#[cfg(desktop)]
struct DesktopSaveTarget {
    files: DesktopFileRuntime,
    path: String,
}

#[cfg(desktop)]
impl DocumentSaveTargetPort for DesktopSaveTarget {
    fn target_path(&self) -> &str {
        &self.path
    }

    fn ensure_authorized(&self, current_document_path: &str) -> Result<(), AppError> {
        desktop::ensure_save_path_authorized(&self.files, &self.path, current_document_path)
    }

    fn stage(
        self: Box<Self>,
        bytes: &[u8],
        _output_name: &str,
    ) -> Result<Box<dyn StagedDocumentWrite>, AppError> {
        let target = PathBuf::from(&self.path);
        let temp = write_temp_file_for_target(&target, bytes)?;
        Ok(Box::new(AtomicStagedWrite { temp, target }))
    }
}

#[cfg(desktop)]
struct AtomicStagedWrite {
    temp: PathBuf,
    target: PathBuf,
}

#[cfg(desktop)]
impl StagedDocumentWrite for AtomicStagedWrite {
    fn commit(self: Box<Self>) -> Result<(), AppError> {
        replace_temp_file(&self.temp, &self.target)
    }
}

#[cfg(desktop)]
impl Drop for AtomicStagedWrite {
    fn drop(&mut self) {
        cleanup_temp_file(&self.temp);
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
struct MobileSaveTarget {
    files: MobileFileRuntime,
    path: String,
    target: PathBuf,
}

#[cfg(any(target_os = "android", target_os = "ios"))]
impl DocumentSaveTargetPort for MobileSaveTarget {
    fn target_path(&self) -> &str {
        &self.path
    }

    fn ensure_authorized(&self, current_document_path: &str) -> Result<(), AppError> {
        mobile::ensure_save_target_authorized(&self.files, &self.target, current_document_path)
    }

    fn stage(
        self: Box<Self>,
        bytes: &[u8],
        output_name: &str,
    ) -> Result<Box<dyn StagedDocumentWrite>, AppError> {
        let transaction =
            self.files
                .begin_managed_save_transaction(&self.target, output_name, bytes)?;
        let temp = write_temp_file_for_target(&self.target, bytes)?;
        Ok(Box::new(MobileStagedWrite {
            temp,
            target: self.target,
            transaction: Some(transaction),
        }))
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
struct MobileStagedWrite {
    temp: PathBuf,
    target: PathBuf,
    transaction: Option<crate::io::managed_documents::ManagedSaveTransaction>,
}

#[cfg(any(target_os = "android", target_os = "ios"))]
impl StagedDocumentWrite for MobileStagedWrite {
    fn commit(mut self: Box<Self>) -> Result<(), AppError> {
        replace_temp_file(&self.temp, &self.target)?;
        if let Some(transaction) = self.transaction.take()
            && let Err(error) = transaction.finish_after_content_commit()
        {
            eprintln!(
                "Saved document content but deferred managed metadata recovery for {}: {error}",
                self.target.display()
            );
        }
        Ok(())
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
impl Drop for MobileStagedWrite {
    fn drop(&mut self) {
        cleanup_temp_file(&self.temp);
    }
}

type ExportWriter = Box<dyn FnOnce(&[u8]) -> Result<String, AppError> + Send>;

struct ClosureExportTarget {
    target_path_or_name: String,
    write: ExportWriter,
}

impl DocumentExportTargetPort for ClosureExportTarget {
    fn target_path_or_name(&self) -> &str {
        &self.target_path_or_name
    }

    fn write(self: Box<Self>, bytes: &[u8]) -> Result<String, AppError> {
        (self.write)(bytes)
    }
}
