use anyhow::{anyhow, Context, Result};
use crate::entry::DEFAULT_TAG_SYMBOLS;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// How a single journal stores its entries on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageMode {
    /// All entries in a single file.
    File,
    /// Entries split into journal_root/YYYY/MM/DD.txt
    Folder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalConfig {
    /// Path to a single file, or to a root folder (depending on `storage`).
    pub path: PathBuf,
    pub storage: StorageMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Map of journal name -> journal config. "default" is used when no
    /// journal name is specified on the command line.
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
    /// Maximum line width (in characters) for displayed entries, wrapping
    /// at word boundaries. 0 disables wrapping.
    #[serde(default = "default_linewrap")]
    pub linewrap: usize,
}

fn default_timeformat() -> String {
    "%Y-%m-%d %H:%M".to_string()
}

fn default_linewrap() -> usize {
    79
}

fn default_tagsymbols() -> String {
    DEFAULT_TAG_SYMBOLS.to_string()
}

impl Default for Config {
    fn default() -> Self {
        let mut journals = HashMap::new();
        let default_path = default_data_dir().join("journal.txt");
        journals.insert(
            "default".to_string(),
            JournalConfig {
                path: default_path,
                storage: StorageMode::File,
            },
        );
        Config {
            journals,
            tagsymbols: default_tagsymbols(),
            editor: None,
            timeformat: default_timeformat(),
            linewrap: default_linewrap(),
        }
    }
}

impl Config {
    /// Load config from the given path, or from the default location.
    /// If no config file exists, returns a default config (without writing it).
    pub fn load(config_file: Option<&str>) -> Result<Self> {
        let path = match config_file {
            Some(p) => PathBuf::from(p),
            None => default_config_path(),
        };

        if !path.exists() {
            return Ok(Config::default());
        }

        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file at {}", path.display()))?;
        let config: Config = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse config file at {}", path.display()))?;
        Ok(config)
    }

    /// Save this config to the given path, or to the default location.
    #[allow(dead_code)]
    pub fn save(&self, config_file: Option<&str>) -> Result<()> {
        let path = match config_file {
            Some(p) => PathBuf::from(p),
            None => default_config_path(),
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
        }
        let content = serde_yaml::to_string(self)?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write config file at {}", path.display()))?;
        Ok(())
    }

    /// Apply --config-override KEY VALUE to this config (in-memory only).
    /// Supported keys: "editor", "timeformat", "linewrap", "tagsymbols",
    /// "journals.<name>.path", "journals.<name>.storage".
    pub fn apply_override(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "editor" => self.editor = Some(value.to_string()),
            "timeformat" => self.timeformat = value.to_string(),
            "tagsymbols" => self.tagsymbols = value.to_string(),
            "linewrap" => {
                self.linewrap = value
                    .parse::<usize>()
                    .map_err(|_| anyhow!("Invalid linewrap value '{}' (expected a non-negative integer)", value))?;
            }
            other => {
                if let Some(rest) = other.strip_prefix("journals.") {
                    let mut parts = rest.splitn(2, '.');
                    let name = parts.next().unwrap_or_default();
                    let field = parts.next().unwrap_or_default();
                    let journal = self
                        .journals
                        .get_mut(name)
                        .ok_or_else(|| anyhow!("Unknown journal '{}'", name))?;
                    match field {
                        "path" => journal.path = PathBuf::from(value),
                        "storage" => {
                            journal.storage = match value.to_lowercase().as_str() {
                                "file" => StorageMode::File,
                                "folder" => StorageMode::Folder,
                                _ => return Err(anyhow!("Invalid storage mode '{}'", value)),
                            };
                        }
                        _ => return Err(anyhow!("Unknown config override key '{}'", key)),
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

    /// Whether an editor is configured via the config file, $VISUAL, or $EDITOR.
    /// Used to decide composing-mode behavior: launch editor vs. prompt on stdin.
    pub fn has_editor_configured(&self) -> bool {
        if let Some(e) = &self.editor {
            if !e.trim().is_empty() {
                return true;
            }
        }
        if let Ok(e) = std::env::var("VISUAL") {
            if !e.trim().is_empty() {
                return true;
            }
        }
        if let Ok(e) = std::env::var("EDITOR") {
            if !e.trim().is_empty() {
                return true;
            }
        }
        false
    }

    /// Resolve the editor command: explicit config, then $VISUAL, then $EDITOR,
    /// then a platform default.
    pub fn resolve_editor(&self) -> String {
        if let Some(e) = &self.editor {
            if !e.trim().is_empty() {
                return e.clone();
            }
        }
        if let Ok(e) = std::env::var("VISUAL") {
            if !e.trim().is_empty() {
                return e;
            }
        }
        if let Ok(e) = std::env::var("EDITOR") {
            if !e.trim().is_empty() {
                return e;
            }
        }
        if cfg!(windows) {
            "notepad".to_string()
        } else {
            "nano".to_string()
        }
    }
}

/// Default config file location: ~/.config/jrnl-rs/config.yaml (Linux),
/// %APPDATA%\jrnl-rs\config.yaml (Windows), via the `dirs` crate.
pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("jrnl-rs")
        .join("config.yaml")
}

/// Default data directory for journal files: ~/.local/share/jrnl-rs (Linux),
/// %APPDATA%\jrnl-rs\data (Windows).
fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("jrnl-rs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_default_journal() {
        let config = Config::default();
        assert!(config.journals.contains_key("default"));
    }

    #[test]
    fn test_default_config_has_default_tagsymbols() {
        let config = Config::default();
        assert_eq!(config.tagsymbols, DEFAULT_TAG_SYMBOLS);
    }

    #[test]
    fn test_apply_override_editor() {
        let mut config = Config::default();
        config.apply_override("editor", "vim").unwrap();
        assert_eq!(config.editor, Some("vim".to_string()));
    }

    #[test]
    fn test_apply_override_tagsymbols() {
        let mut config = Config::default();
        config.apply_override("tagsymbols", "#").unwrap();
        assert_eq!(config.tagsymbols, "#");
    }

    #[test]
    fn test_apply_override_journal_path() {
        let mut config = Config::default();
        config.apply_override("journals.default.path", "/tmp/journal.txt").unwrap();
        assert_eq!(config.get_journal("default").unwrap().path, PathBuf::from("/tmp/journal.txt"));
    }

    #[test]
    fn test_apply_override_unknown_journal() {
        let mut config = Config::default();
        let result = config.apply_override("journals.work.path", "/tmp/work.txt");
        assert!(result.is_err());
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
