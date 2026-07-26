//! File discovery. Uses the `ignore` crate for gitignore-aware traversal and
//! `git2` to locate the repository root.

use crate::config::Config;
use crate::debug_log;
use crate::lang;
use anyhow::{Context, Result};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
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

pub(crate) enum BoundedText {
    Content(String),
    Oversized(u64),
    Unreadable,
}

pub(crate) fn read_text_bounded(path: &Path, max_bytes: u64) -> BoundedText {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return BoundedText::Unreadable,
    };
    let metadata_bytes = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(_) => return BoundedText::Unreadable,
    };
    if metadata_bytes > max_bytes {
        return BoundedText::Oversized(metadata_bytes);
    }

    let capacity = usize::try_from(metadata_bytes.min(max_bytes)).unwrap_or(usize::MAX);
    let mut bytes = Vec::with_capacity(capacity);
    if file
        .by_ref()
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return BoundedText::Unreadable;
    }
    if bytes.len() as u64 > max_bytes {
        return BoundedText::Oversized((bytes.len() as u64).max(metadata_bytes));
    }
    match String::from_utf8(bytes) {
        Ok(content) => BoundedText::Content(content),
        Err(_) => BoundedText::Unreadable,
    }
}

/// Locate the git working-tree root containing `target`, if any.
pub fn git_root(target: &Path) -> Option<PathBuf> {
    let repo = git2::Repository::discover(target).ok()?;
    repo.workdir().map(|p| p.to_path_buf())
}

/// Walk `target`, honoring configuration (gitignore, hidden files, excludes).
pub fn discover(target: &Path, cfg: &Config) -> Result<Discovered> {
    discover_with_exclusions(target, cfg, &[])
}

/// Walk `target`, omitting files whose filesystem identities exactly match an exclusion.
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
    let target = target
        .canonicalize()
        .with_context(|| format!("path not found: {}", target.display()))?;
    let exclusions = exclusions
        .iter()
        .map(|path| exact_path_identity(path))
        .collect::<Result<HashSet<_>>>()?;
    let repo_root = git_root(&target);
    let root = repo_root.clone().unwrap_or_else(|| target.clone());
    let report_base = repo_root
        .as_deref()
        .or_else(|| target.is_dir().then_some(&target));

    let mut builder = WalkBuilder::new(&target);
    builder
        .hidden(!cfg.include_hidden)
        .git_ignore(cfg.respect_gitignore)
        .git_global(cfg.respect_gitignore)
        .git_exclude(cfg.respect_gitignore)
        .ignore(cfg.respect_gitignore)
        .parents(cfg.respect_gitignore)
        .follow_links(false);
    builder.add_custom_ignore_filename(".reposcoutignore");

    let mut exclude_globs: Vec<String> = Vec::new();
    if cfg.exclude_lockfiles {
        // A leading '!' turns an override into an ignore rule; a slash-free
        // pattern matches the file name at any depth (gitignore semantics).
        exclude_globs.extend(LOCKFILES.iter().map(|name| format!("!{name}")));
    }
    exclude_globs.extend(cfg.extra_excludes.iter().map(|pat| format!("!{pat}")));

    if !exclude_globs.is_empty() {
        let mut ob = OverrideBuilder::new(&target);
        for glob in &exclude_globs {
            ob.add(glob)
                .with_context(|| format!("invalid exclude glob: {glob}"))?;
        }
        builder.overrides(ob.build().context("building exclude overrides")?);
    }

    let mut files = Vec::new();
    let mut observed_files = 0usize;
    let mut accepted_bytes = 0u64;
    let mut walker_errors = 0usize;
    let mut oversized_files = 0usize;
    let mut oversized_bytes = 0u64;
    let mut files_omitted_by_limit = 0usize;
    let mut files_omitted_count_incomplete = false;
    let mut bytes_omitted_by_limit = 0u64;
    let mut scan_truncated = false;
    let mut duration_limit_reached = false;
    for result in builder.build() {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            duration_limit_reached = true;
            scan_truncated = true;
            files_omitted_count_incomplete = true;
            break;
        }
        let entry = match result {
            Ok(e) => e,
            Err(_) => {
                walker_errors += 1;
                continue;
            }
        };
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let absolute_path = entry.into_path();
            if exclusions.contains(&absolute_path) {
                continue;
            }
            observed_files = observed_files.saturating_add(1);
            if observed_files > cfg.max_files {
                files_omitted_by_limit = files_omitted_by_limit.saturating_add(1);
                files_omitted_count_incomplete = true;
                scan_truncated = true;
                break;
            }
            let report_path = match report_base {
                Some(base) => absolute_path
                    .strip_prefix(base)
                    .ok()
                    .filter(|path| !path.as_os_str().is_empty())
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| fallback_report_path(&absolute_path)),
                None => fallback_report_path(&absolute_path),
            };
            if lang::detect(&report_path).is_some() {
                match absolute_path.metadata() {
                    Ok(metadata) => {
                        let bytes = metadata.len();
                        if bytes > cfg.max_file_bytes {
                            oversized_files = oversized_files.saturating_add(1);
                            oversized_bytes = oversized_bytes.saturating_add(bytes);
                            scan_truncated = true;
                            continue;
                        }
                        if accepted_bytes.saturating_add(bytes) > cfg.max_total_bytes {
                            files_omitted_by_limit = files_omitted_by_limit.saturating_add(1);
                            bytes_omitted_by_limit = bytes_omitted_by_limit.saturating_add(bytes);
                            scan_truncated = true;
                            continue;
                        }
                        accepted_bytes = accepted_bytes.saturating_add(bytes);
                    }
                    Err(_) => {
                        walker_errors = walker_errors.saturating_add(1);
                        continue;
                    }
                }
            }
            files.push(DiscoveredFile {
                absolute_path,
                report_path,
            });
            if debug_log::enabled() && (files.len() == 1 || files.len().is_multiple_of(1_000)) {
                let latest = files
                    .last()
                    .expect("a discovered file was just appended")
                    .report_path
                    .to_string_lossy();
                debug_log::event("discovery_progress", || {
                    serde_json::json!({
                        "files": files.len(),
                        "latest_path": latest.as_ref(),
                        "walker_errors": walker_errors,
                    })
                });
            }
        }
    }
    files.sort_by(|a, b| {
        a.report_path
            .cmp(&b.report_path)
            .then_with(|| a.absolute_path.cmp(&b.absolute_path))
    });

    Ok(Discovered {
        root,
        target,
        files,
        observed_files,
        walker_errors,
        oversized_files,
        oversized_bytes,
        files_omitted_by_limit,
        files_omitted_count_incomplete,
        bytes_omitted_by_limit,
        scan_truncated,
        duration_limit_reached,
    })
}

/// Resolve an existing or future path to the exact identity used by discovery.
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
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("scan-target"))
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
