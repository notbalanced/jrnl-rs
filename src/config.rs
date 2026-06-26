use anyhow::{anyhow, Context, Result};
use crate::entry::DEFAULT_TAG_SYMBOLS;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::path::PathBuf;

/// How a single journal stores its entries on disk.
#[derive(Debug, Clone, PartialEq)]
pub enum StorageMode {
    /// All entries in a single file.
    File,
    /// Entries split into journal_root/YYYY/MM/DD.txt
    Folder,
}

/// A journal's configuration. Stored in YAML as a bare path string,
/// compatible with the original jrnl format:
///
///   journals:
///     default: /home/user/journal/   # folder (trailing slash or existing dir)
///     work:    /home/user/work.txt   # single file
///
/// Storage mode is inferred at runtime: trailing `/` or `\`, an existing
/// directory, or a path with no file extension → folder; otherwise file.
#[derive(Debug, Clone)]
pub struct JournalConfig {
    pub path: PathBuf,
    pub storage: StorageMode,
}

impl JournalConfig {
    pub fn new(path: PathBuf) -> Self {
        let storage = infer_storage_mode(&path);
        let path = strip_trailing_separator(path);
        JournalConfig { path, storage }
    }
}

/// Infer storage mode from a path.
pub fn infer_storage_mode(path: &PathBuf) -> StorageMode {
    let s = path.to_string_lossy();
    if s.ends_with('/') || s.ends_with('\\') {
        return StorageMode::Folder;
    }
    if path.exists() {
        return if path.is_dir() { StorageMode::Folder } else { StorageMode::File };
    }
    if path.extension().is_some() {
        StorageMode::File
    } else {
        StorageMode::Folder
    }
}

fn strip_trailing_separator(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if (s.ends_with('/') || s.ends_with('\\')) && s.len() > 1 {
        PathBuf::from(s.trim_end_matches(['/', '\\']).to_string())
    } else {
        path
    }
}

// Serialize as a bare path string (jrnl format). Append "/" for folder
// journals so the mode round-trips correctly before the path exists on disk.
impl Serialize for JournalConfig {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut p = self.path.to_string_lossy().to_string();
        if self.storage == StorageMode::Folder && !p.ends_with('/') {
            p.push('/');
        }
        s.serialize_str(&p)
    }
}

impl<'de> Deserialize<'de> for JournalConfig {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(JournalConfig::new(PathBuf::from(s)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Colors {
    #[serde(default = "default_color")]
    pub body: String,
    #[serde(default = "default_color")]
    pub date: String,
    #[serde(default = "default_color")]
    pub tags: String,
    #[serde(default = "default_color")]
    pub title: String,
    #[serde(default = "default_color")]
    pub search: String,
}

fn default_color() -> String {
    "none".to_string()
}

impl Default for Colors {
    fn default() -> Self {
        Self {
            body: default_color(),
            date: default_color(),
            tags: default_color(),
            title: default_color(),
            search: default_color(),
        }
    }
}

impl Colors {
    pub fn any_enabled(&self) -> bool {
        !self.body.eq_ignore_ascii_case("none")
            || !self.date.eq_ignore_ascii_case("none")
            || !self.tags.eq_ignore_ascii_case("none")
            || !self.title.eq_ignore_ascii_case("none")
            || !self.search.eq_ignore_ascii_case("none")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Map of journal name -> journal config.
    pub journals: HashMap<String, JournalConfig>,
    /// Symbols that mark a word as a tag.
    #[serde(default = "default_tagsymbols")]
    pub tagsymbols: String,
    /// Default editor command (falls back to $EDITOR / $VISUAL if empty).
    #[serde(default)]
    pub editor: Option<String>,
    /// strftime-style format used when rendering entry timestamps.
    #[serde(default = "default_timeformat")]
    pub timeformat: String,
    /// Maximum line width for displayed entries. 0 disables wrapping.
    #[serde(default = "default_linewrap")]
    pub linewrap: usize,
    /// Character(s) prepended to each body line in pretty/default output.
    #[serde(default = "default_indent_character")]
    pub indent_character: String,
    /// Optional colors for pretty output.
    #[serde(default)]
    pub colors: Colors,
}

fn default_timeformat() -> String { "%Y-%m-%d %H:%M".to_string() }
fn default_linewrap() -> usize { 79 }
fn default_indent_character() -> String { "|".to_string() }
fn default_tagsymbols() -> String { DEFAULT_TAG_SYMBOLS.to_string() }

impl Default for Config {
    fn default() -> Self {
        let mut journals = HashMap::new();
        let default_path = default_data_dir().join("journal");
        journals.insert("default".to_string(), JournalConfig::new(default_path));
        Config {
            journals,
            tagsymbols: default_tagsymbols(),
            editor: None,
            timeformat: default_timeformat(),
            linewrap: default_linewrap(),
            indent_character: default_indent_character(),
            colors: Colors::default(),
        }
    }
}

/// Result of loading a config: the config itself plus whether a file was found.
pub struct LoadedConfig {
    pub config: Config,
    pub config_path: PathBuf,
    pub found: bool,
}

impl Config {
    /// Load config, also returning whether a file was actually found on disk.
    pub fn load_with_status(config_file: Option<&str>) -> Result<LoadedConfig> {
        let path = match config_file {
            Some(p) => PathBuf::from(p),
            None => default_config_path(),
        };

        if !path.exists() {
            return Ok(LoadedConfig {
                config: Config::default(),
                config_path: path,
                found: false,
            });
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file at {}", path.display()))?;
        let config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse config file at {}", path.display()))?;
        Ok(LoadedConfig { config, config_path: path, found: true })
    }

    /// Load config from the given path, or from the default location.
    /// If no config file exists, returns a default config silently.
    pub fn load(config_file: Option<&str>) -> Result<Self> {
        Ok(Self::load_with_status(config_file)?.config)
    }

    /// Write a default config file to the default location (or given path).
    /// Returns an error (without writing) if the file already exists.
    pub fn write_default(config_file: Option<&str>) -> Result<PathBuf> {
        let path = match config_file {
            Some(p) => PathBuf::from(p),
            None => default_config_path(),
        };

        if path.exists() {
            return Err(anyhow!(
                "Config file already exists at {}\nUse --config-file to specify a different path.",
                path.display()
            ));
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
        }

        let default = Config::default();
        let yaml = default.to_yaml_string()?;
        std::fs::write(&path, &yaml)
            .with_context(|| format!("Failed to write config file at {}", path.display()))?;

        Ok(path)
    }

    /// Render this config as a human-readable YAML string.
    pub fn to_yaml_string(&self) -> Result<String> {
        Ok(serde_yaml::to_string(self)?)
    }

    /// Apply --config-override KEY VALUE to this config (in-memory only).
    pub fn apply_override(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "editor" => self.editor = Some(value.to_string()),
            "timeformat" => self.timeformat = value.to_string(),
            "tagsymbols" => self.tagsymbols = value.to_string(),
            "indent_character" | "indent-character" => self.indent_character = value.to_string(),
            "linewrap" => {
                self.linewrap = value
                    .parse::<usize>()
                    .map_err(|_| anyhow!("Invalid linewrap value '{}' (expected a non-negative integer)", value))?;
            }
            other => {
                if let Some(rest) = other.strip_prefix("colors.") {
                    match rest {
                        "body" => self.colors.body = value.to_string(),
                        "date" => self.colors.date = value.to_string(),
                        "tags" => self.colors.tags = value.to_string(),
                        "title" => self.colors.title = value.to_string(),
                        "search" => self.colors.search = value.to_string(),
                        _ => return Err(anyhow!("Unknown config override key '{}'", key)),
                    }
                } else if let Some(rest) = other.strip_prefix("journals.") {
                    // Allow "journals.work=/path" shorthand or legacy
                    // "journals.work.path=/path" form.
                    if !rest.contains('.') {
                        let journal = self.journals.get_mut(rest)
                            .ok_or_else(|| anyhow!("Unknown journal '{}'", rest))?;
                        journal.path = PathBuf::from(value);
                        journal.storage = infer_storage_mode(&journal.path);
                    } else {
                        let mut parts = rest.splitn(2, '.');
                        let name = parts.next().unwrap_or_default();
                        let field = parts.next().unwrap_or_default();
                        let journal = self.journals.get_mut(name)
                            .ok_or_else(|| anyhow!("Unknown journal '{}'", name))?;
                        match field {
                            "path" => {
                                journal.path = PathBuf::from(value);
                                journal.storage = infer_storage_mode(&journal.path);
                            }
                            "storage" => {
                                journal.storage = match value.to_lowercase().as_str() {
                                    "file" => StorageMode::File,
                                    "folder" => StorageMode::Folder,
                                    _ => return Err(anyhow!("Invalid storage mode '{}'", value)),
                                };
                            }
                            _ => return Err(anyhow!("Unknown config override key '{}'", key)),
                        }
                    }
                } else {
                    return Err(anyhow!("Unknown config override key '{}'", key));
                }
            }
        }
        Ok(())
    }

    pub fn get_journal(&self, name: &str) -> Result<&JournalConfig> {
        self.journals
            .get(name)
            .ok_or_else(|| anyhow!("No journal named '{}' configured", name))
    }

    pub fn has_editor_configured(&self) -> bool {
        if let Some(e) = &self.editor {
            if !e.trim().is_empty() { return true; }
        }
        if let Ok(e) = std::env::var("VISUAL") {
            if !e.trim().is_empty() { return true; }
        }
        if let Ok(e) = std::env::var("EDITOR") {
            if !e.trim().is_empty() { return true; }
        }
        false
    }

    pub fn resolve_editor(&self) -> String {
        if let Some(e) = &self.editor {
            if !e.trim().is_empty() { return e.clone(); }
        }
        if let Ok(e) = std::env::var("VISUAL") {
            if !e.trim().is_empty() { return e; }
        }
        if let Ok(e) = std::env::var("EDITOR") {
            if !e.trim().is_empty() { return e; }
        }
        if cfg!(windows) { "notepad".to_string() } else { "nano".to_string() }
    }
}

/// Default config file location: ~/.config/jrnl-rs/config.yaml (Linux),
/// %APPDATA%\jrnl-rs\config.yaml (Windows).
pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("jrnl-rs")
        .join("config.yaml")
}

/// Default data directory for journal files.
fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("jrnl-rs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_storage_mode_file_extension() {
        assert_eq!(infer_storage_mode(&PathBuf::from("/tmp/journal.txt")), StorageMode::File);
        assert_eq!(infer_storage_mode(&PathBuf::from("/tmp/journal")), StorageMode::Folder);
    }

    #[test]
    fn test_infer_storage_mode_trailing_slash() {
        assert_eq!(infer_storage_mode(&PathBuf::from("/tmp/journal/")), StorageMode::Folder);
        assert_eq!(infer_storage_mode(&PathBuf::from("/tmp/journal\\")), StorageMode::Folder);
    }

    #[test]
    fn test_journal_config_strips_trailing_separator() {
        let cfg = JournalConfig::new(PathBuf::from("/tmp/journal/"));
        assert!(!cfg.path.to_string_lossy().ends_with('/'));
        assert_eq!(cfg.storage, StorageMode::Folder);
    }

    #[test]
    fn test_serialize_file_journal() {
        let cfg = JournalConfig::new(PathBuf::from("/tmp/journal.txt"));
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        assert!(yaml.contains("/tmp/journal.txt"));
        assert!(!yaml.contains('/') || yaml.trim_end().ends_with(".txt"));
    }

    #[test]
    fn test_serialize_folder_journal_appends_slash() {
        let cfg = JournalConfig::new(PathBuf::from("/tmp/journal/"));
        let yaml = serde_yaml::to_string(&cfg).unwrap().trim().to_string();
        // Should serialize with trailing slash so it round-trips as folder
        assert!(yaml.ends_with('/'), "expected trailing slash, got: {}", yaml);
    }

    #[test]
    fn test_deserialize_path_only_yaml() {
        // Original jrnl format: journals section has bare path strings
        let yaml = "journals:\n  default: /tmp/journal/\n  work: /tmp/work.txt\n";
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let default_j = config.get_journal("default").unwrap();
        let work_j = config.get_journal("work").unwrap();
        assert_eq!(default_j.storage, StorageMode::Folder);
        assert_eq!(work_j.storage, StorageMode::File);
    }

    #[test]
    fn test_default_config_has_default_journal() {
        let config = Config::default();
        assert!(config.journals.contains_key("default"));
    }

    #[test]
    fn test_apply_override_editor() {
        let mut config = Config::default();
        config.apply_override("editor", "vim").unwrap();
        assert_eq!(config.editor, Some("vim".to_string()));
    }

    #[test]
    fn test_apply_override_journal_path_infers_storage() {
        let mut config = Config::default();
        config.apply_override("journals.default.path", "/tmp/journal.txt").unwrap();
        let j = config.get_journal("default").unwrap();
        assert_eq!(j.path, PathBuf::from("/tmp/journal.txt"));
        assert_eq!(j.storage, StorageMode::File);
    }

    #[test]
    fn test_apply_override_journal_path_infers_folder() {
        let mut config = Config::default();
        config.apply_override("journals.default.path", "/tmp/journal/").unwrap();
        let j = config.get_journal("default").unwrap();
        assert_eq!(j.storage, StorageMode::Folder);
    }

    #[test]
    fn test_apply_override_unknown_journal() {
        let mut config = Config::default();
        assert!(config.apply_override("journals.work.path", "/tmp/work.txt").is_err());
    }

    #[test]
    fn test_resolve_editor_default() {
        let config = Config::default();
        std::env::remove_var("VISUAL");
        std::env::remove_var("EDITOR");
        let editor = config.resolve_editor();
        assert!(editor == "nano" || editor == "notepad");
    }

    #[test]
    fn test_yaml_roundtrip() {
        let config = Config::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: Config = serde_yaml::from_str(&yaml).unwrap();
        assert!(parsed.journals.contains_key("default"));
    }
}
