use crate::entry::{parse_entries, Entry};
use anyhow::{anyhow, Context, Result};
use std::fs;
use std::process::Command;

/// Open the given entries in the user's editor as plain text, then re-parse
/// the result. Returns (raw_text, parsed_entries).
///
/// Entries are written in jrnl's standard `[date] title\nbody` format,
/// separated by blank lines, so the user can freely edit, delete, or add
/// entries. The raw text is also returned so callers can fall back to
/// treating it as a single free-form entry if no `[date] ...` headers
/// were recognized (e.g. composing mode with a blank starting file).
pub fn edit_entries(editor_cmd: &str, entries: &[Entry]) -> Result<(String, Vec<Entry>)> {
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("jrnl-edit-{}.txt", std::process::id()));

    let mut content = String::new();
    for e in entries {
        content.push_str(&e.to_text());
        content.push('\n');
    }
    fs::write(&tmp_path, &content)
        .with_context(|| format!("Failed to write temp file {}", tmp_path.display()))?;

    run_editor(editor_cmd, &tmp_path)?;

    let edited = fs::read_to_string(&tmp_path)
        .with_context(|| format!("Failed to read temp file {}", tmp_path.display()))?;
    let _ = fs::remove_file(&tmp_path);

    let parsed = parse_entries(&edited);
    Ok((edited, parsed))
}

/// Spawn the configured editor on the given file and wait for it to exit.
/// `editor_cmd` may include arguments, e.g. "code --wait" or "vim".
fn run_editor(editor_cmd: &str, file: &std::path::Path) -> Result<()> {
    let parts: Vec<&str> = editor_cmd.split_whitespace().collect();
    let (program, args) = parts
        .split_first()
        .ok_or_else(|| anyhow!("Editor command is empty"))?;

    let status = Command::new(program)
        .args(args)
        .arg(file)
        .status()
        .with_context(|| format!("Failed to launch editor '{}'", editor_cmd))?;

    if !status.success() {
        return Err(anyhow!("Editor '{}' exited with non-zero status", editor_cmd));
    }
    Ok(())
}
