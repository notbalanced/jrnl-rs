use super::JournalStore;
use crate::entry::{parse_entries, Entry};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

pub struct SingleFileStore {
    pub path: PathBuf,
}

impl SingleFileStore {
    pub fn new(path: PathBuf) -> Self {
        SingleFileStore { path }
    }
}

impl JournalStore for SingleFileStore {
    fn last_entry(&self) -> Result<Option<Entry>> {
        if !self.path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("Failed to read journal file {}", self.path.display()))?;
        Ok(parse_entries(&content).into_iter().last())
    }

    fn load_entries(&self) -> Result<Vec<Entry>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("Failed to read journal file {}", self.path.display()))?;
        let mut entries = parse_entries(&content);
        entries.sort_by_key(|e| e.date);
        Ok(entries)
    }

    fn append_entry(&self, entry: &Entry) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }
        }

        // Load, insert in sorted order, rewrite. For a personal journal this is
        // fine performance-wise and keeps the file consistently sorted.
        let mut entries = self.load_entries()?;
        let pos = entries.partition_point(|e| e.date <= entry.date);
        entries.insert(pos, entry.clone());
        self.save_all(&entries)
    }

    fn save_all(&self, entries: &[Entry]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }
        }
        let mut content = String::new();
        for e in entries {
            content.push_str(&e.to_text());
            content.push('\n');
        }
        // Avoid trailing double newline at EOF.
        let content = content.trim_end_matches('\n').to_string() + "\n";
        fs::write(&self.path, content)
            .with_context(|| format!("Failed to write journal file {}", self.path.display()))?;
        Ok(())
    }
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

    #[test]
    fn test_load_nonexistent_file_returns_empty() {
        let dir = tempdir().unwrap();
        let store = SingleFileStore::new(dir.path().join("journal.txt"));
        assert_eq!(store.load_entries().unwrap().len(), 0);
    }

    #[test]
    fn test_append_and_load() {
        let dir = tempdir().unwrap();
        let store = SingleFileStore::new(dir.path().join("journal.txt"));
        store.append_entry(&entry("2024-01-15 09:00", "First.", "Body one")).unwrap();
        store.append_entry(&entry("2024-01-16 09:00", "Second.", "Body two")).unwrap();

        let entries = store.load_entries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "First.");
        assert_eq!(entries[1].title, "Second.");
    }

    #[test]
    fn test_append_maintains_sort_order() {
        let dir = tempdir().unwrap();
        let store = SingleFileStore::new(dir.path().join("journal.txt"));
        store.append_entry(&entry("2024-01-16 09:00", "Second.", "")).unwrap();
        store.append_entry(&entry("2024-01-15 09:00", "First.", "")).unwrap();

        let entries = store.load_entries().unwrap();
        assert_eq!(entries[0].title, "First.");
        assert_eq!(entries[1].title, "Second.");
    }

    #[test]
    fn test_save_all_overwrites() {
        let dir = tempdir().unwrap();
        let store = SingleFileStore::new(dir.path().join("journal.txt"));
        store.append_entry(&entry("2024-01-15 09:00", "First.", "")).unwrap();
        store.append_entry(&entry("2024-01-16 09:00", "Second.", "")).unwrap();

        let entries = vec![entry("2024-01-15 09:00", "Only.", "")];
        store.save_all(&entries).unwrap();

        let loaded = store.load_entries().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "Only.");
    }

    #[test]
    fn test_creates_parent_directory() {
        let dir = tempdir().unwrap();
        let store = SingleFileStore::new(dir.path().join("nested").join("journal.txt"));
        store.append_entry(&entry("2024-01-15 09:00", "First.", "")).unwrap();
        assert!(store.path.exists());
    }
}
