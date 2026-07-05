use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DOCUMENT_ID: AtomicU64 = AtomicU64::new(1);

pub struct EditorSession {
    document_id: u64,
    revision: u64,
}

impl EditorSession {
    pub fn new() -> Self {
        Self {
            document_id: NEXT_DOCUMENT_ID.fetch_add(1, Ordering::Relaxed),
            revision: 0,
        }
    }

    pub fn document_id(&self) -> u64 {
        self.document_id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}
