//! Bounded hierarchical ignore policy shared by discovery and snapshot queries.

use super::{LOCKFILES, override_ignored};
use crate::config::Config;
use crate::fs_budget::{self, IgnoreLimits, ReadBudget, ReadOutcome, SourceRoot};
use anyhow::Result;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::overrides::{Override, OverrideBuilder};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

struct DirectoryRules {
    custom: Gitignore,
    plain: Gitignore,
    git: Gitignore,
    exclude: Gitignore,
    git_boundary: bool,
    failed: bool,
}

pub(crate) struct PathMatcher {
    root: PathBuf,
    source: Option<SourceRoot>,
    overrides: Override,
    include_hidden: bool,
    repository_ignores: bool,
    git_ignores: bool,
    limits: IgnoreLimits,
    directories: HashMap<PathBuf, DirectoryRules>,
    global_path: Option<PathBuf>,
    global: Option<Result<Gitignore, ()>>,
    rejected: HashSet<PathBuf>,
}

pub(crate) struct PathMatch(bool);

impl PathMatch {
    pub(crate) fn is_ignore(&self) -> bool {
        self.0
    }
}

impl PathMatcher {
    pub(crate) fn new(target: &Path, cfg: &Config) -> Result<Self> {
        let root = if target.is_file() {
            target.parent().unwrap_or(target)
        } else {
            target
        }
        .to_path_buf();
        let mut overrides = OverrideBuilder::new(target);
        if cfg.exclude_lockfiles {
            for name in LOCKFILES {
                overrides.add(&format!("!{name}"))?;
            }
        }
        for pattern in &cfg.extra_excludes {
            overrides.add(&format!("!{pattern}"))?;
        }
        Ok(Self {
            source: SourceRoot::open(&root).ok(),
            root,
            overrides: overrides.build()?,
            include_hidden: cfg.include_hidden,
            repository_ignores: cfg.load_repository_ignores,
            git_ignores: cfg.load_repository_ignores && cfg.respect_gitignore,
            limits: IgnoreLimits {
                max_file_bytes: cfg.max_ignore_file_bytes,
                max_lines: cfg.max_ignore_lines,
                max_line_bytes: cfg.max_ignore_line_bytes,
            },
            directories: HashMap::new(),
            global_path: (cfg.load_repository_ignores && cfg.respect_gitignore)
                .then(ignore::gitignore::gitconfig_excludes_path)
                .flatten(),
            global: None,
            rejected: HashSet::new(),
        })
    }

    pub(crate) fn rejected_files(&self) -> usize {
        self.rejected.len()
    }

    pub(crate) fn matched(&mut self, path: &Path, is_dir: bool) -> PathMatch {
        self.matched_with_errors(path, is_dir).0
    }

    pub(crate) fn matched_with_errors(
        &mut self,
        path: &Path,
        is_dir: bool,
    ) -> (PathMatch, Option<()>) {
        match self.check(path, is_dir) {
            Ok(ignored) => (PathMatch(ignored), None),
            Err(()) => (PathMatch(true), Some(())),
        }
    }

    fn check(&mut self, path: &Path, is_dir: bool) -> Result<bool, ()> {
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.root).map_err(|_| ())?
        } else {
            path
        };
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(());
        }
        let mut candidate = self.root.clone();
        let mut parts = relative.components().peekable();
        while let Some(part) = parts.next() {
            candidate.push(part);
            if self.check_one(&candidate, is_dir || parts.peek().is_some())? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn check_one(&mut self, path: &Path, is_dir: bool) -> Result<bool, ()> {
        if self.overrides.matched(path, is_dir).is_ignore()
            || override_ignored(&self.overrides, path, &self.root)
        {
            return Ok(true);
        }
        if self.overrides.matched(path, is_dir).is_whitelist() {
            return Ok(false);
        }
        let mut ancestors = Vec::new();
        if self.repository_ignores {
            for directory in path.parent().into_iter().flat_map(Path::ancestors) {
                if !self.git_ignores && !directory.starts_with(&self.root) {
                    break;
                }
                self.load_directory(directory);
                ancestors.push(directory);
            }
        }
        let has_git_boundary = ancestors.iter().any(|directory| {
            self.directories
                .get(*directory)
                .is_some_and(|rules| rules.git_boundary)
        });
        if has_git_boundary && self.global.is_none() {
            self.global = Some(if let Some(path) = self.global_path.clone() {
                let base = self.root.clone();
                self.load(&path, &base)
            } else {
                Ok(Gitignore::empty())
            });
        }
        let rules = ancestors
            .iter()
            .filter_map(|directory| self.directories.get(*directory))
            .collect::<Vec<_>>();
        if (has_git_boundary && self.global.as_ref().is_some_and(Result::is_err))
            || rules.iter().any(|rules| rules.failed)
        {
            return Err(());
        }
        // File type precedence is independent of directory depth: custom files
        // override .ignore, which overrides Git rules. Within a type, nearest wins.
        for custom in [true, false] {
            if let Some(ignored) = rules.iter().find_map(|rules| {
                decision(
                    if custom { &rules.custom } else { &rules.plain },
                    path,
                    is_dir,
                )
            }) {
                return Ok(ignored);
            }
        }
        if let Some(boundary) = rules.iter().position(|rules| rules.git_boundary) {
            for rules in &rules[..=boundary] {
                if let Some(ignored) = decision(&rules.git, path, is_dir) {
                    return Ok(ignored);
                }
            }
            if let Some(ignored) = decision(&rules[boundary].exclude, path, is_dir).or_else(|| {
                self.global
                    .as_ref()
                    .and_then(|global| global.as_ref().ok())
                    .and_then(|global| decision(global, path, is_dir))
            }) {
                return Ok(ignored);
            }
        }
        Ok(!self.include_hidden
            && path
                .file_name()
                .is_some_and(|name| name.as_encoded_bytes().starts_with(b".")))
    }

    fn load_directory(&mut self, directory: &Path) {
        if self.directories.contains_key(directory) {
            return;
        }
        let mut rules = DirectoryRules {
            custom: Gitignore::empty(),
            plain: Gitignore::empty(),
            git: Gitignore::empty(),
            exclude: Gitignore::empty(),
            git_boundary: false,
            failed: false,
        };
        let git_ignores = self.git_ignores;
        let mut load = |name: &str| {
            if let Ok(matcher) = self.load(&directory.join(name), directory) {
                matcher
            } else {
                rules.failed = true;
                Gitignore::empty()
            }
        };
        rules.custom = load(".reposcoutignore");
        if git_ignores {
            rules.plain = load(".ignore");
            if directory
                .ancestors()
                .any(|parent| parent.join(".git").exists() || parent.join(".jj").exists())
            {
                rules.git = load(".gitignore");
            }
            rules.git_boundary = directory.join(".git").exists() || directory.join(".jj").exists();
            if rules.git_boundary
                && let Ok(repo) = git2::Repository::open(directory)
            {
                match self.load(&repo.commondir().join("info/exclude"), directory) {
                    Ok(matcher) => rules.exclude = matcher,
                    Err(()) => rules.failed = true,
                }
            }
        }
        self.directories.insert(directory.to_path_buf(), rules);
    }

    fn load(&mut self, path: &Path, base: &Path) -> Result<Gitignore, ()> {
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Gitignore::empty());
            }
            _ => {}
        }
        let outcome = if path.starts_with(&self.root) {
            let mut budget =
                ReadBudget::from_limits(self.limits.max_file_bytes, self.limits.max_file_bytes, 1);
            self.source
                .as_ref()
                .map_or(ReadOutcome::Unreadable, |root| {
                    root.read_absolute(path, &mut budget)
                })
        } else {
            fs_budget::read_text_limited(path, self.limits.max_file_bytes)
        };
        let compiled = fs_budget::validate_ignore_content(outcome, self.limits)
            .map_err(|error| format!("{error:?}"))
            .and_then(|content| {
                let mut builder = GitignoreBuilder::new(base);
                for (index, line) in content.lines().enumerate() {
                    // Match Git and ignore::GitignoreBuilder::add for UTF-8 BOMs.
                    let line = if index == 0 {
                        line.trim_start_matches('\u{feff}')
                    } else {
                        line
                    };
                    builder
                        .add_line(Some(path.to_path_buf()), line)
                        .map_err(|error| error.to_string())?;
                }
                builder.build().map_err(|error| error.to_string())
            });
        compiled.map_err(|reason| {
            self.rejected.insert(path.to_path_buf());
            crate::debug_log::event(
                "ignore_file_error",
                || serde_json::json!({"path": path, "reason": reason}),
            );
        })
    }
}

fn decision(matcher: &Gitignore, path: &Path, is_dir: bool) -> Option<bool> {
    match matcher.matched(path, is_dir) {
        ignore::Match::Ignore(_) => Some(true),
        ignore::Match::Whitelist(_) => Some(false),
        ignore::Match::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn precedence_parent_exclusion_and_hidden_whitelists_match_discovery() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git2::Repository::init(&root).unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::create_dir(root.join("ignored")).unwrap();
        fs::write(root.join(".gitignore"), "\u{feff}*.rs\n").unwrap();
        fs::write(root.join(".ignore"), "!keep.rs\n!.visible.rs\n").unwrap();
        fs::write(root.join(".reposcoutignore"), "ignored/\n").unwrap();
        fs::write(root.join("nested/.gitignore"), "!allowed.rs\n").unwrap();
        fs::write(root.join("ignored/.reposcoutignore"), "!keep.rs\n").unwrap();
        let paths = [
            "keep.rs",
            "drop.rs",
            ".visible.rs",
            ".hidden.rs",
            "nested/allowed.rs",
            "nested/drop.rs",
            "ignored/keep.rs",
        ];
        for path in paths {
            fs::write(root.join(path), "fn example() {}\n").unwrap();
        }
        let cfg = Config::default();
        let mut matcher = PathMatcher::new(&root, &cfg).unwrap();
        let mut legacy = ignore::WalkBuilder::new(&root);
        legacy.add_custom_ignore_filename(".reposcoutignore");
        let expected = legacy
            .build()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
            .map(|entry| entry.into_path())
            .collect::<HashSet<_>>();
        for path in paths {
            assert_eq!(
                !matcher.matched(Path::new(path), false).is_ignore(),
                expected.contains(&root.join(path)),
                "{path}"
            );
        }
        let discovered = super::super::discover(&root, &cfg).unwrap();
        assert_eq!(
            discovered
                .files
                .iter()
                .map(|file| file.absolute_path.clone())
                .collect::<HashSet<_>>(),
            expected
        );
    }

    #[test]
    fn no_ignore_keeps_custom_rules_and_safe_skips_all_ignore_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        git2::Repository::init(&root).unwrap();
        fs::write(root.join(".gitignore"), "git.rs\n").unwrap();
        fs::write(root.join(".reposcoutignore"), "custom.rs\n").unwrap();
        let mut cfg = Config {
            respect_gitignore: false,
            ..Config::default()
        };
        let mut matcher = PathMatcher::new(&root, &cfg).unwrap();
        assert!(!matcher.matched(Path::new("git.rs"), false).is_ignore());
        assert!(matcher.matched(Path::new("custom.rs"), false).is_ignore());
        cfg.load_repository_ignores = false;
        cfg.max_ignore_file_bytes = 1;
        let mut matcher = PathMatcher::new(&root, &cfg).unwrap();
        assert!(!matcher.matched(Path::new("custom.rs"), false).is_ignore());
        assert_eq!(matcher.rejected_files(), 0);
    }
}
