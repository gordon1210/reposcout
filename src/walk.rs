//! File discovery. Uses the `ignore` crate for gitignore-aware traversal and
//! `git2` to locate the repository root.

use crate::config::Config;
use crate::debug_log;
use crate::fs_budget::{self, ReadOutcome};
use crate::lang;
use anyhow::{Context, Result};
use ignore::DirEntry;
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Common dependency lockfiles skipped by default: they are generated, huge,
/// and dominate token/duplication counts without reflecting authored code.
const LOCKFILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lock",
    "bun.lockb",
    "go.sum",
    "poetry.lock",
    "Pipfile.lock",
    "pdm.lock",
    "uv.lock",
    "composer.lock",
    "Gemfile.lock",
    "Podfile.lock",
    "flake.lock",
    "packages.lock.json",
    "deno.lock",
    "mix.lock",
    "pubspec.lock",
    "gradle.lockfile",
];

pub(crate) fn is_lockfile(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| LOCKFILES.contains(&name))
}

/// A file selected for analysis, with separate filesystem and public identities.
pub struct DiscoveredFile {
    /// Absolute path used to read the file and query git metadata.
    pub absolute_path: PathBuf,
    /// Stable relative path used in reports, cache keys, and cross-file data.
    pub report_path: PathBuf,
}

pub struct Discovered {
    /// Git working-tree root, or the target itself if not in a repo.
    pub root: PathBuf,
    /// Canonicalized scan target (may be a subdir/file of `root`).
    pub target: PathBuf,
    pub files: Vec<DiscoveredFile>,
    /// Files observed before resource limits removed or truncated entries.
    pub observed_files: usize,
    /// Number of traversal errors skipped while discovering files.
    pub walker_errors: usize,
    /// Recognized files skipped because they exceed `max_file_bytes`.
    pub oversized_files: usize,
    /// Aggregate bytes in recognized files skipped as individually oversized.
    pub oversized_bytes: u64,
    /// Known files omitted after a file-count or aggregate-byte limit was reached.
    pub files_omitted_by_limit: usize,
    /// Traversal stopped before the exact omitted-file count could be established.
    pub files_omitted_count_incomplete: bool,
    /// Aggregate known bytes omitted by resource limits.
    pub bytes_omitted_by_limit: u64,
    /// Discovery ended before all eligible entries were accepted.
    pub scan_truncated: bool,
    /// Discovery stopped because the cooperative scan deadline elapsed.
    pub duration_limit_reached: bool,
}

struct DiscoveryPaths {
    target: PathBuf,
    root: PathBuf,
    report_base: Option<PathBuf>,
    exclusions: HashSet<PathBuf>,
}

#[derive(Default)]
struct DiscoveryStats {
    observed_files: usize,
    accepted_bytes: u64,
    walker_errors: usize,
    oversized_files: usize,
    oversized_bytes: u64,
    files_omitted_by_limit: usize,
    files_omitted_count_incomplete: bool,
    bytes_omitted_by_limit: u64,
    scan_truncated: bool,
    duration_limit_reached: bool,
}

enum WalkControl {
    Continue,
    Stop,
}

pub(crate) enum BoundedText {
    Content(String),
    Oversized(u64),
    Unreadable,
}

pub(crate) fn read_text_bounded(path: &Path, max_bytes: u64) -> BoundedText {
    match fs_budget::read_text_limited(path, max_bytes) {
        ReadOutcome::Content(content) => BoundedText::Content(content),
        ReadOutcome::Oversized(bytes) => BoundedText::Oversized(bytes),
        ReadOutcome::NotRegularFile
        | ReadOutcome::Unreadable
        | ReadOutcome::BudgetExceeded
        | ReadOutcome::DeadlineExceeded => BoundedText::Unreadable,
    }
}

/// Locate the git working-tree root containing `target`, if any.
#[must_use]
pub fn git_root(target: &Path) -> Option<PathBuf> {
    let repo = git2::Repository::discover(target).ok()?;
    repo.workdir().map(Path::to_path_buf)
}

/// Walk `target`, honoring configuration (gitignore, hidden files, excludes).
///
/// # Errors
///
/// Returns an error when the target cannot be resolved or the configured
/// walker and resource limits cannot be initialized.
pub fn discover(target: &Path, cfg: &Config) -> Result<Discovered> {
    discover_with_exclusions(target, cfg, &[])
}

/// Walk `target`, omitting files whose filesystem identities exactly match an exclusion.
///
/// # Errors
///
/// Returns an error when the target or an exclusion cannot be resolved, or the
/// configured walker and resource limits cannot be initialized.
pub fn discover_with_exclusions(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
) -> Result<Discovered> {
    let started = Instant::now();
    let deadline = Some(
        started
            .checked_add(Duration::from_secs(cfg.max_scan_seconds))
            .unwrap_or(started),
    );
    discover_with_exclusions_until(target, cfg, exclusions, deadline)
}

pub(crate) fn discover_with_exclusions_until(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    deadline: Option<Instant>,
) -> Result<Discovered> {
    let paths = resolve_discovery_paths(target, exclusions)?;
    let builder = build_walker(&paths.target, cfg)?;
    let (mut files, stats) = collect_files(&builder, &paths, cfg, deadline);
    files.sort_by(|left, right| {
        left.report_path
            .cmp(&right.report_path)
            .then_with(|| left.absolute_path.cmp(&right.absolute_path))
    });
    Ok(Discovered {
        root: paths.root,
        target: paths.target,
        files,
        observed_files: stats.observed_files,
        walker_errors: stats.walker_errors,
        oversized_files: stats.oversized_files,
        oversized_bytes: stats.oversized_bytes,
        files_omitted_by_limit: stats.files_omitted_by_limit,
        files_omitted_count_incomplete: stats.files_omitted_count_incomplete,
        bytes_omitted_by_limit: stats.bytes_omitted_by_limit,
        scan_truncated: stats.scan_truncated,
        duration_limit_reached: stats.duration_limit_reached,
    })
}

fn resolve_discovery_paths(target: &Path, exclusions: &[PathBuf]) -> Result<DiscoveryPaths> {
    let target = target
        .canonicalize()
        .with_context(|| format!("path not found: {}", target.display()))?;
    let exclusions = exclusions
        .iter()
        .map(|path| exact_path_identity(path))
        .collect::<Result<HashSet<_>>>()?;
    let repo_root = git_root(&target);
    let root = repo_root.clone().unwrap_or_else(|| target.clone());
    let report_base = repo_root.or_else(|| target.is_dir().then(|| target.clone()));
    Ok(DiscoveryPaths {
        target,
        root,
        report_base,
        exclusions,
    })
}

fn build_walker(target: &Path, cfg: &Config) -> Result<WalkBuilder> {
    let load_repo_ignores = cfg.load_repository_ignores && cfg.respect_gitignore;
    let mut builder = WalkBuilder::new(target);
    builder
        .hidden(!cfg.include_hidden)
        .git_ignore(load_repo_ignores)
        .git_global(load_repo_ignores)
        .git_exclude(load_repo_ignores)
        .ignore(load_repo_ignores)
        .parents(load_repo_ignores)
        .follow_links(false);
    // `.reposcoutignore` is repository-owned and only loaded when ignore policy
    // is enabled. Safe scans deliberately skip all repository ignore files.
    if cfg.load_repository_ignores {
        builder.add_custom_ignore_filename(".reposcoutignore");
    }

    let mut exclude_globs: Vec<String> = Vec::new();
    if cfg.exclude_lockfiles {
        // A leading '!' turns an override into an ignore rule; a slash-free
        // pattern matches the file name at any depth (gitignore semantics).
        exclude_globs.extend(LOCKFILES.iter().map(|name| format!("!{name}")));
    }
    exclude_globs.extend(cfg.extra_excludes.iter().map(|pat| format!("!{pat}")));

    if !exclude_globs.is_empty() {
        let mut ob = OverrideBuilder::new(target);
        for glob in &exclude_globs {
            ob.add(glob)
                .with_context(|| format!("invalid exclude glob: {glob}"))?;
        }
        builder.overrides(ob.build().context("building exclude overrides")?);
    }
    Ok(builder)
}

fn collect_files(
    builder: &WalkBuilder,
    paths: &DiscoveryPaths,
    cfg: &Config,
    deadline: Option<Instant>,
) -> (Vec<DiscoveredFile>, DiscoveryStats) {
    let mut files = Vec::new();
    let mut stats = DiscoveryStats::default();
    for result in builder.build() {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            stats.duration_limit_reached = true;
            stats.scan_truncated = true;
            stats.files_omitted_count_incomplete = true;
            break;
        }
        let Ok(entry) = result else {
            stats.walker_errors = stats.walker_errors.saturating_add(1);
            continue;
        };
        if entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
            && matches!(
                process_file(entry, paths, cfg, &mut files, &mut stats),
                WalkControl::Stop
            )
        {
            break;
        }
    }
    (files, stats)
}

fn process_file(
    entry: DirEntry,
    paths: &DiscoveryPaths,
    cfg: &Config,
    files: &mut Vec<DiscoveredFile>,
    stats: &mut DiscoveryStats,
) -> WalkControl {
    let absolute_path = entry.into_path();
    if paths.exclusions.contains(&absolute_path) {
        return WalkControl::Continue;
    }
    stats.observed_files = stats.observed_files.saturating_add(1);
    if stats.observed_files > cfg.max_files {
        stats.files_omitted_by_limit = stats.files_omitted_by_limit.saturating_add(1);
        stats.files_omitted_count_incomplete = true;
        stats.scan_truncated = true;
        return WalkControl::Stop;
    }
    let report_path = report_path(&absolute_path, paths.report_base.as_deref());
    if lang::detect(&report_path).is_some() && !accept_recognized_file(&absolute_path, cfg, stats) {
        return WalkControl::Continue;
    }
    log_discovery_progress(
        files.len().saturating_add(1),
        &report_path,
        stats.walker_errors,
    );
    files.push(DiscoveredFile {
        absolute_path,
        report_path,
    });
    WalkControl::Continue
}

fn report_path(absolute: &Path, base: Option<&Path>) -> PathBuf {
    base.and_then(|base| absolute.strip_prefix(base).ok())
        .filter(|path| !path.as_os_str().is_empty())
        .map_or_else(|| fallback_report_path(absolute), Path::to_path_buf)
}

fn accept_recognized_file(path: &Path, cfg: &Config, stats: &mut DiscoveryStats) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        stats.walker_errors = stats.walker_errors.saturating_add(1);
        return false;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        stats.walker_errors = stats.walker_errors.saturating_add(1);
        return false;
    }
    let bytes = metadata.len();
    if bytes > cfg.max_file_bytes {
        stats.oversized_files = stats.oversized_files.saturating_add(1);
        stats.oversized_bytes = stats.oversized_bytes.saturating_add(bytes);
        stats.scan_truncated = true;
        return false;
    }
    if stats.accepted_bytes.saturating_add(bytes) > cfg.max_total_bytes {
        stats.files_omitted_by_limit = stats.files_omitted_by_limit.saturating_add(1);
        stats.bytes_omitted_by_limit = stats.bytes_omitted_by_limit.saturating_add(bytes);
        stats.scan_truncated = true;
        return false;
    }
    stats.accepted_bytes = stats.accepted_bytes.saturating_add(bytes);
    true
}

fn log_discovery_progress(count: usize, path: &Path, walker_errors: usize) {
    if !debug_log::enabled() || (count != 1 && !count.is_multiple_of(1_000)) {
        return;
    }
    let latest = path.to_string_lossy();
    debug_log::event("discovery_progress", || {
        serde_json::json!({
            "files": count,
            "latest_path": latest.as_ref(),
            "walker_errors": walker_errors,
        })
    });
}

/// Resolve an existing or future path to the exact identity used by discovery.
///
/// # Errors
///
/// Returns an error when the path or its nearest existing parent cannot be
/// resolved to a stable filesystem identity.
pub fn exact_path_identity(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(path)
    };

    match absolute.canonicalize() {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = absolute
                .parent()
                .context("excluded path has no parent directory")?
                .canonicalize()
                .with_context(|| format!("path not found: {}", path.display()))?;
            let name = absolute
                .file_name()
                .context("excluded path has no file name")?;
            Ok(parent.join(name))
        }
        Err(error) => Err(error)
            .with_context(|| format!("failed to resolve excluded path: {}", path.display())),
    }
}

/// Describe a missing file target inside a git repository.
///
/// This exists for diff impact: a file deleted by the selected diff has no
/// filesystem entry to walk, but it still has a stable repo-relative identity
/// and may have importers worth reporting.
///
/// # Errors
///
/// Returns an error when the missing path has no resolvable ancestor or cannot
/// be mapped to a containing Git worktree.
pub fn discover_missing_file(target: &Path) -> Result<Discovered> {
    let absolute = if target.is_absolute() {
        target.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(target)
    };
    let mut existing = absolute.as_path();
    let mut suffix = Vec::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .context("missing target has no existing ancestor")?;
        suffix.push(name.to_os_string());
        existing = existing
            .parent()
            .context("missing target has no existing ancestor")?;
    }
    let existing = existing
        .canonicalize()
        .with_context(|| format!("failed to resolve ancestor of {}", target.display()))?;
    let root = git_root(&existing).context("diff impact requires a git repository")?;
    let mut target = existing;
    for component in suffix.iter().rev() {
        target.push(component);
    }
    if !target.starts_with(&root) {
        return Err(anyhow::anyhow!(
            "diff impact target is outside the git repository: {}",
            target.display()
        ));
    }

    Ok(Discovered {
        root,
        target,
        files: Vec::new(),
        observed_files: 0,
        walker_errors: 0,
        oversized_files: 0,
        oversized_bytes: 0,
        files_omitted_by_limit: 0,
        files_omitted_count_incomplete: false,
        bytes_omitted_by_limit: 0,
        scan_truncated: false,
        duration_limit_reached: false,
    })
}

fn fallback_report_path(path: &Path) -> PathBuf {
    path.file_name()
        .map_or_else(|| PathBuf::from("scan-target"), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::{
        BoundedText, discover, discover_missing_file, discover_with_exclusions, read_text_bounded,
    };
    use crate::config::Config;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn repo_files_use_repository_relative_report_paths() {
        let dir = tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let file = dir.path().join("src/nested/lib.rs");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "pub fn example() {}\n").unwrap();

        let target = dir.path().join("src");
        let discovered = discover(&target, &Config::default()).unwrap();

        assert_eq!(discovered.root, dir.path().canonicalize().unwrap());
        assert_eq!(discovered.target, target.canonicalize().unwrap());
        assert_eq!(discovered.walker_errors, 0);
        assert_eq!(discovered.files.len(), 1);
        assert_eq!(
            discovered.files[0].absolute_path,
            file.canonicalize().unwrap()
        );
        assert_eq!(
            discovered.files[0].report_path,
            Path::new("src/nested/lib.rs")
        );
    }

    #[test]
    fn oversized_files_are_skipped_with_explicit_diagnostics() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("small.rs"), "fn small() {}\n").unwrap();
        fs::write(dir.path().join("large.rs"), "x".repeat(128)).unwrap();
        let cfg = Config {
            max_file_bytes: 32,
            ..Config::default()
        };

        let discovered = discover(dir.path(), &cfg).unwrap();

        assert_eq!(discovered.files.len(), 1);
        assert_eq!(discovered.oversized_files, 1);
        assert_eq!(discovered.oversized_bytes, 128);
        assert!(discovered.scan_truncated);
    }

    #[test]
    fn bounded_text_reads_never_allocate_past_the_file_limit() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("large.rs");
        fs::write(&file, "x".repeat(128)).unwrap();

        assert!(matches!(
            read_text_bounded(&file, 32),
            BoundedText::Oversized(128)
        ));
    }

    #[test]
    fn file_count_limit_stops_discovery() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();
        let cfg = Config {
            max_files: 1,
            ..Config::default()
        };

        let discovered = discover(dir.path(), &cfg).unwrap();

        assert_eq!(discovered.files.len(), 1);
        assert_eq!(discovered.observed_files, 2);
        assert_eq!(discovered.files_omitted_by_limit, 1);
        assert!(discovered.files_omitted_count_incomplete);
        assert!(discovered.scan_truncated);
    }

    #[test]
    fn aggregate_byte_limit_skips_excess_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "fn a() { let x = 1; }\n").unwrap();
        fs::write(dir.path().join("b.rs"), "fn b() { let x = 2; }\n").unwrap();
        let cfg = Config {
            max_total_bytes: 30,
            ..Config::default()
        };

        let discovered = discover(dir.path(), &cfg).unwrap();

        assert_eq!(discovered.files.len(), 1);
        assert_eq!(discovered.files_omitted_by_limit, 1);
        assert!(!discovered.files_omitted_count_incomplete);
        assert!(discovered.bytes_omitted_by_limit > 0);
        assert!(discovered.scan_truncated);
    }

    #[test]
    fn repository_root_excludes_only_the_matching_path() {
        let dir = tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let source = dir.path().join("src/lib.rs");
        let output = dir.path().join("report.json");
        let lookalike = dir.path().join("report.json.bak");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "pub fn example() {}\n").unwrap();
        fs::write(&output, "{}\n").unwrap();
        fs::write(&lookalike, "{}\n").unwrap();

        let aliased_output = dir.path().join("src/../report.json");
        let discovered =
            discover_with_exclusions(dir.path(), &Config::default(), &[aliased_output]).unwrap();
        let paths = discovered
            .files
            .iter()
            .map(|file| file.report_path.as_path())
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            [Path::new("report.json.bak"), Path::new("src/lib.rs")]
        );
    }

    #[test]
    fn standalone_directory_files_use_target_relative_report_paths() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("nested/file.rs");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "fn example() {}\n").unwrap();

        let discovered = discover(dir.path(), &Config::default()).unwrap();

        assert_eq!(discovered.root, dir.path().canonicalize().unwrap());
        assert_eq!(discovered.target, dir.path().canonicalize().unwrap());
        assert_eq!(discovered.walker_errors, 0);
        assert_eq!(discovered.files.len(), 1);
        assert_eq!(
            discovered.files[0].absolute_path,
            file.canonicalize().unwrap()
        );
        assert_eq!(discovered.files[0].report_path, Path::new("nested/file.rs"));
    }

    #[test]
    fn excludes_an_exact_nonexistent_path_when_it_is_created_later() {
        let dir = tempdir().unwrap();
        let source = dir.path().join("source.rs");
        let output = dir.path().join("report.json");
        fs::write(&source, "fn example() {}\n").unwrap();

        let exclusions = vec![output.clone()];
        let first = discover_with_exclusions(dir.path(), &Config::default(), &exclusions).unwrap();
        fs::write(&output, "{}\n").unwrap();
        let second = discover_with_exclusions(dir.path(), &Config::default(), &exclusions).unwrap();

        assert_eq!(first.files.len(), 1);
        assert_eq!(first.files[0].absolute_path, source.canonicalize().unwrap());
        assert_eq!(second.files.len(), 1);
        assert_eq!(
            second.files[0].absolute_path,
            source.canonicalize().unwrap()
        );
    }

    #[test]
    fn relative_exclusion_is_resolved_from_the_current_directory() {
        let current_dir = std::env::current_dir().unwrap();
        let dir = tempfile::Builder::new()
            .prefix("reposcout-walk-test-")
            .tempdir_in(&current_dir)
            .unwrap();
        let source = dir.path().join("source.rs");
        let output = dir.path().join("report.json");
        fs::write(&source, "fn example() {}\n").unwrap();
        fs::write(&output, "{}\n").unwrap();
        let relative_output = output.strip_prefix(&current_dir).unwrap().to_path_buf();

        let discovered = discover_with_exclusions(
            dir.path(),
            &Config::default(),
            std::slice::from_ref(&relative_output),
        )
        .unwrap();

        assert_eq!(discovered.files.len(), 1);
        assert_eq!(
            discovered.files[0].absolute_path,
            source.canonicalize().unwrap()
        );
    }

    #[test]
    fn repository_subpath_honors_an_exact_exclusion() {
        let dir = tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let source = dir.path().join("src/lib.rs");
        let output = dir.path().join("src/report.json");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "pub fn example() {}\n").unwrap();
        fs::write(&output, "{}\n").unwrap();

        let discovered = discover_with_exclusions(
            source.parent().unwrap(),
            &Config::default(),
            std::slice::from_ref(&output),
        )
        .unwrap();

        assert_eq!(discovered.files.len(), 1);
        assert_eq!(discovered.files[0].report_path, Path::new("src/lib.rs"));
    }

    #[test]
    fn standalone_file_uses_its_basename_as_report_path() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("only.rs");
        fs::write(&file, "fn example() {}\n").unwrap();

        let discovered = discover(&file, &Config::default()).unwrap();

        assert_eq!(discovered.root, file.canonicalize().unwrap());
        assert_eq!(discovered.target, file.canonicalize().unwrap());
        assert_eq!(discovered.walker_errors, 0);
        assert_eq!(discovered.files.len(), 1);
        assert_eq!(
            discovered.files[0].absolute_path,
            file.canonicalize().unwrap()
        );
        assert_eq!(discovered.files[0].report_path, Path::new("only.rs"));
    }

    #[test]
    fn standalone_file_target_can_be_excluded() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("only.rs");
        fs::write(&file, "fn example() {}\n").unwrap();

        let discovered =
            discover_with_exclusions(&file, &Config::default(), std::slice::from_ref(&file))
                .unwrap();

        assert!(discovered.files.is_empty());
    }

    #[test]
    fn missing_file_in_repo_has_a_stable_target_identity() {
        let dir = tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let missing = dir.path().join("src/deleted.js");
        fs::create_dir_all(missing.parent().unwrap()).unwrap();

        let discovered = discover_missing_file(&missing).unwrap();

        assert_eq!(discovered.root, dir.path().canonicalize().unwrap());
        assert_eq!(
            discovered.target,
            missing
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .join("deleted.js")
        );
        assert!(discovered.files.is_empty());
    }

    #[test]
    fn missing_file_with_deleted_parent_has_a_stable_target_identity() {
        let dir = tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let missing = dir.path().join("removed/nested/deleted.js");

        let discovered = discover_missing_file(&missing).unwrap();

        assert_eq!(discovered.root, dir.path().canonicalize().unwrap());
        assert_eq!(
            discovered.target,
            dir.path()
                .canonicalize()
                .unwrap()
                .join("removed/nested/deleted.js")
        );
    }
}
