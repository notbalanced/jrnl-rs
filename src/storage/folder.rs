use super::JournalStore;
use crate::entry::{parse_entries, Entry};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct FolderStore {
    pub root: PathBuf,
}

impl FolderStore {
    pub fn new(root: PathBuf) -> Self {
        FolderStore { root }
    }

    /// Compute the path for a given entry: root/YYYY/MM/DD.txt
    fn path_for_date(&self, date: &chrono::NaiveDateTime) -> PathBuf {
        self.root
            .join(format!("{:04}", date.format("%Y")))
            .join(format!("{:02}", date.format("%m")))
            .join(format!("{}.txt", date.format("%d")))
    }

    /// Walk root/*/*/*.txt and return all day files found, sorted by path
    /// (which corresponds to chronological order given the YYYY/MM/DD layout).
    fn day_files(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        if !self.root.exists() {
            return Ok(files);
        }
        for year_entry in fs::read_dir(&self.root)
            .with_context(|| format!("Failed to read directory {}", self.root.display()))?
        {
            let year_entry = year_entry?;
            if !year_entry.file_type()?.is_dir() {
                continue;
            }
            for month_entry in fs::read_dir(year_entry.path())? {
                let month_entry = month_entry?;
                if !month_entry.file_type()?.is_dir() {
                    continue;
                }
                for day_entry in fs::read_dir(month_entry.path())? {
                    let day_entry = day_entry?;
                    let path = day_entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("txt") {
                        files.push(path);
                    }
                }
            }
        }
        files.sort();
        Ok(files)
    }
}

impl JournalStore for FolderStore {
    fn load_entries(&self) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();
        for file in self.day_files()? {
            let content = fs::read_to_string(&file)
                .with_context(|| format!("Failed to read journal file {}", file.display()))?;
            entries.extend(parse_entries(&content));
        }
        entries.sort_by_key(|e| e.date);
        Ok(entries)
    }

    fn append_entry(&self, entry: &Entry) -> Result<()> {
        let path = self.path_for_date(&entry.date);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        // Load existing entries for this day, insert in sorted order, rewrite the day file.
        let mut day_entries = if path.exists() {
            let content = fs::read_to_string(&path)?;
            parse_entries(&content)
        } else {
            Vec::new()
        };
        let pos = day_entries.partition_point(|e| e.date <= entry.date);
        day_entries.insert(pos, entry.clone());
        write_day_file(&path, &day_entries)
    }

    /// Replace contents of all day files based on the given entries.
    /// Entries are grouped by date; any day file not represented in `entries`
    /// but currently present on disk is removed (covers --delete of an entire day).
    fn save_all(&self, entries: &[Entry]) -> Result<()> {
        // Group new entries by their target day file path.
        let mut grouped: BTreeMap<PathBuf, Vec<Entry>> = BTreeMap::new();
        for e in entries {
            grouped.entry(self.path_for_date(&e.date)).or_default().push(e.clone());
        }

        // Remove existing day files that no longer have any entries.
        for existing in self.day_files()? {
            if !grouped.contains_key(&existing) {
                fs::remove_file(&existing)
                    .with_context(|| format!("Failed to remove {}", existing.display()))?;
            }
        }

        // Write (or overwrite) each day file with its entries, sorted by time.
        // Skip files whose content would be unchanged, so unrelated day files
        // aren't touched (rewritten / mtime-bumped) by operations that only
        // affect a subset of the journal (e.g. `--edit --from ...`).
        for (path, mut day_entries) in grouped {
            day_entries.sort_by_key(|e| e.date);
            let new_content = render_day_file(&day_entries);

            if path.exists() {
                let existing = fs::read_to_string(&path)
                    .with_context(|| format!("Failed to read {}", path.display()))?;
                if existing == new_content {
                    continue;
                }
            }

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create directory {}", parent.display()))?;
            }
            fs::write(&path, &new_content)
                .with_context(|| format!("Failed to write {}", path.display()))?;
        }

        // Clean up now-empty year/month directories.
        prune_empty_dirs(&self.root)?;

        Ok(())
    }
}

/// Render a day file's contents from its entries (jrnl text format, sorted by time).
fn render_day_file(entries: &[Entry]) -> String {
    let mut content = String::new();
    for e in entries {
        content.push_str(&e.to_text());
        content.push('\n');
    }
    content.trim_end_matches('\n').to_string() + "\n"
}

fn write_day_file(path: &Path, entries: &[Entry]) -> Result<()> {
    let content = render_day_file(entries);
    fs::write(path, content).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Recursively remove empty directories under `root` (but not `root` itself).
fn prune_empty_dirs(root: &Path) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for year_entry in fs::read_dir(root)? {
        let year_entry = year_entry?;
        let year_path = year_entry.path();
        if !year_entry.file_type()?.is_dir() {
            continue;
        }
        for month_entry in fs::read_dir(&year_path)? {
            let month_entry = month_entry?;
            let month_path = month_entry.path();
            if !month_entry.file_type()?.is_dir() {
                continue;
            }
            if fs::read_dir(&month_path)?.next().is_none() {
                fs::remove_dir(&month_path)?;
            }
        }
        if fs::read_dir(&year_path)?.next().is_none() {
            fs::remove_dir(&year_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use std::time::{Duration, SystemTime};
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
    fn test_load_empty_root() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().join("journal"));
        assert_eq!(store.load_entries().unwrap().len(), 0);
    }

    #[test]
    fn test_append_creates_year_month_day_path() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        store.append_entry(&entry("2024-03-07 09:00", "Entry.", "Body")).unwrap();

        let expected = dir.path().join("2024").join("03").join("07.txt");
        assert!(expected.exists());
    }

    #[test]
    fn test_append_multiple_same_day() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        store.append_entry(&entry("2024-03-07 09:00", "Morning.", "")).unwrap();
        store.append_entry(&entry("2024-03-07 18:00", "Evening.", "")).unwrap();

        let entries = store.load_entries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Morning.");
        assert_eq!(entries[1].title, "Evening.");
    }

    #[test]
    fn test_load_across_multiple_days_sorted() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        store.append_entry(&entry("2024-03-08 09:00", "Second day.", "")).unwrap();
        store.append_entry(&entry("2024-01-01 09:00", "First day.", "")).unwrap();

        let entries = store.load_entries().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "First day.");
        assert_eq!(entries[1].title, "Second day.");
    }

    #[test]
    fn test_save_all_removes_empty_day_and_prunes_dirs() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        store.append_entry(&entry("2024-03-07 09:00", "Only entry.", "")).unwrap();

        // Save with no entries -> day file and empty dirs should be removed.
        store.save_all(&[]).unwrap();

        assert_eq!(store.load_entries().unwrap().len(), 0);
        assert!(!dir.path().join("2024").exists());
    }

    #[test]
    fn test_save_all_moves_entry_to_new_date() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        store.append_entry(&entry("2024-03-07 09:00", "Entry.", "")).unwrap();

        let moved = entry("2024-04-01 09:00", "Entry.", "");
        store.save_all(&[moved]).unwrap();

        assert!(!dir.path().join("2024").join("03").join("07.txt").exists());
        assert!(dir.path().join("2024").join("04").join("01.txt").exists());
    }

    #[test]
    fn test_save_all_does_not_touch_unchanged_day_files() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());

        // An old entry from 2022, and a recent entry from 2026.
        store.append_entry(&entry("2022-05-01 09:00", "Old entry.", "From 2022")).unwrap();
        store.append_entry(&entry("2026-06-01 09:00", "Recent entry.", "From 2026")).unwrap();

        let old_path = dir.path().join("2022").join("05").join("01.txt");
        assert!(old_path.exists());

        // Record the old file's mtime, then back-date it artificially so we
        // can detect any rewrite (rewrites would reset mtime to "now").
        let old_mtime = SystemTime::now() - Duration::from_secs(3600);
        let f = std::fs::File::open(&old_path).unwrap();
        f.set_modified(old_mtime).unwrap();

        // Simulate `--edit --from 2026-...`: load everything, edit only the
        // 2026 entry's body, then save_all with the full entry list -- the
        // same flow cmd_edit uses.
        let mut all = store.load_entries().unwrap();
        for e in all.iter_mut() {
            if e.date.format("%Y").to_string() == "2026" {
                e.body = "Edited body".to_string();
            }
        }
        store.save_all(&all).unwrap();

        // The 2022 file's content and mtime should be untouched.
        let new_mtime = std::fs::metadata(&old_path).unwrap().modified().unwrap();
        assert_eq!(new_mtime, old_mtime, "unrelated day file's mtime should not change");

        let content = std::fs::read_to_string(&old_path).unwrap();
        assert!(content.contains("Old entry."));

        // The 2026 file should reflect the edit.
        let new_path = dir.path().join("2026").join("06").join("01.txt");
        let new_content = std::fs::read_to_string(&new_path).unwrap();
        assert!(new_content.contains("Edited body"));
    }
}
