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

        let mut groups: Vec<bool> = Vec::new();

        // -on/-from/-to jointly define a single date range and are always
        // ANDed with each other (independent of --and), since otherwise
        // "--from X --to Y" in the default OR mode would be satisfied by
        // almost every entry (anything >= X, OR anything <= Y).
        let mut date_checks: Vec<bool> = Vec::new();
        if let Some(on) = &self.on {
            date_checks.push(entry.date_only() == on.date());
        }
        if let Some(from) = &self.from {
            date_checks.push(entry.date >= *from);
        }
        if let Some(to) = &self.to {
            date_checks.push(entry.date <= *to);
        }
        if !date_checks.is_empty() {
            groups.push(date_checks.into_iter().all(|c| c));
        }

        if let Some(text) = &self.contains {
            let needle = text.to_lowercase();
            let haystack = format!("{} {}", entry.title, entry.body).to_lowercase();
            groups.push(haystack.contains(&needle));
        }
        if self.starred {
            groups.push(entry.starred);
        }
        if self.tagged {
            groups.push(!entry.tags().is_empty());
        }

        if self.and {
            groups.into_iter().all(|c| c)
        } else {
            groups.into_iter().any(|c| c)
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

    #[test]
    fn test_from_and_to_form_a_range_not_or() {
        // Regression test: --from X --to Y should mean "X <= date <= Y",
        // not "date >= X OR date <= Y" (which would match almost everything
        // when Y is in the past relative to X... and vice versa).
        let entries = vec![
            entry("2024-01-01 09:00", false, "Before range.", ""),
            entry("2024-01-15 09:00", false, "In range.", ""),
            entry("2024-02-01 09:00", false, "After range.", ""),
        ];
        let f = Filter {
            from: Some(NaiveDateTime::parse_from_str("2024-01-10 00:00", "%Y-%m-%d %H:%M").unwrap()),
            to: Some(NaiveDateTime::parse_from_str("2024-01-20 23:59", "%Y-%m-%d %H:%M").unwrap()),
            ..Default::default()
        };
        let result = f.apply(&entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "In range.");
    }

    #[test]
    fn test_from_to_range_combines_with_other_filter_via_or() {
        // --from/--to define a range; in default (OR) mode that range
        // combines with other filter types (e.g. --starred) via OR.
        let entries = vec![
            // Outside the date range, but starred -> should still match via OR.
            entry("2023-01-01 09:00", true, "Old starred.", ""),
            // Inside the date range, not starred -> matches via the range.
            entry("2024-01-15 09:00", false, "In range.", ""),
            // Outside the date range, not starred -> matches neither.
            entry("2025-01-01 09:00", false, "Out of range.", ""),
        ];
        let f = Filter {
            from: Some(NaiveDateTime::parse_from_str("2024-01-10 00:00", "%Y-%m-%d %H:%M").unwrap()),
            to: Some(NaiveDateTime::parse_from_str("2024-01-20 23:59", "%Y-%m-%d %H:%M").unwrap()),
            starred: true,
            ..Default::default()
        };
        let result = f.apply(&entries);
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|e| e.title == "Old starred."));
        assert!(result.iter().any(|e| e.title == "In range."));
    }

    #[test]
    fn test_from_to_range_combines_with_other_filter_via_and() {
        let entries = vec![
            entry("2023-01-01 09:00", true, "Old starred.", ""),
            entry("2024-01-15 09:00", false, "In range, not starred.", ""),
            entry("2024-01-16 09:00", true, "In range and starred.", ""),
        ];
        let f = Filter {
            from: Some(NaiveDateTime::parse_from_str("2024-01-10 00:00", "%Y-%m-%d %H:%M").unwrap()),
            to: Some(NaiveDateTime::parse_from_str("2024-01-20 23:59", "%Y-%m-%d %H:%M").unwrap()),
            starred: true,
            and: true,
            ..Default::default()
        };
        let result = f.apply(&entries);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].title, "In range and starred.");
    }
}
