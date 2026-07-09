use crate::error::AppError;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(any(target_os = "android", target_os = "ios"))]
use std::sync::OnceLock;

#[cfg(any(target_os = "android", target_os = "ios"))]
static TRANSIENT_FILE_REGISTRY: OnceLock<TransientFileRegistry> = OnceLock::new();

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn transient_file_registry() -> &'static TransientFileRegistry {
    TRANSIENT_FILE_REGISTRY.get_or_init(TransientFileRegistry::default)
}

#[derive(Default)]
pub struct TransientFileRegistry {
    paths: Mutex<HashSet<PathBuf>>,
}

impl TransientFileRegistry {
    pub fn register(&self, path: PathBuf) -> Result<(), AppError> {
        let mut paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        paths.insert(path);
        Ok(())
    }

    pub fn take(&self, path: &Path) -> Result<PathBuf, AppError> {
        let target = path.to_path_buf();
        let mut paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        if paths.remove(&target) {
            Ok(target)
        } else {
            Err(AppError::DocumentStateInvalid(
                "Refusing to discard a file that is not registered as transient".to_string(),
            ))
        }
    }

    pub fn adopt_if_registered(&self, path: &Path) -> Result<bool, AppError> {
        let mut paths = self
            .paths
            .lock()
            .map_err(|_| AppError::poisoned_lock("transient file registry"))?;
        Ok(paths.remove(path))
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.paths.lock().expect("registry lock").len()
    }
}

#[cfg(test)]
mod tests {
    use super::TransientFileRegistry;
    use std::path::PathBuf;

    #[test]
    fn registered_path_can_be_taken_once() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("imported.xlsx");

        registry.register(path.clone()).unwrap();

        assert_eq!(registry.take(&path).unwrap(), path);
        assert!(registry.take(&path).is_err());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn adopting_registered_path_prevents_later_take() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("saved.xlsx");

        registry.register(path.clone()).unwrap();

        assert!(registry.adopt_if_registered(&path).unwrap());
        assert!(registry.take(&path).is_err());
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
    fn duplicate_registration_is_idempotent() {
        let registry = TransientFileRegistry::default();
        let path = PathBuf::from("tmp").join("repeated.xlsx");

        registry.register(path.clone()).unwrap();
        registry.register(path.clone()).unwrap();

        assert_eq!(registry.len(), 1);
        assert_eq!(registry.take(&path).unwrap(), path);
        assert!(registry.take(&path).is_err());
    }
}
