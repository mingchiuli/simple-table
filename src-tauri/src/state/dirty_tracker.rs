use crate::state::content_hash::ContentHash;

pub struct DirtyTracker {
    current_content_hash: ContentHash,
    saved_content_hash: ContentHash,
}

impl DirtyTracker {
    pub fn new(initial_hash: ContentHash) -> Self {
        Self {
            current_content_hash: initial_hash,
            saved_content_hash: initial_hash,
        }
    }

    #[cfg(test)]
    pub fn current_hash(&self) -> ContentHash {
        self.current_content_hash
    }

    pub fn is_dirty(&self) -> bool {
        self.current_content_hash != self.saved_content_hash
    }

    pub fn refresh(&mut self, content_hash: ContentHash) {
        self.current_content_hash = content_hash;
    }

    pub fn mark_saved(&mut self, content_hash: ContentHash) {
        self.current_content_hash = content_hash;
        self.saved_content_hash = content_hash;
    }
}
