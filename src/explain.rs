//! Full-repository context projected onto one requested file.

use crate::config::Config;
use crate::metrics::{risk, testcov};
use crate::model::{
    DiscoveryExplanation, ExclusionRule, ExplainReport, ExplainRepository, FileReport,
    SCHEMA_VERSION, TestExplanation,
};
use crate::{graph, lang, scan, walk};
use anyhow::{Context, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::overrides::{Override, OverrideBuilder};
use std::path::{Component, Path, PathBuf};

/// Build a repository-aware explanation for one requested file.
///
/// # Errors
///
/// Returns an error when the requested path cannot be resolved, repository
/// discovery fails, configuration-owned ignore rules are invalid, or the
/// surrounding repository scan cannot complete.
pub fn run(file: &Path, cfg: &Config, exclusions: &[PathBuf]) -> Result<ExplainReport> {
    let requested = resolve_path(file)?;
    let repository_path = rebase_onto_repository(&requested)?;
    let (absolute, known_root) = match repository_path {
        Some((path, root)) => (path, Some(root)),
        None => (canonicalize_existing_parent(&requested)?, None),
    };
    let symlink = match known_root.as_deref() {
        Some(root) => first_symlink_component_from(&absolute, root)?,
        None => first_symlink_component(&absolute)?,
    };
    let root = if let Some(root) = known_root {
        root
    } else {
        let anchor = match symlink.as_deref() {
            Some(path) => path
                .parent()
                .context("symbolic link path has no parent")?
                .canonicalize()
                .with_context(|| format!("failed to resolve parent of {}", file.display()))?,
            None => existing_anchor(&absolute)?,
        };
        walk::git_root(&anchor)
            .unwrap_or(anchor)
            .canonicalize()
            .with_context(|| format!("failed to resolve scan root for {}", file.display()))?
    };
    let path = absolute
        .strip_prefix(&root)
        .ok()
        .filter(|path| !path.as_os_str().is_empty())
        .map_or_else(
            || {
                absolute
                    .file_name()
                    .map_or_else(|| PathBuf::from("scan-target"), PathBuf::from)
            },
            Path::to_path_buf,
        );
    let discovery =
        discovery_explanation(&absolute, symlink.as_deref(), &root, &path, cfg, exclusions)?;
    let artifacts = scan::run_with_artifacts(
        &root,
        cfg,
        exclusions,
        scan::ArtifactRequirements {
            symbol_outlines: false,
            graph_facts: true,
        },
    )?;
    let report = artifacts.report;
    let file_report = report
        .files
        .iter()
        .find(|candidate| candidate.path == path)
        .cloned();
    let testing = testing_context(&path, file_report.as_ref(), &report.files);
    let risk = file_report.as_ref().and_then(|file| {
        (testing.classification == "source").then(|| risk::explain(file, !testing.tested))
    });
    let graph = graph::explain_with_facts(
        &report.files,
        &root,
        &path,
        &artifacts.graph_facts,
        Some(&artifacts.resolver_configs),
        graph::GraphReadLimits::from_config(cfg),
    );
    let findings = report
        .finding_catalog
        .findings
        .iter()
        .filter(|finding| {
            finding.primary_location.path == path
                || finding
                    .related_locations
                    .iter()
                    .any(|location| location.path == path)
        })
        .cloned()
        .collect();

    let repository = explain_repository(&report.summary);
    Ok(ExplainReport {
        schema_version: SCHEMA_VERSION.to_string(),
        root,
        path,
        generated_at: chrono::Utc::now().to_rfc3339(),
        encoding: report.encoding,
        execution: report.execution,
        discovery,
        repository,
        file: file_report,
        risk,
        testing,
        graph,
        findings,
    })
}

fn explain_repository(summary: &crate::model::Summary) -> ExplainRepository {
    let (source_files, test_files) = summary
        .test_presence
        .as_ref()
        .map_or((0, 0), |tests| (tests.source_files, tests.test_files));
    ExplainRepository {
        files: summary.files,
        tokens: summary.tokens,
        source_files,
        test_files,
    }
}

fn discovery_explanation(
    absolute: &Path,
    symlink: Option<&Path>,
    root: &Path,
    path: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
) -> Result<DiscoveryExplanation> {
    if let Some(symlink) = symlink {
        let pattern = symlink
            .strip_prefix(root)
            .ok()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(path)
            .to_string_lossy();
        return Ok(discovery(
            "ignored",
            "path contains a symbolic link; repository discovery does not follow links",
            Some(rule("symlink", "built-in", &pattern)),
        ));
    }
    if !absolute.exists() {
        return Ok(discovery("missing", "file does not exist", None));
    }
    if absolute.is_dir() {
        return Ok(discovery(
            "directory",
            "target is a directory, not a file",
            None,
        ));
    }

    let walked = walk::discover_with_exclusions(root, cfg, exclusions)?;
    let selected = walked.files.iter().any(|file| file.report_path == path);
    if !selected {
        if lang::detect(path).is_some()
            && absolute
                .metadata()
                .is_ok_and(|metadata| metadata.len() > cfg.max_file_bytes)
        {
            return Ok(discovery(
                "oversized",
                "file exceeds the configured per-file analysis limit",
                None,
            ));
        }
        let rule = exclusion_rule(absolute, root, path, cfg, exclusions)?;
        let reason = rule.as_ref().map_or_else(
            || "excluded by repository discovery policy".to_string(),
            |rule| format!("excluded by {} pattern `{}`", rule.kind, rule.pattern),
        );
        return Ok(discovery("ignored", &reason, rule));
    }
    if lang::detect(path).is_none() {
        return Ok(discovery(
            "unsupported",
            "file type is not recognized by reposcout",
            None,
        ));
    }
    match walk::read_text_bounded(absolute, cfg.max_file_bytes) {
        walk::BoundedText::Content(_) => {}
        walk::BoundedText::Oversized(_) => {
            return Ok(discovery(
                "oversized",
                "file exceeds the configured per-file analysis limit",
                None,
            ));
        }
        walk::BoundedText::Unreadable => {
            return Ok(discovery(
                "unreadable",
                "file is not readable UTF-8 text",
                None,
            ));
        }
    }
    Ok(discovery(
        "analyzed",
        "file is included in the repository scan",
        None,
    ))
}

fn discovery(status: &str, reason: &str, rule: Option<ExclusionRule>) -> DiscoveryExplanation {
    DiscoveryExplanation {
        status: status.to_string(),
        reason: reason.to_string(),
        rule,
    }
}

fn exclusion_rule(
    absolute: &Path,
    root: &Path,
    path: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
) -> Result<Option<ExclusionRule>> {
    for excluded in exclusions {
        if walk::exact_path_identity(excluded)? == absolute {
            return Ok(Some(rule("exclude", "command line output", "exact path")));
        }
    }
    if !cfg.include_hidden
        && let Some(component) = hidden_component(path)
    {
        return Ok(Some(rule("hidden", "built-in", component)));
    }
    if cfg.exclude_lockfiles && walk::is_lockfile(path) {
        let pattern = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("lockfile");
        return Ok(Some(rule("lockfile", "built-in", pattern)));
    }
    for pattern in &cfg.extra_excludes {
        let mut builder = OverrideBuilder::new(root);
        builder.add(&format!("!{pattern}"))?;
        if override_ignored(&builder.build()?, absolute, root) {
            return Ok(Some(rule("exclude", "configuration", pattern)));
        }
    }

    if cfg.load_repository_ignores {
        if let Some(rule) = local_ignore_rule(absolute, root, cfg.respect_gitignore, cfg) {
            return Ok(Some(rule));
        }
        if cfg.respect_gitignore {
            if let Some(rule) = info_exclude_rule(absolute, root, cfg)? {
                return Ok(Some(rule));
            }
            let (global, _) = Gitignore::global();
            if let ignore::Match::Ignore(glob) = global.matched(absolute, false) {
                return Ok(Some(rule(
                    "gitignore",
                    &glob.from().map_or_else(
                        || "global gitignore".to_string(),
                        |path| path.display().to_string(),
                    ),
                    glob.original(),
                )));
            }
        }
    }
    Ok(None)
}

fn override_ignored(overrides: &Override, absolute: &Path, root: &Path) -> bool {
    if overrides.matched(absolute, false).is_ignore() {
        return true;
    }
    let mut ancestor = absolute.parent();
    while let Some(path) = ancestor.filter(|path| path.starts_with(root)) {
        if overrides.matched(path, true).is_ignore() {
            return true;
        }
        if path == root {
            break;
        }
        ancestor = path.parent();
    }
    false
}

fn local_ignore_rule(
    absolute: &Path,
    root: &Path,
    respect_gitignore: bool,
    cfg: &Config,
) -> Option<ExclusionRule> {
    let parent = absolute.parent()?;
    let relative_parent = parent.strip_prefix(root).ok()?;
    let mut directory = root.to_path_buf();
    let mut matched_rule = None;
    let mut directories = vec![directory.clone()];
    for component in relative_parent.components() {
        if let Component::Normal(component) = component {
            directory.push(component);
            directories.push(directory.clone());
        }
    }

    let limits = crate::fs_budget::IgnoreLimits {
        max_file_bytes: cfg.max_ignore_file_bytes,
        max_lines: cfg.max_ignore_lines,
        max_line_bytes: cfg.max_ignore_line_bytes,
    };
    for directory in directories {
        let names: &[&str] = if respect_gitignore {
            &[".gitignore", ".ignore", ".reposcoutignore"]
        } else {
            &[".reposcoutignore"]
        };
        for name in names {
            let source = directory.join(name);
            if !crate::fs_budget::is_regular_file(&source) {
                continue;
            }
            let Ok(content) = crate::fs_budget::read_ignore_file(&source, limits) else {
                continue;
            };
            let mut builder = GitignoreBuilder::new(&directory);
            for line in content.lines() {
                let _ = builder.add_line(Some(source.clone()), line);
            }
            let Ok(ignore_matcher) = builder.build() else {
                continue;
            };
            match ignore_matcher.matched_path_or_any_parents(absolute, false) {
                ignore::Match::Ignore(glob) => {
                    matched_rule = Some(rule(
                        if *name == ".reposcoutignore" {
                            "reposcoutignore"
                        } else {
                            "gitignore"
                        },
                        &source.display().to_string(),
                        glob.original(),
                    ));
                }
                ignore::Match::Whitelist(_) => matched_rule = None,
                ignore::Match::None => {}
            }
        }
    }
    matched_rule
}

fn info_exclude_rule(absolute: &Path, root: &Path, cfg: &Config) -> Result<Option<ExclusionRule>> {
    let source = root.join(".git/info/exclude");
    let limits = crate::fs_budget::IgnoreLimits {
        max_file_bytes: cfg.max_ignore_file_bytes,
        max_lines: cfg.max_ignore_lines,
        max_line_bytes: cfg.max_ignore_line_bytes,
    };
    let Ok(content) = crate::fs_budget::read_ignore_file(&source, limits) else {
        return Ok(None);
    };
    let mut builder = GitignoreBuilder::new(root);
    for line in content.lines() {
        builder.add_line(Some(source.clone()), line)?;
    }
    if let ignore::Match::Ignore(glob) = builder.build()?.matched(absolute, false) {
        return Ok(Some(rule(
            "gitignore",
            &source.display().to_string(),
            glob.original(),
        )));
    }
    Ok(None)
}

fn rule(kind: &str, source: &str, pattern: &str) -> ExclusionRule {
    ExclusionRule {
        kind: kind.to_string(),
        source: source.to_string(),
        pattern: pattern.to_string(),
    }
}

fn hidden_component(path: &Path) -> Option<&str> {
    path.components().find_map(|component| {
        let Component::Normal(component) = component else {
            return None;
        };
        let component = component.to_str()?;
        (component.starts_with('.') && component != ".").then_some(component)
    })
}

fn testing_context(
    path: &Path,
    file: Option<&FileReport>,
    files: &[FileReport],
) -> TestExplanation {
    let Some(file) = file else {
        return TestExplanation {
            classification: "unavailable".to_string(),
            ..TestExplanation::default()
        };
    };
    if !lang::detect(path).is_some_and(lang::LangInfo::is_code) {
        return TestExplanation {
            classification: "non-code".to_string(),
            ..TestExplanation::default()
        };
    }
    let path_string = path.to_string_lossy();
    if testcov::is_test_file(path_string.as_ref()) {
        let keys = testcov::test_stem_keys(path_string.as_ref());
        let mut matches = files
            .iter()
            .filter(|candidate| {
                let candidate = candidate.path.to_string_lossy();
                !testcov::is_test_file(candidate.as_ref())
                    && keys.contains(&testcov::source_stem(candidate.as_ref()))
            })
            .map(|candidate| candidate.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        matches.sort();
        return TestExplanation {
            classification: "test".to_string(),
            tested: true,
            has_inline_tests: file.has_inline_tests,
            logical_key: keys.first().cloned().unwrap_or_default(),
            matches,
        };
    }

    let logical_key = testcov::source_stem(path_string.as_ref());
    let mut matches = files
        .iter()
        .filter(|candidate| {
            let candidate = candidate.path.to_string_lossy();
            testcov::is_test_file(candidate.as_ref())
                && testcov::test_stem_keys(candidate.as_ref()).contains(&logical_key)
        })
        .map(|candidate| candidate.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    matches.sort();
    TestExplanation {
        classification: "source".to_string(),
        tested: file.has_inline_tests || !matches.is_empty(),
        has_inline_tests: file.has_inline_tests,
        logical_key,
        matches,
    }
}

fn resolve_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                resolved.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
        }
    }
    Ok(resolved)
}

/// Rebase a lexical input path onto its canonical Git root. This removes
/// harmless system aliases such as macOS `/var` -> `/private/var` without
/// resolving symlinks that live inside the repository.
fn rebase_onto_repository(path: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
    for candidate in path.ancestors() {
        let Some(root) = walk::git_root(candidate) else {
            continue;
        };
        let root = root
            .canonicalize()
            .with_context(|| format!("failed to resolve repository root {}", root.display()))?;
        let lexical_root = path.ancestors().find(|ancestor| {
            ancestor
                .canonicalize()
                .is_ok_and(|canonical| canonical == root)
        });
        let Some(lexical_root) = lexical_root else {
            continue;
        };
        let relative = path
            .strip_prefix(lexical_root)
            .context("failed to make explained path repository-relative")?;
        return Ok(Some((root.join(relative), root)));
    }
    Ok(None)
}

/// Canonicalize only the deepest existing parent, retaining the requested
/// final path components so a final symlink can still be identified.
fn canonicalize_existing_parent(path: &Path) -> Result<PathBuf> {
    let Some(parent) = path.parent() else {
        return path
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", path.display()));
    };
    let mut anchor = parent;
    let mut suffix = path
        .file_name()
        .map(|name| vec![name.to_os_string()])
        .unwrap_or_default();
    while !anchor.exists() {
        let name = anchor
            .file_name()
            .context("path has no existing ancestor")?;
        suffix.push(name.to_os_string());
        anchor = anchor.parent().context("path has no existing ancestor")?;
    }
    let mut normalized = anchor
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", anchor.display()))?;
    for component in suffix.iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

fn first_symlink_component_from(path: &Path, root: &Path) -> Result<Option<PathBuf>> {
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("{} is outside repository root", path.display()))?;
    let mut candidate = root.to_path_buf();
    for component in relative.components() {
        candidate.push(component.as_os_str());
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Ok(Some(candidate)),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect {}", candidate.display()));
            }
        }
    }
    Ok(None)
}

fn first_symlink_component(path: &Path) -> Result<Option<PathBuf>> {
    let mut candidate = PathBuf::new();
    for component in path.components() {
        candidate.push(component.as_os_str());
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Ok(Some(candidate)),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to inspect {}", candidate.display()));
            }
        }
    }
    Ok(None)
}

fn existing_anchor(path: &Path) -> Result<PathBuf> {
    let mut anchor = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    while !anchor.exists() {
        anchor = anchor.parent().context("path has no existing ancestor")?;
    }
    anchor
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", anchor.display()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::{first_symlink_component_from, rebase_onto_repository};
    use git2::Repository;

    #[test]
    fn filesystem_alias_above_repo_rebases_without_becoming_an_internal_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let physical_parent = dir.path().join("private-var");
        let repo_root = physical_parent.join("repo");
        std::fs::create_dir_all(repo_root.join("src")).unwrap();
        Repository::init(&repo_root).unwrap();
        std::fs::write(repo_root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let alias = dir.path().join("var");
        std::os::unix::fs::symlink(&physical_parent, &alias).unwrap();
        let requested = alias.join("repo/src/main.rs");

        let (rebased, root) = rebase_onto_repository(&requested)
            .unwrap()
            .expect("repository should be discovered through the lexical alias");

        assert_eq!(root, repo_root.canonicalize().unwrap());
        assert_eq!(rebased, root.join("src/main.rs"));
        assert_eq!(first_symlink_component_from(&rebased, &root).unwrap(), None);
    }

    #[test]
    fn repository_internal_symlink_remains_visible_after_rebasing() {
        let dir = tempfile::tempdir().unwrap();
        let repo_root = dir.path().join("repo");
        let external = dir.path().join("external.rs");
        std::fs::create_dir(&repo_root).unwrap();
        Repository::init(&repo_root).unwrap();
        std::fs::write(&external, "fn external() {}\n").unwrap();
        let link = repo_root.join("outside.rs");
        std::os::unix::fs::symlink(&external, &link).unwrap();

        let (rebased, root) = rebase_onto_repository(&link)
            .unwrap()
            .expect("repository should be discovered");

        assert_eq!(rebased, root.join("outside.rs"));
        assert_eq!(
            first_symlink_component_from(&rebased, &root).unwrap(),
            Some(root.join("outside.rs"))
        );
    }
}
