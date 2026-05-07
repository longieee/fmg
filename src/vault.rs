use serde_yaml::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// A parsed markdown page from the vault.
#[derive(Debug, Clone)]
pub struct Page {
    /// Relative path from vault root.
    pub rel_path: PathBuf,
    /// Filename without `.md` extension.
    pub stem: String,
    /// Raw frontmatter key-value pairs.
    pub frontmatter: HashMap<String, Value>,
}

impl Page {
    /// Get the title: frontmatter `title` field, falling back to filename stem.
    pub fn title(&self, title_field: &str) -> String {
        self.frontmatter
            .get(title_field)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.stem.clone())
    }

    /// Get aliases from frontmatter.
    pub fn aliases(&self, alias_field: &str) -> Vec<String> {
        self.frontmatter
            .get(alias_field)
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Extract all frontmatter fields that contain arrays of `[[WikiLink]]` values.
    /// Returns field name -> list of raw link targets (with brackets stripped).
    pub fn wikilink_fields(&self) -> HashMap<String, Vec<String>> {
        let mut result = HashMap::new();
        for (key, value) in &self.frontmatter {
            let links = extract_wikilinks_from_value(value);
            if !links.is_empty() {
                result.insert(key.clone(), links);
            }
        }
        result
    }
}

/// Extract `[[WikiLink]]` targets from a YAML value.
/// Handles strings, arrays of strings, and nested structures.
fn extract_wikilinks_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => extract_wikilinks_from_str(s),
        Value::Sequence(seq) => seq.iter().flat_map(extract_wikilinks_from_value).collect(),
        _ => vec![],
    }
}

/// Extract `[[WikiLink]]` targets from a string.
/// Strips the `[[` and `]]` brackets and returns the inner text.
fn extract_wikilinks_from_str(s: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut remaining = s;
    while let Some(start) = remaining.find("[[") {
        let after_open = &remaining[start + 2..];
        if let Some(end) = after_open.find("]]") {
            let link = after_open[..end].trim().to_string();
            if !link.is_empty() {
                links.push(link);
            }
            remaining = &after_open[end + 2..];
        } else {
            break;
        }
    }
    links
}

/// Discover and parse all markdown files in a vault directory.
pub fn scan_vault(vault_root: &Path) -> Vec<Page> {
    let mut pages = Vec::new();
    for entry in WalkDir::new(vault_root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("md") {
            continue;
        }
        // Skip hidden directories (e.g., .obsidian, .trash)
        let rel = path.strip_prefix(vault_root).unwrap_or(path);
        if rel.components().any(|c| {
            c.as_os_str()
                .to_str()
                .is_some_and(|s| s.starts_with('.'))
        }) {
            continue;
        }

        if let Some(page) = parse_page(vault_root, path) {
            pages.push(page);
        }
    }
    pages
}

/// Parse a single markdown file, extracting YAML frontmatter.
fn parse_page(vault_root: &Path, path: &Path) -> Option<Page> {
    let content = std::fs::read_to_string(path).ok()?;
    let rel_path = path.strip_prefix(vault_root).ok()?.to_path_buf();
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let frontmatter = parse_frontmatter(&content);

    Some(Page {
        rel_path,
        stem,
        frontmatter,
    })
}

/// Extract YAML frontmatter from markdown content.
/// Frontmatter is delimited by `---` at the start of the file.
fn parse_frontmatter(content: &str) -> HashMap<String, Value> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return HashMap::new();
    }
    let after_open = &trimmed[3..];
    if let Some(end) = after_open.find("\n---") {
        let yaml_str = &after_open[..end];
        // Deserialize as a mapping
        serde_yaml::from_str::<HashMap<String, Value>>(yaml_str).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_wikilinks() {
        let links = extract_wikilinks_from_str("[[Foo]] and [[Bar Baz]]");
        assert_eq!(links, vec!["Foo", "Bar Baz"]);
    }

    #[test]
    fn test_extract_wikilinks_empty() {
        let links = extract_wikilinks_from_str("no links here");
        assert!(links.is_empty());
    }

    #[test]
    fn test_extract_wikilinks_nested_brackets() {
        let links = extract_wikilinks_from_str("[[Hello]] world [[ Spaces ]]");
        assert_eq!(links, vec!["Hello", "Spaces"]);
    }

    #[test]
    fn test_parse_frontmatter_basic() {
        let content = "---\ntitle: Hello World\ntags:\n  - foo\n  - bar\n---\n# Content";
        let fm = parse_frontmatter(content);
        assert_eq!(fm.get("title").unwrap().as_str().unwrap(), "Hello World");
    }

    #[test]
    fn test_parse_frontmatter_none() {
        let content = "# No frontmatter here";
        let fm = parse_frontmatter(content);
        assert!(fm.is_empty());
    }

    #[test]
    fn test_parse_frontmatter_wikilinks() {
        let content = r#"---
title: Test Page
depends_on:
  - "[[LibreChat MongoDB]]"
  - "[[LiteLLM Proxy]]"
---
# Content"#;
        let fm = parse_frontmatter(content);
        let deps = fm.get("depends_on").unwrap();
        let links = extract_wikilinks_from_value(deps);
        assert_eq!(links, vec!["LibreChat MongoDB", "LiteLLM Proxy"]);
    }
}
