use serde_yaml::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// A typed cross-service (runtime) edge parsed from a `cross_service:` frontmatter field.
///
/// Carries per-edge attributes a `[[WikiLink]]` array cannot: type, endpoint, enabling
/// condition (the §9.2 disabled-path-as-live guard), and code/config provenance.
#[derive(Debug, Clone)]
pub struct CrossEdge {
    pub target: String,
    pub edge_type: String,
    pub endpoint: Option<String>,
    pub condition: Option<String>,
    pub provenance: Option<String>,
}

/// A parsed markdown page from the vault.
#[derive(Debug, Clone)]
pub struct Page {
    /// Relative path from vault root.
    pub rel_path: PathBuf,
    /// Filename without `.md` extension.
    pub stem: String,
    /// Raw frontmatter key-value pairs.
    pub frontmatter: HashMap<String, Value>,
    /// Markdown body text (after frontmatter). Only populated when needed.
    pub body: String,
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

    /// Extract all `[[WikiLink]]` targets from the body text.
    pub fn body_wikilinks(&self) -> Vec<String> {
        extract_wikilinks_from_str(&self.body)
    }

    /// Extract all frontmatter fields that contain arrays of `[[WikiLink]]` values.
    /// Returns field name -> list of raw link targets (with brackets stripped).
    pub fn wikilink_fields(&self) -> HashMap<String, Vec<String>> {
        let mut result = HashMap::new();
        for (key, value) in &self.frontmatter {
            // The runtime layer (`cross_service`) is object-valued, not [[WikiLink]] arrays —
            // exclude it so it never leaks into the structural edge set.
            if key == "cross_service" {
                continue;
            }
            let links = extract_wikilinks_from_value(value);
            if !links.is_empty() {
                result.insert(key.clone(), links);
            }
        }
        result
    }

    /// Parse the `cross_service:` frontmatter field: an array of maps, each a typed
    /// cross-service edge. Returns empty if the field is absent or malformed.
    pub fn cross_service_edges(&self) -> Vec<CrossEdge> {
        let Some(Value::Sequence(seq)) = self.frontmatter.get("cross_service") else {
            return vec![];
        };
        let mut out = Vec::new();
        for item in seq {
            let Value::Mapping(m) = item else { continue };
            let get = |k: &str| m.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
            let Some(target_raw) = get("target") else { continue };
            // Accept "[[Target]]" or bare "Target".
            let target = target_raw
                .trim()
                .trim_start_matches("[[")
                .trim_end_matches("]]")
                .trim()
                .to_string();
            if target.is_empty() {
                continue;
            }
            out.push(CrossEdge {
                target,
                edge_type: get("type").unwrap_or_else(|| "cross_service".to_string()),
                endpoint: get("endpoint"),
                condition: get("condition"),
                provenance: get("provenance"),
            });
        }
        out
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
    // Sort by path so node indices — and thus all output ordering — are deterministic,
    // independent of filesystem walk order.
    pages.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
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

    let (frontmatter, body) = parse_frontmatter_and_body(&content);

    Some(Page {
        rel_path,
        stem,
        frontmatter,
        body,
    })
}

/// Extract YAML frontmatter and body from markdown content.
fn parse_frontmatter_and_body(content: &str) -> (HashMap<String, Value>, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (HashMap::new(), content.to_string());
    }
    let after_open = &trimmed[3..];
    if let Some(end) = after_open.find("\n---") {
        let yaml_str = &after_open[..end];
        let fm = serde_yaml::from_str::<HashMap<String, Value>>(yaml_str).unwrap_or_default();
        let body_start = end + 4; // skip "\n---"
        let body = if body_start < after_open.len() {
            after_open[body_start..].to_string()
        } else {
            String::new()
        };
        (fm, body)
    } else {
        (HashMap::new(), content.to_string())
    }
}

/// Extract YAML frontmatter from markdown content.
/// Frontmatter is delimited by `---` at the start of the file.
fn parse_frontmatter(content: &str) -> HashMap<String, Value> {
    parse_frontmatter_and_body(content).0
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
