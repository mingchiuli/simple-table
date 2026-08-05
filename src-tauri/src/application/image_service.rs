use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::document_data::{MAX_EMBEDDED_IMAGE_BYTES, MAX_RENDER_IMAGE_PIXELS};
use crate::error::AppError;
use image::ImageFormat;
use sha2::{Digest, Sha256};

const MAX_STAGED_IMAGES: usize = 4;
const MAX_STAGED_BYTES: usize = 48 * 1024 * 1024;
const SELECTION_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
pub(crate) struct StagedImage {
    pub file_name: String,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub media_id: String,
    pub bytes: Arc<[u8]>,
}

pub(crate) struct StagedImageSelection {
    pub token: String,
    pub file_name: String,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
}

struct StagedEntry {
    selected_at: Instant,
    image: StagedImage,
}

#[derive(Default)]
struct ImageSelectionStore {
    entries: HashMap<String, StagedEntry>,
    order: VecDeque<String>,
}

#[derive(Clone, Default)]
pub struct ImageService {
    selections: Arc<Mutex<ImageSelectionStore>>,
}

impl ImageService {
    pub(crate) fn stage(
        &self,
        file_name: String,
        bytes: Vec<u8>,
    ) -> Result<StagedImageSelection, AppError> {
        let staged = validate_image(file_name, bytes)?;
        let token = uuid::Uuid::new_v4().to_string();
        let selection = StagedImageSelection {
            token: token.clone(),
            file_name: staged.file_name.clone(),
            mime_type: staged.mime_type.clone(),
            width: staged.width,
            height: staged.height,
        };
        self.insert(token, staged)?;
        Ok(selection)
    }

    pub(crate) fn get(&self, token: &str) -> Result<StagedImage, AppError> {
        let mut store = self
            .selections
            .lock()
            .map_err(|_| AppError::poisoned_lock("image selection store"))?;
        prune_expired(&mut store);
        store
            .entries
            .get(token)
            .map(|entry| entry.image.clone())
            .ok_or_else(|| {
                AppError::DocumentStateInvalid(
                    "image selection expired or was already used".to_string(),
                )
            })
    }

    pub(crate) fn discard(&self, token: &str) -> Result<(), AppError> {
        let mut store = self
            .selections
            .lock()
            .map_err(|_| AppError::poisoned_lock("image selection store"))?;
        store.entries.remove(token);
        store.order.retain(|entry| entry != token);
        Ok(())
    }

    fn insert(&self, token: String, image: StagedImage) -> Result<(), AppError> {
        let mut store = self
            .selections
            .lock()
            .map_err(|_| AppError::poisoned_lock("image selection store"))?;
        prune_expired(&mut store);
        store.order.push_back(token.clone());
        store.entries.insert(
            token,
            StagedEntry {
                selected_at: Instant::now(),
                image,
            },
        );
        while store.entries.len() > MAX_STAGED_IMAGES || staged_bytes(&store) > MAX_STAGED_BYTES {
            let Some(oldest) = store.order.pop_front() else {
                break;
            };
            store.entries.remove(&oldest);
        }
        Ok(())
    }
}

fn validate_image(file_name: String, bytes: Vec<u8>) -> Result<StagedImage, AppError> {
    if bytes.is_empty() || bytes.len() > MAX_EMBEDDED_IMAGE_BYTES {
        return Err(AppError::ResourceLimitExceeded(format!(
            "image is {} bytes, maximum is {MAX_EMBEDDED_IMAGE_BYTES} bytes",
            bytes.len()
        )));
    }
    let format = image::guess_format(&bytes).map_err(|_| AppError::UnsupportedFormat)?;
    let mime_type = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        _ => return Err(AppError::UnsupportedFormat),
    };
    let reader = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|error| AppError::ReadError(error.to_string()))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|error| AppError::ReadError(error.to_string()))?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_RENDER_IMAGE_PIXELS {
        return Err(AppError::ResourceLimitExceeded(format!(
            "image dimensions {width}x{height} exceed the supported limit"
        )));
    }
    let media_id = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(StagedImage {
        file_name,
        mime_type: mime_type.to_string(),
        width,
        height,
        media_id,
        bytes: Arc::from(bytes),
    })
}

fn prune_expired(store: &mut ImageSelectionStore) {
    let now = Instant::now();
    store
        .entries
        .retain(|_, entry| now.duration_since(entry.selected_at) <= SELECTION_TTL);
    store
        .order
        .retain(|token| store.entries.contains_key(token));
}

fn staged_bytes(store: &ImageSelectionStore) -> usize {
    store
        .entries
        .values()
        .map(|entry| entry.image.bytes.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_png_by_content_and_records_dimensions() {
        let pixels = image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        let mut output = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut output, ImageFormat::Png)
            .expect("encode png");

        let staged =
            validate_image("renamed.jpg".to_string(), output.into_inner()).expect("valid png");

        assert_eq!(staged.mime_type, "image/png");
        assert_eq!((staged.width, staged.height), (3, 2));
        assert_eq!(staged.media_id.len(), 64);
    }

    #[test]
    fn rejects_unsupported_and_oversized_images() {
        assert!(matches!(
            validate_image("image.gif".to_string(), b"GIF89a".to_vec()),
            Err(AppError::UnsupportedFormat)
        ));
        assert!(matches!(
            validate_image(
                "image.png".to_string(),
                vec![0; MAX_EMBEDDED_IMAGE_BYTES.saturating_add(1)]
            ),
            Err(AppError::ResourceLimitExceeded(_))
        ));
    }
}
