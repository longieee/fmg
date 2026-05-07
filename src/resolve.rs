use std::collections::HashMap;

use crate::vault::Page;

/// Index for resolving `[[WikiLink]]` targets to page indices.
/// Priority: exact title → alias → filename stem → case-insensitive title.
pub struct Resolver {
    /// title (lowercase) → page index
    title_map: HashMap<String, usize>,
    /// alias (lowercase) → page index
    alias_map: HashMap<String, usize>,
    /// filename stem (lowercase) → page index
    stem_map: HashMap<String, usize>,
}

impl Resolver {
    /// Build a resolver index from a slice of pages.
    pub fn new(pages: &[Page], title_field: &str, alias_field: &str) -> Self {
        let mut title_map = HashMap::new();
        let mut alias_map = HashMap::new();
        let mut stem_map = HashMap::new();

        for (idx, page) in pages.iter().enumerate() {
            // Index by title
            let title = page.title(title_field);
            title_map.entry(title.to_lowercase()).or_insert(idx);

            // Index by aliases
            for alias in page.aliases(alias_field) {
                alias_map.entry(alias.to_lowercase()).or_insert(idx);
            }

            // Index by filename stem
            stem_map
                .entry(page.stem.to_lowercase())
                .or_insert(idx);
        }

        Self {
            title_map,
            alias_map,
            stem_map,
        }
    }

    /// Resolve a wikilink target to a page index.
    /// Returns `None` if the link cannot be resolved (external node).
    pub fn resolve(&self, link: &str) -> Option<usize> {
        let lower = link.to_lowercase();

        // 1. Exact title match (case-insensitive)
        if let Some(&idx) = self.title_map.get(&lower) {
            return Some(idx);
        }

        // 2. Alias match
        if let Some(&idx) = self.alias_map.get(&lower) {
            return Some(idx);
        }

        // 3. Filename stem match
        if let Some(&idx) = self.stem_map.get(&lower) {
            return Some(idx);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Page;
    use serde_yaml::Value;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_page(stem: &str, title: Option<&str>, aliases: Vec<&str>) -> Page {
        let mut fm = HashMap::new();
        if let Some(t) = title {
            fm.insert("title".to_string(), Value::String(t.to_string()));
        }
        if !aliases.is_empty() {
            fm.insert(
                "aliases".to_string(),
                Value::Sequence(
                    aliases
                        .into_iter()
                        .map(|a| Value::String(a.to_string()))
                        .collect(),
                ),
            );
        }
        Page {
            rel_path: PathBuf::from(format!("{stem}.md")),
            stem: stem.to_string(),
            frontmatter: fm,
            body: String::new(),
        }
    }

    #[test]
    fn test_resolve_by_title() {
        let pages = vec![make_page("librechat", Some("LibreChat (HelperAI)"), vec![])];
        let resolver = Resolver::new(&pages, "title", "aliases");
        assert_eq!(resolver.resolve("LibreChat (HelperAI)"), Some(0));
        assert_eq!(resolver.resolve("librechat (helperai)"), Some(0));
    }

    #[test]
    fn test_resolve_by_alias() {
        let pages = vec![make_page("mongo", Some("MongoDB"), vec!["Mongo", "mongo-db"])];
        let resolver = Resolver::new(&pages, "title", "aliases");
        assert_eq!(resolver.resolve("Mongo"), Some(0));
        assert_eq!(resolver.resolve("mongo-db"), Some(0));
    }

    #[test]
    fn test_resolve_by_stem() {
        let pages = vec![make_page("my-note", None, vec![])];
        let resolver = Resolver::new(&pages, "title", "aliases");
        assert_eq!(resolver.resolve("my-note"), Some(0));
    }

    #[test]
    fn test_resolve_unresolved() {
        let pages = vec![make_page("foo", Some("Foo"), vec![])];
        let resolver = Resolver::new(&pages, "title", "aliases");
        assert_eq!(resolver.resolve("Nonexistent"), None);
    }
}
