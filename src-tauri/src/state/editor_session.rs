use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DOCUMENT_ID: AtomicU64 = AtomicU64::new(1);

pub struct EditorSession {
    document_id: u64,
    revision: u64,
}

impl EditorSession {
    pub fn new() -> Self {
        let document_id = NEXT_DOCUMENT_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .unwrap_or_else(|_| panic!("document id space exhausted"));
        Self {
            document_id,
            revision: 0,
        }
    }

    pub fn document_id(&self) -> u64 {
        self.document_id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn bump_revision(&mut self) -> Option<u64> {
        self.revision = self.revision.checked_add(1)?;
        Some(self.revision)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_never_wraps() {
        let mut session = EditorSession {
            document_id: 1,
            revision: u64::MAX,
        };

        assert_eq!(session.bump_revision(), None);
        assert_eq!(session.revision(), u64::MAX);
    }
}
