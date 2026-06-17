use crate::entry::Entry;
use anyhow::Result;
use chrono::NaiveDate;
use chrono::NaiveDateTime;

pub mod single_file;
pub mod folder;

/// A backend that can load and persist a journal's entries.
pub trait JournalStore {
    /// Load all entries from storage, sorted by date ascending.
    fn load_entries(&self) -> Result<Vec<Entry>>;

    /// Load only entries within the given date range (inclusive on both ends).
    /// Both `from` and `to` are optional; if both are None this is equivalent
    /// to `load_entries`. For folder-mode this avoids opening files outside
    /// the range at all. For single-file mode it reads the whole file and
    /// filters by date (unavoidable).
    fn load_entries_in_range(
        &self,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    ) -> Result<Vec<Entry>>;

    /// Load only the entries whose date matches the given datetime's calendar
    /// date. For folder-mode this reads a single YYYY/MM/DD.txt file; for
    /// single-file mode it reads the whole file and filters by date.
    fn load_entries_for_date(&self, date: NaiveDateTime) -> Result<Vec<Entry>>;

    /// Return the most recently added entry as stored on disk.
    fn last_entry(&self) -> Result<Option<Entry>>;

    /// Append a single new entry to storage.
    fn append_entry(&self, entry: &Entry) -> Result<()>;

    /// Replace the entire contents of storage with the given entries.
    /// Used after --edit or --delete reconciliation.
    /// `entries` is assumed sorted by date ascending.
    fn save_all(&self, entries: &[Entry]) -> Result<()>;
}
