#![cfg_attr(test, allow(dead_code))]

use crate::error::AppError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use std::sync::OnceLock;

#[cfg(any(target_os = "android", target_os = "ios", test))]
static TRANSIENT_FILE_REGISTRY: OnceLock<TransientFileRegistry> = OnceLock::new();

#[cfg(any(target_os = "android", target_os = "ios", test))]
pub fn transient_file_registry() -> &'static TransientFileRegistry {
    TRANSIENT_FILE_REGISTRY.get_or_init(TransientFileRegistry::default)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransientFilePurpose {
    OpenSelection,
    SaveLocation,
}

#[derive(Default)]
pub struct TransientFileRegistry {
    paths: Mutex<HashMap<PathBuf, TransientFilePurpose>>,
}

impl TransientFileRegistry {
    pub fn register(&self, path: PathBuf, purpose: TransientFilePurpose) -> Result<(), AppError> {
        let mut paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        if let Some(existing) = paths.get(&path) {
            if *existing == purpose {
                return Ok(());
            }
            return Err(AppError::DocumentStateInvalid(
                "transient file is already registered for a different purpose".to_string(),
            ));
        }
        paths.insert(path, purpose);
        Ok(())
    }

    pub fn take(&self, path: &Path, purpose: TransientFilePurpose) -> Result<PathBuf, AppError> {
        let target = path.to_path_buf();
        let mut paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        if paths.get(&target) == Some(&purpose) {
            paths.remove(&target);
            Ok(target)
        } else {
            Err(AppError::DocumentStateInvalid(
                "Refusing to discard a file that is not registered for this purpose".to_string(),
            ))
        }
    }

    pub fn adopt_if_registered(&self, path: &Path) -> Result<bool, AppError> {
        let mut paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        Ok(paths.remove(path).is_some())
    }

    pub fn contains(&self, path: &Path) -> Result<bool, AppError> {
        let paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        Ok(paths.contains_key(path))
    }

    pub fn contains_for(
        &self,
        path: &Path,
        purpose: TransientFilePurpose,
    ) -> Result<bool, AppError> {
        let paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        Ok(paths.get(path) == Some(&purpose))
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.paths.lock().expect("registry lock").len()
    }
}

#[cfg(test)]
mod tests {
    use super::{TransientFilePurpose, TransientFileRegistry};
    use std::path::PathBuf;

    #[test]
    fn registered_path_can_be_taken_once_for_its_purpose() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("imported.xlsx");
        registry
            .register(path.clone(), TransientFilePurpose::OpenSelection)
            .unwrap();

        assert_eq!(
            registry
                .take(&path, TransientFilePurpose::OpenSelection)
                .unwrap(),
            path
        );
        assert!(
            registry
                .take(&path, TransientFilePurpose::OpenSelection)
                .is_err()
        );
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn adopting_registered_path_prevents_later_take() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("saved.xlsx");
        registry
            .register(path.clone(), TransientFilePurpose::SaveLocation)
            .unwrap();

        assert!(registry.adopt_if_registered(&path).unwrap());
        assert!(
            registry
                .take(&path, TransientFilePurpose::SaveLocation)
                .is_err()
        );
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn adopting_unknown_path_is_a_noop() {
        let registry = TransientFileRegistry::default();

        assert!(
            !registry
                .adopt_if_registered(&PathBuf::from("tmp").join("unknown.xlsx"))
                .unwrap()
        );
    }

    #[test]
    fn duplicate_registration_is_idempotent_for_the_same_purpose() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("repeated.xlsx");
        registry
            .register(path.clone(), TransientFilePurpose::OpenSelection)
            .unwrap();
        registry
            .register(path.clone(), TransientFilePurpose::OpenSelection)
            .unwrap();

        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn purpose_checks_do_not_consume_the_registration() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("reserved.xlsx");
        registry
            .register(path.clone(), TransientFilePurpose::SaveLocation)
            .unwrap();

        assert!(registry.contains(&path).unwrap());
        assert!(
            registry
                .contains_for(&path, TransientFilePurpose::SaveLocation)
                .unwrap()
        );
        assert!(
            !registry
                .contains_for(&path, TransientFilePurpose::OpenSelection)
                .unwrap()
        );
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn registration_cannot_change_a_transient_file_purpose() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("selection.xlsx");
        registry
            .register(path.clone(), TransientFilePurpose::OpenSelection)
            .unwrap();

        assert!(
            registry
                .register(path, TransientFilePurpose::SaveLocation)
                .is_err()
        );
    }
}
