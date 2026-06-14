mod cli;
mod config;
mod date_parser;
mod editor;
mod entry;
mod filter;
mod formatter;
mod journal;
mod storage;

use anyhow::{anyhow, Result};
use chrono::Local;
use clap::Parser;
use cli::{Cli, FormatType};
use config::Config;
use entry::Entry;
use filter::Filter;
use journal::Journal;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    // Load config (with optional override path).
    let mut config = Config::load(cli.config_file.as_deref())?;

    // Apply one-off --config-override KEY VALUE.
    if let Some(kvs) = &cli.config_override {
        for pair in kvs.chunks(2) {
            if pair.len() == 2 {
                config.apply_override(&pair[0], &pair[1])?;
            }
        }
    }

    // --list: list configured journals and exit.
    if cli.list {
        return cmd_list_journals(&config, cli.format);
    }

    // For now, operate on the "default" journal. (Named journals could be
    // added by checking if the first word of `text` matches a configured
    // journal name.)
    let journal_cfg = config.get_journal("default")?;
    let journal = Journal::from_config(journal_cfg);

    if cli.is_search_mode() {
        cmd_search(&cli, &config, &journal)
    } else if !cli.text.is_empty() {
        cmd_add(&cli, &journal, &cli.text.join(" "))
    } else {
        // No text and no search flags: prompt for input on stdin.
        cmd_compose(&journal)
    }
}

/// --list: print configured journal names and their paths.
fn cmd_list_journals(config: &Config, format: Option<FormatType>) -> Result<()> {
    match format {
        Some(FormatType::Json) => {
            let map: std::collections::HashMap<&String, &std::path::Path> = config
                .journals
                .iter()
                .map(|(k, v)| (k, v.path.as_path()))
                .collect();
            println!("{}", serde_json::to_string_pretty(&map)?);
        }
        _ => {
            for (name, jcfg) in &config.journals {
                println!("{}: {}", name, jcfg.path.display());
            }
        }
    }
    Ok(())
}

/// Add a new entry. Splits an optional date prefix (e.g. "yesterday: text")
/// and a leading "*" for starred entries.
fn cmd_add(cli: &Cli, journal: &Journal, text: &str) -> Result<()> {
    let (date, rest) = date_parser::split_date_prefix(text);
    let date = date.unwrap_or_else(|| Local::now().naive_local());

    let (starred, rest) = match rest.strip_prefix('*') {
        Some(r) => (true, r),
        None => (false, rest),
    };

    let (title, body) = split_title_body(rest);

    let entry = if let Some(template_path) = &cli.template {
        apply_template(template_path, date, starred, &title, &body)?
    } else {
        Entry::new(date, starred, title, body)
    };

    journal.add_entry(&entry)?;
    println!("Entry added to journal.");
    Ok(())
}

/// Split entry text into a title (up to and including the first '.', '?', or '!')
/// and a body (the remainder).
fn split_title_body(text: &str) -> (String, String) {
    let text = text.trim();
    for (i, c) in text.char_indices() {
        if c == '.' || c == '?' || c == '!' {
            let split_at = i + c.len_utf8();
            let title = text[..split_at].trim().to_string();
            let body = text[split_at..].trim().to_string();
            return (title, body);
        }
    }
    (text.to_string(), String::new())
}

fn apply_template(
    path: &str,
    date: chrono::NaiveDateTime,
    starred: bool,
    title: &str,
    body: &str,
) -> Result<Entry> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("Failed to read template '{}': {}", path, e))?;
    let filled = content
        .replace("{{title}}", title)
        .replace("{{body}}", body)
        .replace("{{date}}", &date.format("%Y-%m-%d %H:%M").to_string());

    let (file_title, file_body) = split_title_body(&filled);
    let (final_title, final_body) = if file_title.is_empty() {
        (title.to_string(), file_body)
    } else {
        (file_title, file_body)
    };

    Ok(Entry::new(date, starred, final_title, final_body))
}

/// No text and no search flags: read a free-form entry from stdin until EOF.
fn cmd_compose(journal: &Journal) -> Result<()> {
    use std::io::Read;
    println!("Composing new entry. Press Ctrl-D (Linux/Mac) or Ctrl-Z then Enter (Windows) to finish.");
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    if buf.trim().is_empty() {
        println!("No input given, nothing saved.");
        return Ok(());
    }
    let date = Local::now().naive_local();
    let (starred, rest) = match buf.trim_start().strip_prefix('*') {
        Some(r) => (true, r),
        None => (false, buf.trim_start()),
    };
    let (title, body) = split_title_body(rest);
    let entry = Entry::new(date, starred, title, body);
    journal.add_entry(&entry)?;
    println!("Entry added to journal.");
    Ok(())
}

/// Handle all search/filter/action flags.
fn cmd_search(cli: &Cli, config: &Config, journal: &Journal) -> Result<()> {
    let entries = journal.load_entries()?;

    let filter = build_filter(cli)?;
    let matched_refs = filter.apply(&entries);
    let matched: Vec<Entry> = matched_refs.iter().map(|e| (*e).clone()).collect();

    if matched.is_empty() {
        println!("No entries found.");
        // --edit/--delete with no matches just exits cleanly.
        if !cli.edit && !cli.delete {
            return Ok(());
        }
    }

    if cli.delete {
        return cmd_delete(journal, &entries, &matched);
    }

    if cli.edit {
        return cmd_edit(config, journal, &entries, &matched);
    }

    if cli.tags {
        let refs: Vec<&Entry> = matched.iter().collect();
        let out = formatter::format_entries(&refs, Some(FormatType::Tags), false);
        println!("{}", out);
        return Ok(());
    }

    let refs: Vec<&Entry> = matched.iter().collect();
    let out = formatter::format_entries(&refs, cli.format, cli.short);

    if let Some(file_path) = &cli.file {
        std::fs::write(file_path, &out)
            .map_err(|e| anyhow!("Failed to write output file '{}': {}", file_path, e))?;
        println!("Output written to {}", file_path);
    } else {
        println!("{}", out);
    }

    Ok(())
}

fn build_filter(cli: &Cli) -> Result<Filter> {
    let mut filter = Filter {
        and: cli.and,
        starred: cli.starred,
        tagged: cli.tagged,
        contains: cli.contains.clone(),
        not: cli.not.clone(),
        limit: cli.n,
        ..Default::default()
    };

    if let Some(s) = &cli.on {
        filter.on = Some(parse_required_date(s)?);
    }
    if let Some(s) = &cli.from {
        filter.from = Some(parse_required_date(s)?);
    }
    if let Some(s) = &cli.to {
        // Treat -to as inclusive of the whole day by setting time to 23:59.
        let mut d = parse_required_date(s)?;
        d = d.date().and_hms_opt(23, 59, 59).unwrap();
        filter.to = Some(d);
    }

    Ok(filter)
}

fn parse_required_date(s: &str) -> Result<chrono::NaiveDateTime> {
    date_parser::parse_date(s).ok_or_else(|| anyhow!("Could not parse date: '{}'", s))
}

/// Interactively delete matched entries (with confirmation).
fn cmd_delete(journal: &Journal, all_entries: &[Entry], matched: &[Entry]) -> Result<()> {
    use std::io::{self, Write};

    if matched.is_empty() {
        return Ok(());
    }

    let mut to_delete: Vec<bool> = vec![false; matched.len()];

    for (i, e) in matched.iter().enumerate() {
        print!("Delete entry?\n{}\n[y/N] ", e.to_text());
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if answer.trim().eq_ignore_ascii_case("y") {
            to_delete[i] = true;
        }
    }

    let keys_to_delete: Vec<(chrono::NaiveDateTime, String, String)> = matched
        .iter()
        .zip(to_delete.iter())
        .filter(|(_, &del)| del)
        .map(|(e, _)| (e.date, e.title.clone(), e.body.clone()))
        .collect();

    if keys_to_delete.is_empty() {
        println!("No entries deleted.");
        return Ok(());
    }

    let remaining: Vec<Entry> = all_entries
        .iter()
        .filter(|e| {
            let key = (e.date, e.title.clone(), e.body.clone());
            !keys_to_delete.contains(&key)
        })
        .cloned()
        .collect();

    journal.save_all(&remaining)?;
    println!("{} entr{} deleted.", keys_to_delete.len(), if keys_to_delete.len() == 1 { "y" } else { "ies" });
    Ok(())
}

/// Open matched entries in the configured editor and reconcile changes.
fn cmd_edit(config: &Config, journal: &Journal, all_entries: &[Entry], matched: &[Entry]) -> Result<()> {
    if matched.is_empty() {
        return Ok(());
    }

    let editor_cmd = config.resolve_editor();
    let edited = editor::edit_entries(&editor_cmd, matched)?;

    let updated_all = journal.reconcile(all_entries, matched, &edited);
    journal.save_all(&updated_all)?;

    let delta = edited.len() as i64 - matched.len() as i64;
    match delta.cmp(&0) {
        std::cmp::Ordering::Less => println!("Entries updated. {} entr{} removed.", -delta, if -delta == 1 { "y" } else { "ies" }),
        std::cmp::Ordering::Greater => println!("Entries updated. {} entr{} added.", delta, if delta == 1 { "y" } else { "ies" }),
        std::cmp::Ordering::Equal => println!("Entries updated."),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_title_body_basic() {
        let (title, body) = split_title_body("Went for a walk. It was nice.");
        assert_eq!(title, "Went for a walk.");
        assert_eq!(body, "It was nice.");
    }

    #[test]
    fn test_split_title_body_no_punctuation() {
        let (title, body) = split_title_body("Just a quick note");
        assert_eq!(title, "Just a quick note");
        assert_eq!(body, "");
    }

    #[test]
    fn test_split_title_body_question() {
        let (title, body) = split_title_body("Did it work? Yes it did.");
        assert_eq!(title, "Did it work?");
        assert_eq!(body, "Yes it did.");
    }
}
