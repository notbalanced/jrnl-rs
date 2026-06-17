use crate::config::{JournalConfig, StorageMode};
use crate::entry::Entry;
use crate::storage::folder::FolderStore;
use crate::storage::single_file::SingleFileStore;
use crate::storage::JournalStore;
use anyhow::{Context, Result};
use chrono::NaiveDateTime;
use std::fs;
use std::path::PathBuf;

/// Wraps the appropriate storage backend for a journal config.
pub struct Journal {
    store: Box<dyn JournalStore>,
    cookie_path: PathBuf,
}

impl Journal {
    pub fn from_config(cfg: &JournalConfig) -> Self {
        let store: Box<dyn JournalStore> = match cfg.storage {
            StorageMode::File => Box::new(SingleFileStore::new(cfg.path.clone())),
            StorageMode::Folder => Box::new(FolderStore::new(cfg.path.clone())),
        };
        let cookie_path = cookie_path_for(cfg);
        Journal { store, cookie_path }
    }

    pub fn load_entries(&self) -> Result<Vec<Entry>> {
        self.store.load_entries()
    }

    /// Load entries within the given date range. For folder-mode journals
    /// this only opens the day files that fall within [from, to], making
    /// date-bounded queries much faster on large journals.
    pub fn load_entries_in_range(
        &self,
        from: Option<chrono::NaiveDate>,
        to: Option<chrono::NaiveDate>,
    ) -> Result<Vec<Entry>> {
        self.store.load_entries_in_range(from, to)
    }

    /// Load only the entries whose date matches the given datetime's date.
    /// For folder-mode journals this reads a single YYYY/MM/DD.txt file;
    /// for single-file journals it still reads the whole file (unavoidable)
    /// but then filters by date so the caller only sees the relevant entries.
    pub fn load_entries_for_date(&self, date: NaiveDateTime) -> Result<Vec<Entry>> {
        self.store.load_entries_for_date(date)
    }

    pub fn cookie_path(&self) -> &PathBuf {
        &self.cookie_path
    }

    #[allow(dead_code)]
    pub fn last_entry(&self) -> Result<Option<Entry>> {
        if self.cookie_path.exists() {
            let content = fs::read_to_string(&self.cookie_path)
                .with_context(|| format!("Failed to read last-entry cookie {}", self.cookie_path.display()))?;
            if let Some(entry) = crate::entry::parse_entries(&content).into_iter().last() {
                return Ok(Some(entry));
            }
        }

        self.store.last_entry()
    }

    pub fn add_entry(&self, entry: &Entry) -> Result<()> {
        self.store.append_entry(entry)?;
        write_cookie_file(&self.cookie_path, entry)?;
        Ok(())
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
        self.store.save_all(entries)?;

        if let Some(entry) = entries.last().cloned() {
            write_cookie_file(&self.cookie_path, &entry)?;
        }

        Ok(())
    }
}

fn cookie_path_for(cfg: &JournalConfig) -> PathBuf {
    match cfg.storage {
        StorageMode::File => {
            let mut path = cfg.path.clone();
            let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("journal.txt");
            path.set_file_name(format!("{}.last", file_name));
            path
        }
        StorageMode::Folder => cfg.path.join(".jrnl-last"),
    }
}

fn write_cookie_file(path: &PathBuf, entry: &Entry) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create cookie directory {}", parent.display()))?;
        }
    }

    let content = format!("{}\n", entry.to_text());
    fs::write(path, content)
        .with_context(|| format!("Failed to write last-entry cookie {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use tempfile::tempdir;

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
    fn test_cookie_path_uses_configured_cookie_dir() {
        let dir = tempdir().unwrap();
        let cookie_dir = dir.path();

        let journal = Journal::from_config(&JournalConfig {
            path: dir.path().join("journal.txt"),
            storage: StorageMode::File,
        });

        assert_eq!(journal.cookie_path, cookie_dir.join("journal.txt.last"));
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
    fn test_last_entry_prefers_newest_modified_day_file() {
        let dir = tempdir().unwrap();
        //let cookie_dir = dir.path().join("cookies");
        let journal = Journal::from_config(&JournalConfig {
            path: dir.path().to_path_buf(),
            storage: StorageMode::Folder,
        });

        let older = Entry::new(
            NaiveDateTime::parse_from_str("2026-06-10 09:00", "%Y-%m-%d %H:%M").unwrap(),
            false,
            "Test1.".to_string(),
            "body".to_string(),
        );
        let newer = Entry::new(
            NaiveDateTime::parse_from_str("2026-06-09 09:00", "%Y-%m-%d %H:%M").unwrap(),
            false,
            "This is Test 4.".to_string(),
            "body".to_string(),
        );

        journal.add_entry(&older).unwrap();
        journal.add_entry(&newer).unwrap();

        let last = journal.last_entry().unwrap().unwrap();
        assert_eq!(last.title, "This is Test 4.");
        assert_eq!(last.date, newer.date);
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
