use crate::entry::Entry;
use anyhow::Result;

pub mod single_file;
pub mod folder;

/// A backend that can load and persist a journal's entries.
pub trait JournalStore {
    /// Load all entries from storage, sorted by date ascending.
    fn load_entries(&self) -> Result<Vec<Entry>>;

    /// Append a single new entry to storage.
    fn append_entry(&self, entry: &Entry) -> Result<()>;

    /// Replace the entire contents of storage with the given entries.
    /// Used after --edit or --delete reconciliation.
    /// `entries` is assumed sorted by date ascending.
    fn save_all(&self, entries: &[Entry]) -> Result<()>;
}
