//! Advanced search engine with full-text and faceted search.
//!
//! Provides an inverted index for fast module discovery, with support for
//! filtering by category, trust level, download count, and capabilities.

use super::registry::{ModuleManifest, TrustLevel};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Which field a search index entry was extracted from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchField {
    Name,
    Description,
    Tags,
    Author,
    Category,
}

/// A single entry in the inverted index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub module_name: String,
    pub field: SearchField,
    pub score: f64,
}

/// Faceted search filter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilter {
    pub categories: Vec<String>,
    pub trust_levels: Vec<TrustLevel>,
    pub min_downloads: Option<u64>,
    pub updated_after: Option<chrono::DateTime<chrono::Utc>>,
    pub has_capabilities: Vec<String>,
}

/// A single search hit with relevance score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub module_name: String,
    pub version: String,
    pub description: String,
    pub trust_level: TrustLevel,
    pub score: f64,
    pub downloads: u64,
    pub highlights: Vec<String>,
}

/// Facet counts for search results.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFacets {
    pub categories: HashMap<String, usize>,
    pub trust_levels: HashMap<String, usize>,
}

/// Paginated search results with facet counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub results: Vec<SearchHit>,
    pub total_count: usize,
    pub facets: SearchFacets,
}

/// Indexed module metadata kept alongside the inverted index.
#[derive(Debug, Clone)]
struct ModuleMeta {
    version: String,
    description: String,
    trust_level: TrustLevel,
    downloads: u64,
    categories: Vec<String>,
    capabilities: Vec<String>,
    updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Full-text and faceted search engine backed by an in-memory inverted index.
pub struct SearchEngine {
    index: HashMap<String, Vec<IndexEntry>>,
    modules: HashMap<String, ModuleMeta>,
}

impl SearchEngine {
    /// Create an empty search engine.
    pub fn new() -> Self {
        Self { index: HashMap::new(), modules: HashMap::new() }
    }

    /// Index a module manifest so it becomes searchable.
    pub fn index_module(&mut self, manifest: &ModuleManifest) {
        let name = &manifest.name;

        // Remove previous entries for this module
        self.remove_module(name);

        // Extract categories from metadata
        let categories: Vec<String> = manifest
            .metadata
            .get("categories")
            .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            .unwrap_or_default();

        let downloads: u64 =
            manifest.metadata.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);

        // Store module metadata
        self.modules.insert(
            name.clone(),
            ModuleMeta {
                version: manifest.version.to_string(),
                description: manifest.description.clone(),
                trust_level: TrustLevel::Community,
                downloads,
                categories: categories.clone(),
                capabilities: manifest.required_capabilities.clone(),
                updated_at: None,
            },
        );

        // Tokenise and insert index entries
        let add = |index: &mut HashMap<String, Vec<IndexEntry>>,
                   text: &str,
                   field: SearchField,
                   boost: f64| {
            for token in tokenize(text) {
                index.entry(token).or_default().push(IndexEntry {
                    module_name: name.clone(),
                    field: field.clone(),
                    score: boost,
                });
            }
        };

        add(&mut self.index, name, SearchField::Name, 3.0);
        add(&mut self.index, &manifest.description, SearchField::Description, 1.0);

        for kw in &manifest.keywords {
            add(&mut self.index, kw, SearchField::Tags, 2.0);
        }
        if let Some(ref author) = manifest.author {
            add(&mut self.index, author, SearchField::Author, 1.5);
        }
        for cat in &categories {
            add(&mut self.index, cat, SearchField::Category, 1.5);
        }
    }

    /// Search the index, optionally applying filters and pagination.
    pub fn search(
        &self,
        query: &str,
        filter: Option<&SearchFilter>,
        limit: usize,
        offset: usize,
    ) -> SearchResults {
        // Score each module by summing matching index entries
        let mut scores: HashMap<String, f64> = HashMap::new();
        let mut highlights: HashMap<String, Vec<String>> = HashMap::new();

        for token in tokenize(query) {
            if let Some(entries) = self.index.get(&token) {
                for entry in entries {
                    *scores.entry(entry.module_name.clone()).or_default() += entry.score;
                    let hl = highlights.entry(entry.module_name.clone()).or_default();
                    let snippet = format!("{:?}: {}", entry.field, token);
                    if !hl.contains(&snippet) {
                        hl.push(snippet);
                    }
                }
            }
        }

        // Collect candidates that have a score > 0
        let mut hits: Vec<SearchHit> = scores
            .into_iter()
            .filter_map(|(name, score)| {
                let meta = self.modules.get(&name)?;

                // Apply filters
                if let Some(f) = filter {
                    if !f.categories.is_empty()
                        && !f.categories.iter().any(|c| meta.categories.contains(c))
                    {
                        return None;
                    }
                    if !f.trust_levels.is_empty() && !f.trust_levels.contains(&meta.trust_level) {
                        return None;
                    }
                    if let Some(min_dl) = f.min_downloads {
                        if meta.downloads < min_dl {
                            return None;
                        }
                    }
                    if let Some(after) = f.updated_after {
                        if let Some(updated) = meta.updated_at {
                            if updated < after {
                                return None;
                            }
                        } else {
                            return None;
                        }
                    }
                    if !f.has_capabilities.is_empty()
                        && !f.has_capabilities.iter().all(|c| meta.capabilities.contains(c))
                    {
                        return None;
                    }
                }

                Some(SearchHit {
                    module_name: name.clone(),
                    version: meta.version.clone(),
                    description: meta.description.clone(),
                    trust_level: meta.trust_level,
                    score,
                    downloads: meta.downloads,
                    highlights: highlights.remove(&name).unwrap_or_default(),
                })
            })
            .collect();

        // Sort by score descending, then name ascending for stability
        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.module_name.cmp(&b.module_name))
        });

        let total_count = hits.len();

        // Build facets from the full (pre-pagination) result set
        let mut facets = SearchFacets::default();
        for hit in &hits {
            if let Some(meta) = self.modules.get(&hit.module_name) {
                for cat in &meta.categories {
                    *facets.categories.entry(cat.clone()).or_default() += 1;
                }
            }
            *facets.trust_levels.entry(hit.trust_level.to_string()).or_default() += 1;
        }

        // Apply pagination
        let results: Vec<SearchHit> = hits.into_iter().skip(offset).take(limit).collect();

        SearchResults { results, total_count, facets }
    }

    /// Suggest module names matching a prefix (for autocomplete).
    pub fn suggest(&self, prefix: &str) -> Vec<String> {
        let lower = prefix.to_lowercase();
        let mut suggestions: Vec<String> = self
            .modules
            .keys()
            .filter(|name| name.to_lowercase().starts_with(&lower))
            .cloned()
            .collect();
        suggestions.sort();
        suggestions
    }

    /// Remove all index entries for a module.
    pub fn remove_module(&mut self, name: &str) {
        self.modules.remove(name);
        for entries in self.index.values_mut() {
            entries.retain(|e| e.module_name != name);
        }
        self.index.retain(|_, entries| !entries.is_empty());
    }
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple whitespace + lowercase tokenizer.
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marketplace::registry::ModuleVersion;

    fn sample_manifest(name: &str, desc: &str, keywords: &[&str]) -> ModuleManifest {
        let mut m = ModuleManifest::builder(name, ModuleVersion::new(1, 0, 0))
            .description(desc)
            .author("alice")
            .build();
        m.keywords = keywords.iter().map(|s| s.to_string()).collect();
        m
    }

    #[test]
    fn test_index_and_search_basic() {
        let mut engine = SearchEngine::new();
        engine.index_module(&sample_manifest("json-parser", "Parse JSON documents", &["json"]));
        engine.index_module(&sample_manifest("csv-reader", "Read CSV files", &["csv"]));

        let results = engine.search("json", None, 10, 0);
        assert_eq!(results.total_count, 1);
        assert_eq!(results.results[0].module_name, "json-parser");
    }

    #[test]
    fn test_search_by_keyword() {
        let mut engine = SearchEngine::new();
        engine.index_module(&sample_manifest("mod-a", "Module A", &["networking"]));
        engine.index_module(&sample_manifest("mod-b", "Module B", &["crypto"]));

        let results = engine.search("networking", None, 10, 0);
        assert_eq!(results.total_count, 1);
        assert_eq!(results.results[0].module_name, "mod-a");
    }

    #[test]
    fn test_search_pagination() {
        let mut engine = SearchEngine::new();
        for i in 0..5 {
            let name = format!("test-mod-{}", i);
            engine.index_module(&sample_manifest(&name, "test module", &["test"]));
        }

        let page1 = engine.search("test", None, 2, 0);
        assert_eq!(page1.results.len(), 2);
        assert_eq!(page1.total_count, 5);

        let page2 = engine.search("test", None, 2, 2);
        assert_eq!(page2.results.len(), 2);
    }

    #[test]
    fn test_search_filter_trust_level() {
        let mut engine = SearchEngine::new();
        engine.index_module(&sample_manifest("mod-a", "Module A", &["common"]));
        // default trust is Community

        let filter =
            SearchFilter { trust_levels: vec![TrustLevel::Official], ..Default::default() };
        let results = engine.search("common", Some(&filter), 10, 0);
        assert_eq!(results.total_count, 0);

        let filter2 =
            SearchFilter { trust_levels: vec![TrustLevel::Community], ..Default::default() };
        let results2 = engine.search("common", Some(&filter2), 10, 0);
        assert_eq!(results2.total_count, 1);
    }

    #[test]
    fn test_search_filter_min_downloads() {
        let mut engine = SearchEngine::new();
        let mut m = sample_manifest("popular", "Popular module", &["hot"]);
        m.metadata.insert("downloads".to_string(), serde_json::json!(500));
        engine.index_module(&m);

        engine.index_module(&sample_manifest("niche", "Niche module", &["hot"]));

        let filter = SearchFilter { min_downloads: Some(100), ..Default::default() };
        let results = engine.search("hot", Some(&filter), 10, 0);
        assert_eq!(results.total_count, 1);
        assert_eq!(results.results[0].module_name, "popular");
    }

    #[test]
    fn test_suggest() {
        let mut engine = SearchEngine::new();
        engine.index_module(&sample_manifest("json-parser", "Parse JSON", &[]));
        engine.index_module(&sample_manifest("json-validator", "Validate JSON", &[]));
        engine.index_module(&sample_manifest("csv-reader", "Read CSV", &[]));

        let suggestions = engine.suggest("json");
        assert_eq!(suggestions.len(), 2);
        assert!(suggestions.contains(&"json-parser".to_string()));
        assert!(suggestions.contains(&"json-validator".to_string()));
    }

    #[test]
    fn test_remove_module() {
        let mut engine = SearchEngine::new();
        engine.index_module(&sample_manifest("mod-a", "Module A", &["test"]));
        engine.index_module(&sample_manifest("mod-b", "Module B", &["test"]));

        engine.remove_module("mod-a");

        let results = engine.search("test", None, 10, 0);
        assert_eq!(results.total_count, 1);
        assert_eq!(results.results[0].module_name, "mod-b");
        assert!(engine.suggest("mod-a").is_empty());
    }

    #[test]
    fn test_search_no_results() {
        let engine = SearchEngine::new();
        let results = engine.search("nonexistent", None, 10, 0);
        assert_eq!(results.total_count, 0);
        assert!(results.results.is_empty());
    }

    #[test]
    fn test_facets() {
        let mut engine = SearchEngine::new();
        let mut m = sample_manifest("mod-a", "Module A", &["shared"]);
        m.metadata.insert("categories".to_string(), serde_json::json!(["networking"]));
        engine.index_module(&m);

        let mut m2 = sample_manifest("mod-b", "Module B", &["shared"]);
        m2.metadata.insert("categories".to_string(), serde_json::json!(["crypto"]));
        engine.index_module(&m2);

        let results = engine.search("shared", None, 10, 0);
        assert_eq!(results.total_count, 2);
        assert_eq!(results.facets.categories.len(), 2);
        assert_eq!(results.facets.categories.get("networking"), Some(&1));
        assert_eq!(results.facets.categories.get("crypto"), Some(&1));
    }
}
