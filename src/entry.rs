use chrono::{NaiveDateTime, NaiveDate};
use std::collections::HashSet;
use std::fmt;

pub const DEFAULT_TAG_SYMBOLS: &str = "#@";

#[derive(Debug, Clone)]
pub struct Entry {
    pub date: NaiveDateTime,
    pub starred: bool,
    pub title: String,
    pub body: String,
}

impl Entry {
    pub fn new(date: NaiveDateTime, starred: bool, title: String, body: String) -> Self {
        Entry { date, starred, title, body }
    }

    pub fn tags_with_symbols(&self, symbols: &str) -> HashSet<String> {
        let mut tags = HashSet::new();
        for word in format!("{} {}", self.title, self.body).split_whitespace() {
            let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric() && !symbols.contains(c));
            if let Some(symbol) = trimmed.chars().next() {
                if symbols.contains(symbol) {
                    let tag = &trimmed[symbol.len_utf8()..];
                    if !tag.is_empty() {
                        tags.insert(format!("{}{}", symbol, tag.to_lowercase()));
                    }
                }
            }
        }
        tags
    }

    pub fn date_only(&self) -> NaiveDate {
        self.date.date()
    }

    /// Render entry in jrnl's plain-text format:
    /// [YYYY-MM-DD HH:MM] Title.
    /// Body
    pub fn to_text(&self) -> String {
        let star = if self.starred { "*" } else { "" };
        let mut out = format!(
            "[{}] {}{}",
            self.date.format("%Y-%m-%d %H:%M"),
            star,
            self.title
        );
        if !self.body.trim().is_empty() {
            out.push('\n');
            out.push_str(self.body.trim_end());
            out.push('\n');
        } else {
            out.push('\n');
        }
        out
    }

    /// Short representation: "[date] title"
    pub fn to_short(&self) -> String {
        format!("[{}] {}{}", self.date.format("%Y-%m-%d %H:%M"), if self.starred { "*" } else { "" }, self.title)
    }
}

impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_text())
    }
}

/// Parse the contents of a journal text file into a list of entries.
/// Entries are delimited by lines matching "[YYYY-MM-DD HH:MM] ..."
pub fn parse_entries(content: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut current: Option<(NaiveDateTime, bool, String, Vec<String>)> = None;

    for line in content.lines() {
        if let Some((date, rest)) = try_parse_header(line) {
            // flush previous entry
            if let Some((d, starred, title, body_lines)) = current.take() {
                entries.push(Entry::new(d, starred, title, body_lines.join("\n")));
            }
            let (starred, title) = if let Some(t) = rest.strip_prefix('*') {
                (true, t.to_string())
            } else {
                (false, rest.to_string())
            };
            current = Some((date, starred, title, Vec::new()));
        } else if let Some((_, _, _, body_lines)) = current.as_mut() {
            body_lines.push(line.to_string());
        }
        // lines before the first header are ignored
    }
    if let Some((d, starred, title, body_lines)) = current.take() {
        // trim trailing empty lines from body
        let mut lines = body_lines;
        while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
            lines.pop();
        }
        entries.push(Entry::new(d, starred, title, lines.join("\n")));
    }
    entries
}

/// Try to parse a line like "[2024-01-15 09:30] Title here" into (datetime, "Title here")
fn try_parse_header(line: &str) -> Option<(NaiveDateTime, &str)> {
    let line = line.trim_start();
    if !line.starts_with('[') {
        return None;
    }
    let close = line.find(']')?;
    let date_str = &line[1..close];
    let date = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M").ok()?;
    let rest = line[close + 1..].trim_start();
    Some((date, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_entry() {
        let content = "[2024-01-15 09:30] Went for a walk.\nIt was sunny and nice.\n";
        let entries = parse_entries(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Went for a walk.");
        assert_eq!(entries[0].body, "It was sunny and nice.");
        assert!(!entries[0].starred);
    }

    #[test]
    fn test_parse_starred_entry() {
        let content = "[2024-01-15 09:30] *Big news today.\nDetails here.\n";
        let entries = parse_entries(content);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].starred);
        assert_eq!(entries[0].title, "Big news today.");
    }

    #[test]
    fn test_parse_multiple_entries() {
        let content = "[2024-01-15 09:30] First entry.\nBody one.\n\n[2024-01-16 10:00] Second entry.\nBody two.\n";
        let entries = parse_entries(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "First entry.");
        assert_eq!(entries[1].title, "Second entry.");
    }

    #[test]
    fn test_tags() {
        let e = Entry::new(
            NaiveDateTime::parse_from_str("2024-01-15 09:30", "%Y-%m-%d %H:%M").unwrap(),
            false,
            "Met with @bob about @project.".to_string(),
            "Discussed @timeline.".to_string(),
        );
        let tags = e.tags_with_symbols(DEFAULT_TAG_SYMBOLS);
        assert!(tags.contains("@bob"));
        assert!(tags.contains("@project"));
        assert!(tags.contains("@timeline"));
        assert_eq!(tags.len(), 3);
    }

    #[test]
    fn test_tags_with_custom_symbols() {
        let e = Entry::new(
            NaiveDateTime::parse_from_str("2024-01-15 09:30", "%Y-%m-%d %H:%M").unwrap(),
            false,
            "A #Run and @Walk entry.".to_string(),
            String::new(),
        );
        let tags = e.tags_with_symbols("#");
        assert!(tags.contains("#run"));
        assert!(!tags.contains("@walk"));
    }

    #[test]
    fn test_roundtrip() {
        let content = "[2024-01-15 09:30] First entry.\nBody one.\nMore body.\n";
        let entries = parse_entries(content);
        let rendered = entries[0].to_text();
        let reparsed = parse_entries(&rendered);
        assert_eq!(reparsed[0].title, entries[0].title);
        assert_eq!(reparsed[0].body, entries[0].body);
    }
}
