use crate::config::{JournalConfig, StorageMode};
use crate::entry::Entry;
use crate::storage::folder::FolderStore;
use crate::storage::single_file::SingleFileStore;
use crate::storage::JournalStore;
use anyhow::Result;
use chrono::NaiveDateTime;

/// Wraps the appropriate storage backend for a journal config.
pub struct Journal {
    store: Box<dyn JournalStore>,
}

impl Journal {
    pub fn from_config(cfg: &JournalConfig) -> Self {
        let store: Box<dyn JournalStore> = match cfg.storage {
            StorageMode::File => Box::new(SingleFileStore::new(cfg.path.clone())),
            StorageMode::Folder => Box::new(FolderStore::new(cfg.path.clone())),
        };
        Journal { store }
    }

    pub fn load_entries(&self) -> Result<Vec<Entry>> {
        self.store.load_entries()
    }

    pub fn add_entry(&self, entry: &Entry) -> Result<()> {
        self.store.append_entry(entry)
    }

    /// Reconcile a user-edited subset of entries against the full journal.
    ///
    /// `original` is the subset that was sent to the editor (with original
    /// timestamps), `edited` is what came back. Entries are matched by
    /// position: if `edited` has fewer entries than `original`, the missing
    /// ones are deleted. Remaining entries (matched by index) have their
    /// title/body/starred updated, but keep their original timestamp unless
    /// the edited text included a different `[date]` header.
    ///
    /// Any entries beyond `edited.len()` in `original` are deleted. Extra
    /// entries appended in `edited` beyond `original.len()` are added as new.
    ///
    /// Returns the full updated entry list (sorted), which the caller should
    /// pass to `save_all`.
    pub fn reconcile(
        &self,
        all_entries: &[Entry],
        original: &[Entry],
        edited: &[Entry],
    ) -> Vec<Entry> {
        // Identify which entries in `all_entries` are part of `original` by
        // matching on (date, title, body) -- the same fields used in to_text().
        let original_keys: Vec<(NaiveDateTime, String, String)> = original
            .iter()
            .map(|e| (e.date, e.title.clone(), e.body.clone()))
            .collect();

        let mut result: Vec<Entry> = all_entries
            .iter()
            .filter(|e| {
                let key = (e.date, e.title.clone(), e.body.clone());
                !original_keys.contains(&key)
            })
            .cloned()
            .collect();

        // All edited entries (whether modified, unchanged, or newly added) get included.
        result.extend(edited.iter().cloned());
        result.sort_by_key(|e| e.date);
        result
    }

    pub fn save_all(&self, entries: &[Entry]) -> Result<()> {
        self.store.save_all(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn entry(date: &str, title: &str, body: &str) -> Entry {
        Entry::new(
            NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M").unwrap(),
            false,
            title.to_string(),
            body.to_string(),
        )
    }

    fn dummy_journal() -> Journal {
        let cfg = JournalConfig {
            path: std::path::PathBuf::from("/tmp/unused.txt"),
            storage: StorageMode::File,
        };
        Journal::from_config(&cfg)
    }

    #[test]
    fn test_reconcile_edit_changes_body() {
        let j = dummy_journal();
        let all = vec![
            entry("2024-01-01 09:00", "A.", "old body"),
            entry("2024-01-02 09:00", "B.", "unrelated"),
        ];
        let original = vec![all[0].clone()];
        let edited = vec![entry("2024-01-01 09:00", "A.", "new body")];

        let result = j.reconcile(&all, &original, &edited);
        assert_eq!(result.len(), 2);
        let edited_entry = result.iter().find(|e| e.title == "A.").unwrap();
        assert_eq!(edited_entry.body, "new body");
    }

    #[test]
    fn test_reconcile_delete_entry() {
        let j = dummy_journal();
        let all = vec![
            entry("2024-01-01 09:00", "A.", ""),
            entry("2024-01-02 09:00", "B.", ""),
        ];
        let original = vec![all[0].clone()];
        let edited: Vec<Entry> = vec![]; // user deleted it from the temp file

        let result = j.reconcile(&all, &original, &edited);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "B.");
    }

    #[test]
    fn test_reconcile_add_new_entry() {
        let j = dummy_journal();
        let all = vec![entry("2024-01-01 09:00", "A.", "")];
        let original = vec![all[0].clone()];
        let edited = vec![
            entry("2024-01-01 09:00", "A.", ""),
            entry("2024-01-03 09:00", "New entry.", "Added by user"),
        ];

        let result = j.reconcile(&all, &original, &edited);
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|e| e.title == "New entry."));
    }

    #[test]
    fn test_reconcile_change_date() {
        let j = dummy_journal();
        let all = vec![entry("2024-01-01 09:00", "A.", "")];
        let original = vec![all[0].clone()];
        // user changed the timestamp in the editor
        let edited = vec![entry("2024-02-01 09:00", "A.", "")];

        let result = j.reconcile(&all, &original, &edited);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].date_only().to_string(), "2024-02-01");
    }
}
