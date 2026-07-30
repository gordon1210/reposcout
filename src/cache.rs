//! Incremental cache: per-file results keyed by content hash, invalidated when
//! the tool version, schema, or per-file analysis profile changes. Stored under
//! the user's OS cache directory, keyed by the canonical scan root, so scanning
//! a repository never writes into it. When the platform does not expose an
//! application cache directory, caching is disabled rather than falling back
//! into the scanned repository.

use crate::config::Config;
use crate::fs_budget::{self, DEFAULT_MAX_CACHE_FILE_BYTES, ReadOutcome};
use crate::lang::{HealthInclude, HealthScope};
use crate::model::{FileReport, LineRange, SCHEMA_VERSION, SymbolOutline};
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use xxhash_rust::xxh3::xxh3_64;

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    hash: u64,
    report: FileReport,
    #[serde(default)]
    test_regions: Vec<LineRange>,
    #[serde(default)]
    symbol_outlines: Option<Vec<SymbolOutline>>,
    #[serde(default)]
    graph_facts: Option<crate::graph::SourceFacts>,
}

pub(crate) struct CachedAnalysis {
    pub report: FileReport,
    pub test_regions: Vec<LineRange>,
    pub symbol_outlines: Option<Vec<SymbolOutline>>,
    pub graph_facts: Option<crate::graph::SourceFacts>,
}

#[derive(Serialize, Deserialize, Default)]
struct CacheData {
    version: String,
    entries: HashMap<String, Entry>,
}

pub struct Cache {
    enabled: bool,
    path: PathBuf,
    key: String,
    loaded: HashMap<String, Entry>,
    fresh: Mutex<HashMap<String, Entry>>,
    hits: AtomicUsize,
    misses: AtomicUsize,
    enrichments: AtomicUsize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CacheStats {
    pub enabled: bool,
    pub hits: usize,
    pub misses: usize,
    pub enrichments: usize,
}

/// A cache location removed by a manual cache reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    Analysis,
    GitHistory,
    All,
}

impl CacheKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Analysis => "analysis",
            Self::GitHistory => "Git history",
            Self::All => "all caches",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheLocation {
    pub kind: CacheKind,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheClearScope {
    ScanRoot(PathBuf),
    All(PathBuf),
}

/// Result of an idempotent cache-clear operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheClearResult {
    pub scope: CacheClearScope,
    /// Locations that existed and were removed.
    pub removed: Vec<CacheLocation>,
}

/// Bump when cached per-file analysis facts are added or changed.
const ANALYZER_VERSION: &str = "15";

/// The configuration that can change a cached per-file analysis entry.
///
/// This deliberately excludes scan-wide settings such as duplication and
/// churn: those run after file analysis. Churn has a separate commit-event
/// cache in `git::churn_cache` rather than storing data in a `FileReport`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalysisProfile {
    /// `None` means token analysis is disabled, so the configured encoding
    /// cannot affect the cached zero value.
    token_encoding: Option<String>,
    complexity: bool,
    imports: bool,
    /// The effective marker set. Disabled marker analysis and an empty marker
    /// list both produce the same empty per-file result.
    markers: Vec<String>,
    /// Health eligibility changes cached complexity and marker facts. These
    /// fields are absent when neither analyzer is effectively enabled.
    health_scope: Option<HealthScope>,
    health_includes: Vec<HealthInclude>,
    health_excludes: Vec<String>,
}

impl AnalysisProfile {
    pub fn from_config(cfg: &Config) -> Self {
        let markers = if cfg.enabled.markers {
            canonical_markers(&cfg.markers)
        } else {
            Vec::new()
        };
        let health_enabled = cfg.enabled.complexity || !markers.is_empty();
        let health_scope = health_enabled.then_some(cfg.health_scope);
        let mut health_includes = if health_enabled && cfg.health_scope == HealthScope::Source {
            cfg.health_includes.clone()
        } else {
            Vec::new()
        };
        health_includes.sort();
        health_includes.dedup();
        let mut health_excludes = if health_enabled {
            cfg.health_excludes.clone()
        } else {
            Vec::new()
        };
        health_excludes.sort();
        health_excludes.dedup();
        AnalysisProfile {
            token_encoding: cfg
                .enabled
                .tokens
                .then(|| canonical_token_encoding(&cfg.encoding)),
            complexity: cfg.enabled.complexity,
            imports: cfg.enabled.imports,
            markers,
            health_scope,
            health_includes,
            health_excludes,
        }
    }

    fn cache_key(&self) -> String {
        let profile = serde_json::to_string(self)
            .expect("analysis profiles contain only serializable primitives");
        format!(
            "v{}|schema{}|profile{}|analyzer{}",
            env!("CARGO_PKG_VERSION"),
            SCHEMA_VERSION,
            profile,
            ANALYZER_VERSION
        )
    }
}

/// Match the aliases accepted by `TokenCounter` so equivalent token behavior
/// can reuse the same entries.
fn canonical_token_encoding(encoding: &str) -> String {
    match encoding.to_ascii_lowercase().as_str() {
        "o200k_base" | "o200k" => "o200k_base".to_string(),
        "cl100k_base" | "cl100k" => "cl100k_base".to_string(),
        other => other.to_string(),
    }
}

/// `markers::scan` ignores empty strings and is order-independent. Repeated
/// markers also overwrite the same map entry, so remove all three sources of
/// cache-key noise.
fn canonical_markers(markers: &[String]) -> Vec<String> {
    let mut markers: Vec<String> = markers
        .iter()
        .filter(|marker| !marker.is_empty())
        .cloned()
        .collect();
    markers.sort();
    markers.dedup();
    markers
}

impl Cache {
    pub fn open(root: &Path, enabled: bool, profile: AnalysisProfile) -> Self {
        let key = profile.cache_key();
        let path = cache_path(root);
        // Without an OS cache directory there is no safe place to persist
        // analysis results; never fall back into the scanned repository.
        let enabled = enabled && path.is_some();
        let path = path.unwrap_or_else(|| PathBuf::from("reposcout-cache-disabled.json"));
        let loaded = if enabled {
            load(&path, &key).unwrap_or_default()
        } else {
            HashMap::new()
        };
        Cache {
            enabled,
            path,
            key,
            loaded,
            fresh: Mutex::new(HashMap::new()),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
            enrichments: AtomicUsize::new(0),
        }
    }

    /// Return a cached report if the content hash matches. On a hit the entry
    /// is carried forward so it survives the next `save`.
    pub(crate) fn get(&self, rel: &str, hash: u64) -> Option<CachedAnalysis> {
        if !self.enabled {
            return None;
        }
        let Some(entry) = self.loaded.get(rel).filter(|e| e.hash == hash) else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            return None;
        };
        self.hits.fetch_add(1, Ordering::Relaxed);
        self.fresh
            .lock()
            .unwrap()
            .insert(rel.to_string(), entry.clone());
        Some(CachedAnalysis {
            report: entry.report.clone(),
            test_regions: entry.test_regions.clone(),
            symbol_outlines: entry.symbol_outlines.clone(),
            graph_facts: entry.graph_facts.clone(),
        })
    }

    pub(crate) fn put(
        &self,
        rel: &str,
        hash: u64,
        report: &FileReport,
        test_regions: &[LineRange],
        symbol_outlines: Option<&[SymbolOutline]>,
        graph_facts: Option<&crate::graph::SourceFacts>,
    ) {
        if !self.enabled {
            return;
        }
        self.fresh.lock().unwrap().insert(
            rel.to_string(),
            Entry {
                hash,
                report: report.clone(),
                test_regions: test_regions.to_vec(),
                symbol_outlines: symbol_outlines.map(<[SymbolOutline]>::to_vec),
                graph_facts: graph_facts.cloned(),
            },
        );
    }

    pub fn save(&self, prune_unseen: bool) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let fresh = std::mem::take(&mut *self.fresh.lock().unwrap());
        let entries = if prune_unseen {
            fresh
        } else {
            let mut entries = self.loaded.clone();
            entries.extend(fresh);
            entries
        };
        let data = CacheData {
            version: self.key.clone(),
            entries,
        };
        let bytes = serde_json::to_vec(&data)?;
        if bytes.len() as u64 > DEFAULT_MAX_CACHE_FILE_BYTES {
            // Refuse to persist an oversized cache rather than writing a partial
            // or unbounded artifact that later scans would load in full.
            return Ok(());
        }
        fs_budget::write_atomic_bytes(&self.path, &bytes)?;
        Ok(())
    }

    pub(crate) fn stats(&self) -> CacheStats {
        CacheStats {
            enabled: self.enabled,
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            enrichments: self.enrichments.load(Ordering::Relaxed),
        }
    }

    /// Record a content-identical entry that still needed one or more lazy
    /// query facts computed before it could satisfy the current request.
    pub(crate) fn record_enrichment(&self) {
        if self.enabled {
            self.enrichments.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Clear both analysis and Git-history caches for the scan root containing
/// `target`. A subpath inside a Git repository maps to the same root cache as a
/// full repository scan; standalone paths retain their own scan identity.
pub fn clear_for_target(target: &Path) -> Result<CacheClearResult> {
    let root = scan_root(target)?;
    let mut checked = Vec::new();
    if let Some(path) = cache_path(&root) {
        checked.push(CacheLocation {
            kind: CacheKind::Analysis,
            path,
        });
    }
    if let Some(path) = crate::git::churn_cache_directory(&root) {
        checked.push(CacheLocation {
            kind: CacheKind::GitHistory,
            path,
        });
    }
    let removed = clear_locations(&checked)?;
    Ok(CacheClearResult {
        scope: CacheClearScope::ScanRoot(root),
        removed,
    })
}

/// Clear every cache stored in RepoScout's OS-managed application cache
/// directory.
pub fn clear_all() -> Result<CacheClearResult> {
    let path = cache_directory().context(
        "the platform does not expose a RepoScout cache directory; clear a specific PATH instead",
    )?;
    let checked = vec![CacheLocation {
        kind: CacheKind::All,
        path: path.clone(),
    }];
    let removed = clear_locations(&checked)?;
    Ok(CacheClearResult {
        scope: CacheClearScope::All(path),
        removed,
    })
}

fn scan_root(target: &Path) -> Result<PathBuf> {
    let target = target
        .canonicalize()
        .with_context(|| format!("path not found: {}", target.display()))?;
    Ok(crate::walk::git_root(&target).unwrap_or(target))
}

fn clear_locations(locations: &[CacheLocation]) -> Result<Vec<CacheLocation>> {
    let mut removed = Vec::new();
    for location in locations {
        if remove_path(&location.path)? {
            removed.push(location.clone());
        }
    }
    Ok(removed)
}

fn remove_path(path: &Path) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect cache path {}", path.display()));
        }
    };
    let result = if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match result {
        Ok(()) => Ok(true),
        // Another process may have removed the same cache between inspection
        // and deletion. The requested end state is already satisfied.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("failed to remove cache path {}", path.display()))
        }
    }
}

fn load(path: &Path, key: &str) -> Option<HashMap<String, Entry>> {
    let bytes = match fs_budget::read_bytes_limited(path, DEFAULT_MAX_CACHE_FILE_BYTES) {
        Ok(bytes) => bytes,
        Err(ReadOutcome::NotRegularFile | ReadOutcome::Oversized(_) | ReadOutcome::Unreadable) => {
            return None;
        }
        Err(_) => return None,
    };
    let data: CacheData = serde_json::from_slice(&bytes).ok()?;
    if data.version == key {
        Some(data.entries)
    } else {
        None
    }
}

/// Where the on-disk cache for `root` lives. Kept in the user's OS cache
/// directory (keyed by the canonical root path) so scanning never pollutes the
/// repository being analyzed. Returns `None` when the platform does not expose
/// an application cache directory.
fn cache_path(root: &Path) -> Option<PathBuf> {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let directory = cache_directory()?;
    let id = xxh3_64(canonical.to_string_lossy().as_bytes());
    Some(directory.join(format!("{id:016x}.json")))
}

fn cache_directory() -> Option<PathBuf> {
    ProjectDirs::from("", "", "reposcout").map(|dirs| dirs.cache_dir().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::{
        AnalysisProfile, Cache, CacheKind, CacheLocation, Entry, clear_locations, load, scan_root,
    };
    use crate::config::{Config, Enabled};
    use crate::lang::{HealthInclude, HealthScope};
    use crate::model::{FileReport, LineRange, SymbolOutline};
    use std::collections::{BTreeMap, HashMap};
    use std::path::PathBuf;

    fn report(path: &str) -> FileReport {
        FileReport {
            path: PathBuf::from(path),
            language: "Rust".to_string(),
            bytes: 0,
            tokens: 0,
            loc: 0,
            sloc: 0,
            comment_lines: 0,
            comment_ratio: 0.0,
            line_metrics_approximate: false,
            complexity: None,
            imports: Vec::new(),
            markers: BTreeMap::new(),
            marker_occurrences: Vec::new(),
            churn: None,
            approximate: false,
            symbols: None,
            skip_hint: None,
            has_inline_tests: false,
        }
    }

    fn entry(path: &str, hash: u64) -> Entry {
        Entry {
            hash,
            report: report(path),
            test_regions: Vec::new(),
            symbol_outlines: None,
            graph_facts: None,
        }
    }

    #[test]
    fn profile_canonicalizes_equivalent_marker_settings() {
        let first = Config {
            markers: vec![
                "FIXME".to_string(),
                String::new(),
                "TODO".to_string(),
                "TODO".to_string(),
            ],
            ..Config::default()
        };

        let second = Config {
            markers: vec!["TODO".to_string(), "FIXME".to_string()],
            ..Config::default()
        };

        assert_eq!(
            AnalysisProfile::from_config(&first),
            AnalysisProfile::from_config(&second)
        );
    }

    #[test]
    fn profile_ignores_marker_settings_when_the_analyzer_is_effectively_empty() {
        let disabled = Config {
            enabled: Enabled {
                markers: false,
                ..Enabled::default()
            },
            markers: vec!["TODO".to_string()],
            ..Config::default()
        };

        let empty = Config {
            markers: vec![String::new()],
            ..Config::default()
        };

        assert_eq!(
            AnalysisProfile::from_config(&disabled),
            AnalysisProfile::from_config(&empty)
        );
    }

    #[test]
    fn profile_tracks_every_configured_per_file_analyzer() {
        let base = Config::default();
        let base_profile = AnalysisProfile::from_config(&base);

        let mut tokens_disabled = base.clone();
        tokens_disabled.enabled.tokens = false;
        assert_ne!(base_profile, AnalysisProfile::from_config(&tokens_disabled));

        let mut different_encoding = base.clone();
        different_encoding.encoding = "cl100k_base".to_string();
        assert_ne!(
            base_profile,
            AnalysisProfile::from_config(&different_encoding)
        );

        let mut complexity_disabled = base.clone();
        complexity_disabled.enabled.complexity = false;
        assert_ne!(
            base_profile,
            AnalysisProfile::from_config(&complexity_disabled)
        );

        let mut imports_disabled = base.clone();
        imports_disabled.enabled.imports = false;
        assert_ne!(
            base_profile,
            AnalysisProfile::from_config(&imports_disabled)
        );

        let mut markers_changed = base.clone();
        markers_changed.markers = vec!["NOTE".to_string()];
        assert_ne!(base_profile, AnalysisProfile::from_config(&markers_changed));

        let mut marker_scope_changed = base.clone();
        marker_scope_changed.health_scope = HealthScope::All;
        assert_ne!(
            base_profile,
            AnalysisProfile::from_config(&marker_scope_changed)
        );

        let mut marker_include_changed = base.clone();
        marker_include_changed.health_includes = vec![HealthInclude::Json];
        assert_ne!(
            base_profile,
            AnalysisProfile::from_config(&marker_include_changed)
        );

        let mut health_exclude_changed = base.clone();
        health_exclude_changed.health_excludes = vec!["vendor/**".to_string()];
        assert_ne!(
            base_profile,
            AnalysisProfile::from_config(&health_exclude_changed)
        );

        let context_with_narrow_analyzers = Config {
            context: true,
            enabled: Enabled {
                tokens: true,
                ..Enabled::none()
            },
            ..Config::default()
        };
        let without_context = Config {
            context: false,
            ..context_with_narrow_analyzers.clone()
        };
        assert_eq!(
            AnalysisProfile::from_config(&context_with_narrow_analyzers),
            AnalysisProfile::from_config(&without_context)
        );
    }

    #[test]
    fn profile_tracks_health_selection_for_complexity_without_markers() {
        let base = Config {
            enabled: Enabled {
                complexity: true,
                ..Enabled::none()
            },
            ..Config::default()
        };
        let base_profile = AnalysisProfile::from_config(&base);

        let scope_changed = Config {
            health_scope: HealthScope::All,
            ..base.clone()
        };
        assert_ne!(base_profile, AnalysisProfile::from_config(&scope_changed));

        let include_changed = Config {
            health_includes: vec![HealthInclude::Json],
            ..base
        };
        assert_ne!(base_profile, AnalysisProfile::from_config(&include_changed));
    }

    #[test]
    fn profile_ignores_redundant_includes_for_all_content_health_scope() {
        let all = Config {
            health_scope: HealthScope::All,
            ..Config::default()
        };
        let all_with_include = Config {
            health_includes: vec![HealthInclude::Json],
            ..all.clone()
        };

        assert_eq!(
            AnalysisProfile::from_config(&all),
            AnalysisProfile::from_config(&all_with_include)
        );
    }

    #[test]
    fn profile_canonicalizes_token_encoding_aliases() {
        let alias = Config {
            encoding: "O200K".to_string(),
            ..Config::default()
        };

        let canonical = Config::default();

        assert_eq!(
            AnalysisProfile::from_config(&alias),
            AnalysisProfile::from_config(&canonical)
        );
    }

    #[test]
    fn profile_ignores_summary_only_function_threshold() {
        let base = Config::default();
        let stricter = Config {
            max_complexity: 5,
            ..Config::default()
        };

        assert_eq!(
            AnalysisProfile::from_config(&base),
            AnalysisProfile::from_config(&stricter)
        );
    }

    #[test]
    fn scoped_save_preserves_unseen_entries_but_full_save_prunes_them() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        let loaded = HashMap::from([("old.rs".to_string(), entry("old.rs", 1))]);
        let cache = Cache {
            enabled: true,
            path: path.clone(),
            key: "test-key".to_string(),
            loaded: loaded.clone(),
            fresh: Default::default(),
            hits: Default::default(),
            misses: Default::default(),
            enrichments: Default::default(),
        };
        cache.put("new.rs", 2, &report("new.rs"), &[], None, None);
        cache.save(false).unwrap();
        let merged = load(&path, "test-key").unwrap();
        assert_eq!(merged.len(), 2);
        assert!(merged.contains_key("old.rs"));
        assert!(merged.contains_key("new.rs"));

        let cache = Cache {
            enabled: true,
            path,
            key: "test-key".to_string(),
            loaded,
            fresh: Default::default(),
            hits: Default::default(),
            misses: Default::default(),
            enrichments: Default::default(),
        };
        cache.put("new.rs", 2, &report("new.rs"), &[], None, None);
        cache.save(true).unwrap();
        let pruned = load(&cache.path, "test-key").unwrap();
        assert_eq!(
            pruned.keys().map(String::as_str).collect::<Vec<_>>(),
            ["new.rs"]
        );
    }

    #[test]
    fn cached_context_facts_round_trip_symbol_outlines() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache {
            enabled: true,
            path: dir.path().join("cache.json"),
            key: "test-key".to_string(),
            loaded: Default::default(),
            fresh: Default::default(),
            hits: Default::default(),
            misses: Default::default(),
            enrichments: Default::default(),
        };
        let outline = SymbolOutline {
            name: "PublicValue".to_string(),
            kind: "type".to_string(),
            signature: "pub struct PublicValue".to_string(),
            line: 3,
            exported: true,
            reasons: vec!["exported/public declaration".to_string()],
        };

        cache.put(
            "lib.rs",
            42,
            &report("lib.rs"),
            &[LineRange { start: 8, end: 12 }],
            Some(&[outline]),
            None,
        );
        cache.save(true).unwrap();
        let loaded = Cache {
            enabled: true,
            path: cache.path.clone(),
            key: "test-key".to_string(),
            loaded: load(&cache.path, "test-key").unwrap(),
            fresh: Default::default(),
            hits: Default::default(),
            misses: Default::default(),
            enrichments: Default::default(),
        };
        let cached = loaded.get("lib.rs", 42).unwrap();

        let outlines = cached.symbol_outlines.unwrap();
        assert_eq!(outlines.len(), 1);
        assert_eq!(outlines[0].name, "PublicValue");
        assert_eq!(outlines[0].line, 3);
        assert!(outlines[0].exported);
        assert_eq!(cached.test_regions.len(), 1);
        assert_eq!(cached.test_regions[0].start, 8);
        assert_eq!(cached.test_regions[0].end, 12);
    }

    #[test]
    fn cached_graph_facts_round_trip_without_changing_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache {
            enabled: true,
            path: dir.path().join("cache.json"),
            key: "test-key".to_string(),
            loaded: Default::default(),
            fresh: Default::default(),
            hits: Default::default(),
            misses: Default::default(),
            enrichments: Default::default(),
        };
        let facts = crate::graph::extract_source_facts(
            crate::lang::FirstClass::Rust,
            "lib.rs",
            "pub trait Service {}\npub struct App;\nimpl Service for App {}\n",
        );
        cache.put("lib.rs", 42, &report("lib.rs"), &[], None, Some(&facts));
        cache.save(true).unwrap();

        let loaded = Cache {
            enabled: true,
            path: cache.path.clone(),
            key: "test-key".to_string(),
            loaded: load(&cache.path, "test-key").unwrap(),
            fresh: Default::default(),
            hits: Default::default(),
            misses: Default::default(),
            enrichments: Default::default(),
        };
        let cached = loaded.get("lib.rs", 42).unwrap();

        assert!(cached.graph_facts.is_some());
        assert_eq!(loaded.stats().hits, 1);
        assert_eq!(loaded.stats().misses, 0);
    }

    #[test]
    fn cache_clear_removes_only_requested_locations_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let analysis = dir.path().join("analysis.json");
        let history = dir.path().join("churn/repository");
        let unrelated = dir.path().join("keep.txt");
        std::fs::write(&analysis, "cached").unwrap();
        std::fs::create_dir_all(&history).unwrap();
        std::fs::write(history.join("events.json"), "cached").unwrap();
        std::fs::write(&unrelated, "keep").unwrap();
        let locations = vec![
            CacheLocation {
                kind: CacheKind::Analysis,
                path: analysis.clone(),
            },
            CacheLocation {
                kind: CacheKind::GitHistory,
                path: history.clone(),
            },
        ];

        let removed = clear_locations(&locations).unwrap();
        assert_eq!(removed, locations);
        assert!(!analysis.exists());
        assert!(!history.exists());
        assert!(unrelated.exists());

        assert!(clear_locations(&locations).unwrap().is_empty());
    }

    #[test]
    fn analysis_cache_never_falls_back_into_the_repository() {
        // The public path helper is private; assert via open() when the OS cache
        // directory is available that the cache path is outside the scan root.
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(
            dir.path(),
            true,
            AnalysisProfile::from_config(&Config::default()),
        );
        if cache.stats().enabled {
            assert!(
                !cache.path.starts_with(dir.path()),
                "enabled analysis cache must not live under the scanned repository"
            );
            assert!(!cache.path.ends_with(".reposcout/cache.json"));
        }
    }

    #[test]
    fn cache_clear_uses_the_same_git_root_for_repository_subpaths() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let nested = dir.path().join("src/nested");
        std::fs::create_dir_all(&nested).unwrap();

        assert_eq!(
            scan_root(&nested).unwrap(),
            dir.path().canonicalize().unwrap()
        );

        let standalone = tempfile::tempdir().unwrap();
        let standalone_nested = standalone.path().join("nested");
        std::fs::create_dir_all(&standalone_nested).unwrap();
        assert_eq!(
            scan_root(&standalone_nested).unwrap(),
            standalone_nested.canonicalize().unwrap()
        );
    }
}
