use crate::entry::Entry;
use chrono::NaiveDateTime;

#[derive(Debug, Default)]
pub struct Filter {
    pub on: Option<NaiveDateTime>,
    pub from: Option<NaiveDateTime>,
    pub to: Option<NaiveDateTime>,
    pub contains: Option<String>,
    pub starred: bool,
    pub tagged: bool,
    pub not: Option<String>,
    pub and: bool,
    pub limit: Option<usize>,
}

impl Filter {
    /// Whether any filter condition is set (other than limit).
    fn has_conditions(&self) -> bool {
        self.on.is_some()
            || self.from.is_some()
            || self.to.is_some()
            || self.contains.is_some()
            || self.starred
            || self.tagged
    }

    fn matches(&self, entry: &Entry) -> bool {
        if !self.has_conditions() {
            return true;
        }

        let mut checks: Vec<bool> = Vec::new();

        if let Some(on) = &self.on {
            checks.push(entry.date_only() == on.date());
        }
        if let Some(from) = &self.from {
            checks.push(entry.date >= *from);
        }
        if let Some(to) = &self.to {
            checks.push(entry.date <= *to);
        }
        if let Some(text) = &self.contains {
            let needle = text.to_lowercase();
            let haystack = format!("{} {}", entry.title, entry.body).to_lowercase();
            checks.push(haystack.contains(&needle));
        }
        if self.starred {
            checks.push(entry.starred);
        }
        if self.tagged {
            checks.push(!entry.tags().is_empty());
        }

        if self.and {
            checks.into_iter().all(|c| c)
        } else {
            checks.into_iter().any(|c| c)
        }
    }

    fn excluded(&self, entry: &Entry) -> bool {
        match &self.not {
            None => false,
            Some(val) => match val.as_str() {
                "starred" => entry.starred,
                "tagged" => !entry.tags().is_empty(),
                tag => {
                    let tag = if tag.starts_with('@') {
                        tag.to_lowercase()
                    } else {
                        format!("@{}", tag.to_lowercase())
                    };
                    entry.tags().contains(&tag)
                }
            },
        }
    }

    /// Apply this filter to a list of entries (assumed already sorted by date).
    /// Returns the matching entries, capped by `limit` (most recent first if limited).
    pub fn apply<'a>(&self, entries: &'a [Entry]) -> Vec<&'a Entry> {
        let mut matched: Vec<&Entry> = entries
            .iter()
            .filter(|e| self.matches(e) && !self.excluded(e))
            .collect();

        if let Some(n) = self.limit {
            // Take the n most recent
            let len = matched.len();
            if len > n {
                matched = matched.split_off(len - n);
            }
        }

        matched
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::Entry;
    use chrono::NaiveDateTime;

    fn entry(date: &str, starred: bool, title: &str, body: &str) -> Entry {
        Entry::new(
            NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M").unwrap(),
            starred,
            title.to_string(),
            body.to_string(),
        )
    }

    #[test]
    fn test_no_filter_returns_all() {
        let entries = vec![entry("2024-01-01 09:00", false, "A.", "")];
        let f = Filter::default();
        assert_eq!(f.apply(&entries).len(), 1);
    }

    #[test]
    fn test_starred_filter() {
        let entries = vec![
            entry("2024-01-01 09:00", true, "A.", ""),
            entry("2024-01-02 09:00", false, "B.", ""),
        ];
        let f = Filter { starred: true, ..Default::default() };
        let result = f.apply(&entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "A.");
    }

    #[test]
    fn test_contains_filter() {
        let entries = vec![
            entry("2024-01-01 09:00", false, "Went for a walk.", "It rained."),
            entry("2024-01-02 09:00", false, "Stayed home.", "Read a book."),
        ];
        let f = Filter { contains: Some("rained".to_string()), ..Default::default() };
        let result = f.apply(&entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Went for a walk.");
    }

    #[test]
    fn test_and_filter() {
        let entries = vec![
            entry("2024-01-01 09:00", true, "Walk.", "rained"),
            entry("2024-01-02 09:00", true, "Home.", "sunny"),
            entry("2024-01-03 09:00", false, "Run.", "rained"),
        ];
        let f = Filter {
            starred: true,
            contains: Some("rained".to_string()),
            and: true,
            ..Default::default()
        };
        let result = f.apply(&entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Walk.");
    }

    #[test]
    fn test_not_tag() {
        let entries = vec![
            entry("2024-01-01 09:00", false, "Met @bob.", ""),
            entry("2024-01-02 09:00", false, "Solo day.", ""),
        ];
        let f = Filter { not: Some("bob".to_string()), ..Default::default() };
        let result = f.apply(&entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "Solo day.");
    }

    #[test]
    fn test_limit_takes_most_recent() {
        let entries = vec![
            entry("2024-01-01 09:00", false, "A.", ""),
            entry("2024-01-02 09:00", false, "B.", ""),
            entry("2024-01-03 09:00", false, "C.", ""),
        ];
        let f = Filter { limit: Some(2), ..Default::default() };
        let result = f.apply(&entries);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].title, "B.");
        assert_eq!(result[1].title, "C.");
    }
}
