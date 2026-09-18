//! Shared primitives for idempotent request replay.
//!
//! Mutation and file-operation coordinators both need to (a) fingerprint a
//! request so a reused identifier with a different payload is rejected and
//! (b) remember a bounded window of terminal results so retries replay the
//! original outcome. This module owns those two mechanisms; each coordinator
//! keeps its own admission and in-flight policy.

use std::collections::VecDeque;

use sha2::{Digest, Sha256};

use crate::error::AppError;

/// Fixed-size hash identifying a request payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Fingerprint([u8; 32]);

impl Fingerprint {
    #[cfg(test)]
    pub(crate) const LENGTH: usize = 32;

    #[cfg(test)]
    pub(crate) fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }
}

/// Length-prefixed hasher used by every replay fingerprint.
#[derive(Default)]
pub(crate) struct FingerprintWriter(Sha256);

impl FingerprintWriter {
    pub(crate) fn write_tag(&mut self, tag: u8) {
        self.0.update([tag]);
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub(crate) fn write_u32(&mut self, value: u32) {
        self.0.update(value.to_le_bytes());
    }

    pub(crate) fn write_u64(&mut self, value: u64) {
        self.0.update(value.to_le_bytes());
    }

    pub(crate) fn write_i32(&mut self, value: i32) {
        self.0.update(value.to_le_bytes());
    }

    pub(crate) fn write_i64(&mut self, value: i64) {
        self.0.update(value.to_le_bytes());
    }

    pub(crate) fn write_index(&mut self, value: usize) -> Result<(), AppError> {
        self.write_u64(u64::try_from(value).map_err(|_| {
            AppError::ResourceLimitExceeded("fingerprint index exceeds u64 range".to_string())
        })?);
        Ok(())
    }

    /// Writes a length prefix followed by the text bytes. Field boundaries are
    /// part of the hash so concatenated values cannot collide.
    pub(crate) fn write_text(&mut self, value: &str) {
        self.write_u64(value.len() as u64);
        self.write_bytes(value.as_bytes());
    }

    pub(crate) fn write_optional_u64(&mut self, value: Option<u64>) {
        match value {
            Some(value) => {
                self.write_tag(1);
                self.write_u64(value);
            }
            None => self.write_tag(0),
        }
    }

    pub(crate) fn finish(self) -> Fingerprint {
        Fingerprint(self.0.finalize().into())
    }
}

/// One remembered terminal result.
pub(crate) struct TerminalEntry<K, V> {
    pub(crate) key: K,
    pub(crate) fingerprint: Fingerprint,
    pub(crate) value: V,
    pub(crate) bytes: usize,
}

/// FIFO cache of terminal results bounded by entry count and estimated bytes.
///
/// The caller supplies the estimated cost of each entry, including any payload
/// the entry retains, so a coordinator can share its response budget with the
/// cache.
pub(crate) struct TerminalCache<K, V> {
    entries: VecDeque<TerminalEntry<K, V>>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
}

impl<K, V> TerminalCache<K, V> {
    pub(crate) fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            max_entries,
            max_bytes,
        }
    }

    pub(crate) fn find(&self, mut matches: impl FnMut(&K) -> bool) -> Option<&TerminalEntry<K, V>> {
        self.entries.iter().find(|entry| matches(&entry.key))
    }

    pub(crate) fn insert(&mut self, key: K, fingerprint: Fingerprint, value: V, bytes: usize) {
        while self.entries.len() >= self.max_entries
            || self.bytes.saturating_add(bytes) > self.max_bytes
        {
            let Some(expired) = self.entries.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(expired.bytes);
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.entries.push_back(TerminalEntry {
            key,
            fingerprint,
            value,
            bytes,
        });
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        self.entries.retain(|entry| keep(&entry.key, &entry.value));
        self.bytes = self.entries.iter().map(|entry| entry.bytes).sum();
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn total_bytes(&self) -> usize {
        self.bytes
    }
}

/// Per-entry bookkeeping cost used when a coordinator reserves replay budget.
pub(crate) fn entry_overhead<K, V>() -> usize {
    std::mem::size_of::<TerminalEntry<K, V>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint(tag: u8) -> Fingerprint {
        let mut writer = FingerprintWriter::default();
        writer.write_tag(tag);
        writer.finish()
    }

    #[test]
    fn terminal_cache_evicts_by_entry_count_in_insertion_order() {
        let mut cache: TerminalCache<u32, u32> = TerminalCache::new(2, usize::MAX);

        cache.insert(1, fingerprint(1), 10, 1);
        cache.insert(2, fingerprint(2), 20, 1);
        cache.insert(3, fingerprint(3), 30, 1);

        assert!(cache.find(|key| *key == 1).is_none());
        assert_eq!(cache.find(|key| *key == 2).expect("second").value, 20);
        assert_eq!(cache.find(|key| *key == 3).expect("third").value, 30);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn terminal_cache_evicts_by_byte_budget_and_keeps_the_newest_entry() {
        let mut cache: TerminalCache<u32, u32> = TerminalCache::new(usize::MAX, 100);

        cache.insert(1, fingerprint(1), 10, 60);
        cache.insert(2, fingerprint(2), 20, 60);

        assert!(cache.find(|key| *key == 1).is_none());
        assert_eq!(cache.total_bytes(), 60);

        // The newest result is always retained so the caller that just
        // finished can still replay its own outcome.
        cache.insert(3, fingerprint(3), 30, 101);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.find(|key| *key == 3).expect("newest").value, 30);
        assert_eq!(cache.total_bytes(), 101);
    }

    #[test]
    fn terminal_cache_retain_recomputes_byte_usage() {
        let mut cache: TerminalCache<u32, u32> = TerminalCache::new(usize::MAX, usize::MAX);

        cache.insert(1, fingerprint(1), 10, 30);
        cache.insert(2, fingerprint(2), 20, 40);
        cache.retain(|key, _| *key != 1);

        assert_eq!(cache.len(), 1);
        assert_eq!(cache.total_bytes(), 40);
    }

    #[test]
    fn fingerprints_are_fixed_size_and_field_delimited() {
        let mut first = FingerprintWriter::default();
        first.write_text("ab");
        first.write_text("c");
        let mut second = FingerprintWriter::default();
        second.write_text("a");
        second.write_text("bc");

        let concatenated_first = first.finish();
        let concatenated_second = second.finish();
        assert_eq!(concatenated_first.as_bytes().len(), Fingerprint::LENGTH);
        assert_ne!(concatenated_first, concatenated_second);
        assert_ne!(fingerprint(1), fingerprint(2));
    }
}
