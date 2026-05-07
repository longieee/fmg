use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Optional configuration from `.fmg.toml` at the vault root.
/// Everything has sensible defaults — fmg works without any config.
#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub resolve: ResolveConfig,
    #[serde(default)]
    pub fields: FieldsConfig,
    #[serde(default)]
    pub display: DisplayConfig,
}

#[derive(Debug, Deserialize)]
pub struct ResolveConfig {
    #[serde(default = "default_title_field")]
    pub title_field: String,
    #[serde(default = "default_alias_field")]
    pub alias_field: String,
    #[serde(default = "default_fallback")]
    pub fallback: String,
}

impl Default for ResolveConfig {
    fn default() -> Self {
        Self {
            title_field: default_title_field(),
            alias_field: default_alias_field(),
            fallback: default_fallback(),
        }
    }
}

fn default_title_field() -> String {
    "title".to_string()
}

fn default_alias_field() -> String {
    "aliases".to_string()
}

fn default_fallback() -> String {
    "filename_stem".to_string()
}

#[derive(Debug, Deserialize, Default)]
pub struct FieldsConfig {
    /// If set, only these fields become edges. Otherwise, auto-detect.
    pub edges: Option<Vec<String>>,
    /// Override directionality per field.
    #[serde(default)]
    pub direction: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct DisplayConfig {
    #[serde(default = "default_depth")]
    pub default_depth: u32,
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            default_depth: default_depth(),
            max_depth: default_max_depth(),
        }
    }
}

fn default_depth() -> u32 {
    1
}

fn default_max_depth() -> u32 {
    10
}

impl Config {
    /// Load config from `.fmg.toml` in the given vault root, or return defaults.
    pub fn load(vault_root: &Path) -> Self {
        let config_path = vault_root.join(".fmg.toml");
        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(config) => config,
                    Err(e) => {
                        eprintln!("Warning: failed to parse .fmg.toml: {e}");
                        Config::default()
                    }
                },
                Err(e) => {
                    eprintln!("Warning: failed to read .fmg.toml: {e}");
                    Config::default()
                }
            }
        } else {
            Config::default()
        }
    }

    /// Get the directionality for a relationship field.
    pub fn direction_for(&self, field: &str) -> Direction {
        if let Some(dir) = self.fields.direction.get(field) {
            match dir.as_str() {
                "bidirectional" => Direction::Bidirectional,
                "forward" => Direction::Forward,
                _ => default_direction(field),
            }
        } else {
            default_direction(field)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Bidirectional,
}

/// Default directionality based on known field semantics from the spec.
fn default_direction(field: &str) -> Direction {
    match field {
        "related_to" | "seeAlso" => Direction::Bidirectional,
        _ => Direction::Forward,
    }
}
