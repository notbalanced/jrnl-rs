use super::JournalStore;
use crate::entry::{parse_entries, Entry};
use anyhow::{Context, Result};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
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
        self.day_files_in_range(None, None)
    }

    fn day_files_reverse(&self) -> Result<Vec<PathBuf>> {
        let mut files = self.day_files()?;
        files.reverse();
        Ok(files)
    }

    /// Walk root/*/*/*.txt, skipping year/month/day directories that fall
    /// entirely outside [from, to] (inclusive). Either bound may be None
    /// (meaning "no lower/upper limit"). Returns files sorted by path.
    fn day_files_in_range(
        &self,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    ) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        if !self.root.exists() {
            return Ok(files);
        }

        let from_year = from.map(|d| d.year());
        let to_year   = to.map(|d| d.year());

        let mut year_dirs: Vec<_> = fs::read_dir(&self.root)
            .with_context(|| format!("Failed to read directory {}", self.root.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .collect();
        year_dirs.sort_by_key(|e| e.file_name());

        for year_entry in year_dirs {
            // Parse the directory name as a year number; skip non-numeric dirs.
            let year: i32 = match year_entry.file_name().to_string_lossy().parse() {
                Ok(y) => y,
                Err(_) => continue,
            };
            // Prune: skip this year if it's entirely outside the range.
            if from_year.map(|fy| year < fy).unwrap_or(false) { continue; }
            if to_year.map(|ty| year > ty).unwrap_or(false)   { continue; }

            let mut month_dirs: Vec<_> = fs::read_dir(year_entry.path())?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .collect();
            month_dirs.sort_by_key(|e| e.file_name());

            for month_entry in month_dirs {
                let month: u32 = match month_entry.file_name().to_string_lossy().parse() {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                // Prune: first day of this month is after `to`, or last day
                // of this month is before `from`.
                if let Some(from_date) = from {
                    // Last possible day of this month: use day 1 of next month minus 1.
                    let last_of_month = last_day_of_month(year, month);
                    if last_of_month < from_date { continue; }
                }
                if let Some(to_date) = to {
                    // First day of this month.
                    let first_of_month = NaiveDate::from_ymd_opt(year, month, 1)
                        .unwrap_or(to_date);
                    if first_of_month > to_date { continue; }
                }

                let mut day_files_in_month: Vec<_> = fs::read_dir(month_entry.path())?
                    .filter_map(|e| e.ok())
                    .collect();
                day_files_in_month.sort_by_key(|e| e.file_name());

                for day_entry in day_files_in_month {
                    let path = day_entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                        continue;
                    }
                    // Parse the stem as a day number for fine-grained pruning.
                    let stem = path.file_stem()
                        .and_then(|s| s.to_str())
                        .and_then(|s| s.parse::<u32>().ok());
                    if let Some(day) = stem {
                        let file_date = NaiveDate::from_ymd_opt(year, month, day);
                        if let Some(fd) = file_date {
                            if from.map(|f| fd < f).unwrap_or(false) { continue; }
                            if to.map(|t| fd > t).unwrap_or(false)   { continue; }
                        }
                    }
                    files.push(path);
                }
            }
        }
        files.sort();
        Ok(files)
    }
}

impl JournalStore for FolderStore {
    fn load_entries_in_range(
        &self,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    ) -> Result<Vec<Entry>> {
        let mut entries = Vec::new();
        for file in self.day_files_in_range(from, to)? {
            let content = fs::read_to_string(&file)
                .with_context(|| format!("Failed to read journal file {}", file.display()))?;
            // The file-level pruning already ensured this file is within
            // range, but entries within the file are filtered individually
            // to handle --on (single day) and exact time boundaries cleanly.
            // The caller's Filter will apply the real datetime comparison;
            // here we just load every entry from the qualifying files.
            entries.extend(parse_entries(&content));
        }
        entries.sort_by_key(|e| e.date);
        Ok(entries)
    }

    fn last_entry(&self) -> Result<Option<Entry>> {
        let mut latest: Option<(std::fs::Metadata, PathBuf)> = None;

        for file in self.day_files()? {
            let metadata = fs::metadata(&file)
                .with_context(|| format!("Failed to stat journal file {}", file.display()))?;
            match &latest {
                Some((current, _)) if metadata.modified()? <= current.modified()? => {}
                _ => latest = Some((metadata, file)),
            }
        }

        let Some((_, path)) = latest else {
            return Ok(None);
        };

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read journal file {}", path.display()))?;
        Ok(parse_entries(&content).into_iter().last())
    }

    fn load_entries_for_date(&self, date: NaiveDateTime) -> Result<Vec<Entry>> {
        let path = self.path_for_date(&date);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read journal file {}", path.display()))?;
        let target_date = date.date();
        let mut entries: Vec<Entry> = parse_entries(&content)
            .into_iter()
            .filter(|e| e.date_only() == target_date)
            .collect();
        entries.sort_by_key(|e| e.date);
        Ok(entries)
    }

    fn load_last_n_entries(&self, n: usize) -> Result<Vec<Entry>> {
        if n == 0 {
            return Ok(Vec::new());
        }

        let mut entries: Vec<Entry> = Vec::new();
        for file in self.day_files_reverse()? {
            let content = fs::read_to_string(&file)
                .with_context(|| format!("Failed to read journal file {}", file.display()))?;
            entries.extend(parse_entries(&content));
            if entries.len() >= n {
                break;
            }
        }
        entries.sort_by_key(|e| e.date);
        if entries.len() > n {
            entries = entries.split_off(entries.len() - n);
        }
        Ok(entries)
    }

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
/// Return the last calendar date of the given year/month.
fn last_day_of_month(year: i32, month: u32) -> NaiveDate {
    // First day of the next month, minus one day.
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .unwrap()
        .pred_opt()
        .unwrap()
}

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
    use chrono::{NaiveDate, NaiveDateTime};
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

    #[test]
    fn test_load_entries_in_range_returns_only_matching_files() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());

        store.append_entry(&entry("2022-01-01 09:00", "2022 entry.", "")).unwrap();
        store.append_entry(&entry("2024-06-15 09:00", "Mid 2024 entry.", "")).unwrap();
        store.append_entry(&entry("2026-06-01 09:00", "Early June 2026.", "")).unwrap();
        store.append_entry(&entry("2026-06-10 09:00", "Mid June 2026.", "")).unwrap();
        store.append_entry(&entry("2026-07-04 09:00", "July 2026 entry.", "")).unwrap();

        let from = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let to   = NaiveDate::from_ymd_opt(2026, 6, 30).unwrap();

        let entries = store.load_entries_in_range(Some(from), Some(to)).unwrap();

        assert_eq!(entries.len(), 2, "should only load June 2026 entries");
        assert!(entries.iter().any(|e| e.title == "Early June 2026."));
        assert!(entries.iter().any(|e| e.title == "Mid June 2026."));
    }

    #[test]
    fn test_load_entries_in_range_open_ended_from() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());

        store.append_entry(&entry("2022-01-01 09:00", "Old entry.", "")).unwrap();
        store.append_entry(&entry("2026-06-01 09:00", "New entry.", "")).unwrap();

        let from = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let entries = store.load_entries_in_range(Some(from), None).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "New entry.");
    }

    #[test]
    fn test_load_entries_in_range_open_ended_to() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());

        store.append_entry(&entry("2022-01-01 09:00", "Old entry.", "")).unwrap();
        store.append_entry(&entry("2026-06-01 09:00", "New entry.", "")).unwrap();

        let to = NaiveDate::from_ymd_opt(2023, 12, 31).unwrap();
        let entries = store.load_entries_in_range(None, Some(to)).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Old entry.");
    }

    #[test]
    fn test_load_entries_in_range_no_bounds_returns_all() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());

        store.append_entry(&entry("2022-01-01 09:00", "Entry A.", "")).unwrap();
        store.append_entry(&entry("2026-06-01 09:00", "Entry B.", "")).unwrap();

        let entries = store.load_entries_in_range(None, None).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_load_entries_in_range_skips_years_outside_range() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());

        // Create entries across many years.
        for year in [2020u32, 2021, 2022, 2023, 2024, 2025, 2026] {
            store.append_entry(&entry(
                &format!("{}-06-01 09:00", year),
                &format!("Entry {}.", year),
                "",
            )).unwrap();
        }

        let from = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        let to   = NaiveDate::from_ymd_opt(2025, 12, 31).unwrap();

        let entries = store.load_entries_in_range(Some(from), Some(to)).unwrap();

        // Should only return 2024 and 2025 entries, not 2020-2023 or 2026.
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.title == "Entry 2024."));
        assert!(entries.iter().any(|e| e.title == "Entry 2025."));
    }

    #[test]
    fn test_load_last_n_entries_returns_most_recent_entries() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());

        store.append_entry(&entry("2024-06-01 09:00", "Entry A.", "")).unwrap();
        store.append_entry(&entry("2024-06-02 09:00", "Entry B.", "")).unwrap();
        store.append_entry(&entry("2024-06-03 09:00", "Entry C.", "")).unwrap();
        store.append_entry(&entry("2024-06-04 09:00", "Entry D.", "")).unwrap();

        let entries = store.load_last_n_entries(2).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Entry C.");
        assert_eq!(entries[1].title, "Entry D.");
    }

    #[test]
    fn test_load_last_n_entries_zero_returns_empty() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        store.append_entry(&entry("2024-06-01 09:00", "Entry.", "")).unwrap();
        assert_eq!(store.load_last_n_entries(0).unwrap().len(), 0);
    }

    #[test]
    fn test_load_last_n_entries_n_larger_than_journal() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        store.append_entry(&entry("2024-06-01 09:00", "Only entry.", "")).unwrap();
        let entries = store.load_last_n_entries(100).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Only entry.");
    }

    #[test]
    fn test_load_last_n_entries_sorted_by_date_ascending() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        // Entries in different months — reverse walking should pick 3 most recent.
        store.append_entry(&entry("2024-01-15 09:00", "January.", "")).unwrap();
        store.append_entry(&entry("2024-03-20 09:00", "March.", "")).unwrap();
        store.append_entry(&entry("2024-06-05 09:00", "June A.", "")).unwrap();
        store.append_entry(&entry("2024-06-10 09:00", "June B.", "")).unwrap();

        let entries = store.load_last_n_entries(3).unwrap();
        assert_eq!(entries.len(), 3);
        // Must be returned sorted oldest-first within the selection.
        assert_eq!(entries[0].title, "March.");
        assert_eq!(entries[1].title, "June A.");
        assert_eq!(entries[2].title, "June B.");
    }

    #[test]
    fn test_load_last_n_entries_multiple_entries_same_day() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        // Two entries in the same day file — requesting n=1 should return the
        // most recent one only; requesting n=2 should return both.
        store.append_entry(&entry("2024-06-10 09:00", "Morning.", "")).unwrap();
        store.append_entry(&entry("2024-06-10 18:00", "Evening.", "")).unwrap();

        let one = store.load_last_n_entries(1).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].title, "Evening.");

        let two = store.load_last_n_entries(2).unwrap();
        assert_eq!(two.len(), 2);
        assert_eq!(two[0].title, "Morning.");
        assert_eq!(two[1].title, "Evening.");
    }

    #[test]
    fn test_load_last_n_entries_on_empty_journal() {
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());
        assert_eq!(store.load_last_n_entries(5).unwrap().len(), 0);
    }

    #[test]
    fn test_last_day_of_month() {
        assert_eq!(last_day_of_month(2024, 1), NaiveDate::from_ymd_opt(2024, 1, 31).unwrap());
        assert_eq!(last_day_of_month(2024, 2), NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()); // leap year
        assert_eq!(last_day_of_month(2023, 2), NaiveDate::from_ymd_opt(2023, 2, 28).unwrap());
        assert_eq!(last_day_of_month(2024, 12), NaiveDate::from_ymd_opt(2024, 12, 31).unwrap());
    }

    #[test]
    fn test_save_all_with_date_scoped_entries_deletes_everything_outside_range() {
        // DOCUMENTS THE DANGER (see doc comment on the trait method): if a
        // caller passes save_all() a date-range-scoped subset instead of the
        // full journal, every day file outside that subset is deleted. This
        // was the root cause of a real data-loss bug where
        // `jrnl --on today --edit` wiped out years of entries, because the
        // caller mistakenly used `load_entries_in_range(today, today)`'s
        // result as the basis for save_all() instead of the full journal.
        //
        // This test exists to make that failure mode explicit and easy to
        // find if it's ever reintroduced upstream -- the fix belongs in the
        // CALLER (always load the full journal before edit/delete), not here.
        let dir = tempdir().unwrap();
        let store = FolderStore::new(dir.path().to_path_buf());

        store.append_entry(&entry("2022-01-01 09:00", "Old entry.", "")).unwrap();
        store.append_entry(&entry("2025-03-10 09:00", "Another old entry.", "")).unwrap();
        store.append_entry(&entry("2026-06-19 09:00", "Today entry.", "")).unwrap();

        // Simulate the buggy caller: load only "today"'s range, then save_all
        // with just that scoped result (as if the user deleted today's entry,
        // leaving zero entries in the scoped set).
        let today = NaiveDate::from_ymd_opt(2026, 6, 19).unwrap();
        let scoped = store.load_entries_in_range(Some(today), Some(today)).unwrap();
        assert_eq!(scoped.len(), 1, "sanity check: scoped load should only see today's entry");

        // Pretend the user deleted today's entry in the editor -> empty scoped set.
        store.save_all(&[]).unwrap();

        // BUG: this would wipe 2022 and 2025 too, because they weren't
        // represented in the (empty) scoped entries passed to save_all.
        let remaining = store.load_entries().unwrap();
        assert_eq!(
            remaining.len(), 0,
            "this assertion documents the bug: save_all() with scoped/empty \
             entries deletes the WHOLE journal, not just the scoped range. \
             Callers must always pass the full journal to save_all()."
        );
    }
}
