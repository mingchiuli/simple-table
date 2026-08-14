pub struct EditorSession {
    document_id: u64,
    revision: u64,
}

impl EditorSession {
    pub fn new() -> Self {
        let document_id = nonzero_random_u64();
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

    pub fn can_bump_revision(&self) -> bool {
        self.revision < u64::MAX
    }

    #[cfg(test)]
    pub fn set_revision_for_test(&mut self, revision: u64) {
        self.revision = revision;
    }
}

fn nonzero_random_u64() -> u64 {
    loop {
        let value = uuid::Uuid::new_v4().as_u128() as u64;
        if value != 0 {
            return value;
        }
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
