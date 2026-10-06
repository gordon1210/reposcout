use assert_cmd::Command;
use git2::{IndexAddOption, Oid, Repository, RepositoryInitOptions, Signature, Time};
use serde_json::Value;
use std::cell::Cell;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// One synthetic repository plus private configuration/cache storage outside its inventory.
pub(super) struct Fixture {
    directory: Option<TempDir>,
    root: PathBuf,
    home: PathBuf,
    cache: PathBuf,
    label: String,
    commits: Cell<i64>,
    started: Instant,
}

impl Fixture {
    pub(super) fn new(label: &str) -> Self {
        let directory = tempfile::Builder::new()
            .prefix("reposcout-scenario-")
            .tempdir()
            .unwrap();
        let root = directory.path().join("repository");
        let home = directory.path().join("state");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&home).unwrap();
        Repository::init_opts(&root, RepositoryInitOptions::new().initial_head("main")).unwrap();
        let cache = if cfg!(target_os = "macos") {
            home.join("Library/Caches/reposcout")
        } else {
            home.join("reposcout")
        };
        Self {
            directory: Some(directory),
            root,
            home,
            cache,
            label: label.to_owned(),
            commits: Cell::new(0),
            started: Instant::now(),
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.root
    }

    pub(super) fn cache_path(&self) -> &Path {
        &self.cache
    }

    pub(super) fn state_path(&self) -> &Path {
        &self.home
    }

    fn child(&self, path: &str) -> PathBuf {
        assert!(
            !path.is_empty()
                && Path::new(path)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "fixture path must stay below its repository: {path:?}"
        );
        self.root.join(path)
    }

    pub(super) fn write(&self, path: &str, source: &str) {
        let target = self.child(path);
        for ancestor in target.ancestors().take_while(|p| *p != self.root) {
            if let Ok(metadata) = fs::symlink_metadata(ancestor) {
                assert!(
                    !metadata.file_type().is_symlink(),
                    "fixture write follows {}",
                    ancestor.display()
                );
            }
        }
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, source).unwrap();
    }

    pub(super) fn remove(&self, path: &str) {
        let target = self.child(path);
        for ancestor in target
            .parent()
            .unwrap()
            .ancestors()
            .take_while(|p| *p != self.root)
        {
            assert!(
                !fs::symlink_metadata(ancestor)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        if fs::symlink_metadata(&target).unwrap().is_dir() {
            fs::remove_dir_all(target).unwrap();
        } else {
            fs::remove_file(target).unwrap();
        }
    }

    pub(super) fn stage_all(&self) {
        let repository = Repository::open(&self.root).unwrap();
        let mut index = repository.index().unwrap();
        index.add_all(["*"], IndexAddOption::DEFAULT, None).unwrap();
        index.update_all(["*"], None).unwrap();
        index.write().unwrap();
    }

    pub(super) fn commit(&self, message: &str) -> String {
        self.stage_all();
        let repository = Repository::open(&self.root).unwrap();
        let tree_id = repository.index().unwrap().write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let time = Time::new(1_700_000_000 + self.commits.get(), 0);
        self.commits.set(self.commits.get() + 1);
        let signature = Signature::new("Scenario", "scenario@example.invalid", &time).unwrap();
        let parent = repository
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok());
        let parents: Vec<_> = parent.iter().collect();
        repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &parents,
            )
            .unwrap()
            .to_string()
    }

    /// Only resets this fixture's disposable Git history and files.
    pub(super) fn checkout(&self, commit: &str) {
        let repository = Repository::open(&self.root).unwrap();
        let commit = repository
            .find_commit(Oid::from_str(commit).unwrap())
            .unwrap();
        repository.set_head_detached(commit.id()).unwrap();
        repository
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
    }

    pub(super) fn command(&self, arguments: &[&str]) -> Command {
        eprintln!("[{}] reposcout {arguments:?}", self.label);
        let mut command = super::test_command::reposcout_command();
        // Child-only overrides isolate caches on Linux and macOS without mutating the test process.
        command
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("XDG_CACHE_HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .timeout(Duration::from_secs(30))
            .args(arguments);
        command
    }

    pub(super) fn json(&self, arguments: &[&str]) -> Value {
        let output = self
            .command(arguments)
            .args(["-f", "json", "--quiet"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice(&output).expect("scenario command emits valid JSON")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        eprintln!(
            "[{}] elapsed {:.2}s",
            self.label,
            self.started.elapsed().as_secs_f64()
        );
        if std::thread::panicking()
            && std::env::var_os("REPOSCOUT_SCENARIO_KEEP_FAILED").is_some_and(|value| value == "1")
            && let Some(directory) = self.directory.take()
        {
            eprintln!(
                "[{}] retained failing fixture: {}",
                self.label,
                directory.keep().display()
            );
        }
    }
}
