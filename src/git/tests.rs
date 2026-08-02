use super::churn_cache::ChurnCache;
use super::native_churn::NativeGit;
use super::{
    ChurnLimits, DiffScope, changed_files_with_base, collect, collect_impl_with_native,
    diff_base_tree_id,
};
use git2::Repository;
use std::fs;
use std::path::{Path, PathBuf};

fn commit_all(repo: &Repository, message: &str) -> git2::Oid {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("reposcout tests", "tests@example.com").unwrap();
    let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents = parent.iter().collect::<Vec<_>>();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parents,
    )
    .unwrap()
}

#[test]
fn collect_git_churn_for_repo_files() {
    if Repository::discover(".").is_err() {
        return;
    }

    let files = [
        PathBuf::from("src/main.rs"),
        PathBuf::from("src/git.rs"),
        PathBuf::from("does/not/exist.rs"),
    ];
    let churn = collect(Path::new("."), &files, 0);

    let main = churn
        .get(Path::new("src/main.rs"))
        .expect("src/main.rs should be present in git history");
    assert!(main.commits >= 1);
    assert!(main.authors >= 1);
    assert!(main.last_commit.is_some());
    assert!(!churn.contains_key(Path::new("does/not/exist.rs")));
}

#[test]
fn churn_follows_file_history_across_renames() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let old = dir.path().join("old.rs");
    let new = dir.path().join("new.rs");
    fs::write(&old, "fn value() -> i32 { 1 }\n").unwrap();
    commit_all(&repo, "add old");
    fs::write(&old, "fn value() -> i32 { 2 }\n").unwrap();
    commit_all(&repo, "modify old");
    fs::rename(&old, &new).unwrap();
    commit_all(&repo, "rename old to new");

    let churn = collect(dir.path(), &[PathBuf::from("new.rs")], 0);
    assert_eq!(churn[Path::new("new.rs")].commits, 3);
}

#[test]
fn churn_does_not_double_count_a_change_and_its_merge_commit() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let source = dir.path().join("lib.rs");
    fs::write(&source, "fn value() -> i32 { 1 }\n").unwrap();
    let root = commit_all(&repo, "root");
    let main_ref = repo.head().unwrap().name().unwrap().to_string();

    let root_commit = repo.find_commit(root).unwrap();
    repo.branch("feature", &root_commit, false).unwrap();
    drop(root_commit);
    repo.set_head("refs/heads/feature").unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    fs::write(&source, "fn value() -> i32 { 2 }\n").unwrap();
    let feature = commit_all(&repo, "feature change");

    repo.set_head(&main_ref).unwrap();
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    fs::write(dir.path().join("other.rs"), "fn other() {}\n").unwrap();
    let main = commit_all(&repo, "main change");

    fs::write(&source, "fn value() -> i32 { 2 }\n").unwrap();
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("reposcout tests", "tests@example.com").unwrap();
    let main_parent = repo.find_commit(main).unwrap();
    let feature_parent = repo.find_commit(feature).unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "merge feature",
        &tree,
        &[&main_parent, &feature_parent],
    )
    .unwrap();

    let churn = collect(dir.path(), &[PathBuf::from("lib.rs")], 0);
    assert_eq!(churn[Path::new("lib.rs")].commits, 2);
}

#[test]
fn native_git_stream_matches_libgit2_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let old = dir.path().join("old.rs");
    let new = dir.path().join("new.rs");
    let other = dir.path().join("other.rs");
    fs::write(&old, "fn value() -> i32 { 1 }\n").unwrap();
    fs::write(&other, "fn other() {}\n").unwrap();
    commit_all(&repo, "add files");
    fs::write(&old, "fn value() -> i32 { 2 }\n").unwrap();
    commit_all(&repo, "modify old");
    fs::rename(&old, &new).unwrap();
    commit_all(&repo, "rename old to new");
    let wanted = [PathBuf::from("new.rs"), PathBuf::from("other.rs")];

    let mut native_cache =
        ChurnCache::for_repo(dir.path(), false, ChurnLimits::default().max_cache_bytes);
    let (native_result, native_stats) = collect_impl_with_native(
        dir.path(),
        &wanted,
        &ChurnLimits::with_max_commits(0),
        &mut native_cache,
        &NativeGit::default(),
    );
    let mut fallback_cache =
        ChurnCache::for_repo(dir.path(), false, ChurnLimits::default().max_cache_bytes);
    let (fallback_result, fallback_stats) = collect_impl_with_native(
        dir.path(),
        &wanted,
        &ChurnLimits::with_max_commits(0),
        &mut fallback_cache,
        &NativeGit::with_executable(dir.path().join("missing-git")),
    );

    assert_eq!(
        serde_json::to_value(native_result.churn).unwrap(),
        serde_json::to_value(fallback_result.churn).unwrap()
    );
    assert_eq!(native_stats.native_batches, 1);
    assert_eq!(native_stats.native_events, 2);
    assert_eq!(native_stats.native_fallbacks, 0);
    assert_eq!(fallback_stats.native_fallbacks, 1);
    assert_eq!(fallback_stats.tree_diffs, 3);
}

#[test]
fn churn_only_probes_renames_when_a_tracked_addition_can_be_one() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let source = dir.path().join("lib.rs");
    fs::write(&source, "fn value() -> i32 { 1 }\n").unwrap();
    commit_all(&repo, "add");
    fs::write(&source, "fn value() -> i32 { 2 }\n").unwrap();
    commit_all(&repo, "modify once");
    fs::write(&source, "fn value() -> i32 { 3 }\n").unwrap();
    commit_all(&repo, "modify twice");

    let mut cache = ChurnCache::for_repo(dir.path(), false, ChurnLimits::default().max_cache_bytes);
    let missing_git = dir.path().join("missing-git");
    let (churn_result, stats) = collect_impl_with_native(
        dir.path(),
        &[PathBuf::from("lib.rs")],
        &ChurnLimits::with_max_commits(0),
        &mut cache,
        &NativeGit::with_executable(missing_git),
    );

    assert_eq!(churn_result.churn[Path::new("lib.rs")].commits, 3);
    assert_eq!(stats.tree_diffs, 3);
    assert_eq!(stats.rename_probes, 0);
    assert_eq!(stats.native_fallbacks, 1);
}

#[test]
fn churn_cache_reuses_views_and_only_diffs_new_commits() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let old = dir.path().join("old.rs");
    let new = dir.path().join("new.rs");
    fs::write(&old, "fn value() -> i32 { 1 }\n").unwrap();
    commit_all(&repo, "add old");
    fs::write(&old, "fn value() -> i32 { 2 }\n").unwrap();
    commit_all(&repo, "modify old");
    fs::rename(&old, &new).unwrap();
    commit_all(&repo, "rename old to new");
    let wanted = [PathBuf::from("new.rs")];

    let mut cold_cache = ChurnCache::for_test(cache_dir.path());
    let (cold_result, cold_stats) = collect_impl_with_native(
        dir.path(),
        &wanted,
        &ChurnLimits::with_max_commits(0),
        &mut cold_cache,
        &NativeGit::default(),
    );
    assert_eq!(cold_result.churn[Path::new("new.rs")].commits, 3);
    assert_eq!(cold_stats.tree_diffs, 2);
    assert_eq!(cold_stats.rename_probes, 1);
    assert_eq!(cold_stats.native_batches, 1);
    assert_eq!(cold_stats.native_events, 2);
    assert_eq!(cold_stats.native_fallbacks, 0);
    assert_eq!(cold_stats.event_hits, 0);
    assert_eq!(cold_stats.view_hits, 0);

    let mut warm_cache = ChurnCache::for_test(cache_dir.path());
    let (warm_result, warm_stats) = collect_impl_with_native(
        dir.path(),
        &wanted,
        &ChurnLimits::with_max_commits(0),
        &mut warm_cache,
        &NativeGit::default(),
    );
    assert_eq!(warm_result.churn[Path::new("new.rs")].commits, 3);
    assert_eq!(warm_stats.tree_diffs, 0);
    assert_eq!(warm_stats.view_hits, 1);

    fs::write(&new, "fn value() -> i32 { 3 }\n").unwrap();
    commit_all(&repo, "modify new");
    let mut advanced_cache = ChurnCache::for_test(cache_dir.path());
    let (advanced_result, advanced_stats) = collect_impl_with_native(
        dir.path(),
        &wanted,
        &ChurnLimits::with_max_commits(0),
        &mut advanced_cache,
        &NativeGit::default(),
    );
    assert_eq!(advanced_result.churn[Path::new("new.rs")].commits, 4);
    assert_eq!(advanced_stats.tree_diffs, 0);
    assert_eq!(advanced_stats.native_batches, 1);
    assert_eq!(advanced_stats.native_events, 1);
    assert_eq!(advanced_stats.event_hits, 3);
    assert_eq!(advanced_stats.rename_probes, 0);
    assert_eq!(advanced_stats.view_hits, 0);

    let mut capped_cache = ChurnCache::for_test(cache_dir.path());
    let (capped_result, capped_stats) = collect_impl_with_native(
        dir.path(),
        &wanted,
        &ChurnLimits::with_max_commits(2),
        &mut capped_cache,
        &NativeGit::default(),
    );
    assert_eq!(capped_result.churn[Path::new("new.rs")].commits, 2);
    assert_eq!(capped_stats.tree_diffs, 0);
    assert_eq!(capped_stats.event_hits, 2);
    assert_eq!(capped_stats.view_hits, 0);
}

#[test]
fn diff_base_uses_resolved_tree_identity() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join("lib.rs"), "fn value() {}\n").unwrap();
    commit_all(&repo, "initial");

    let head = diff_base_tree_id(dir.path(), &DiffScope::Since("HEAD".to_string())).unwrap();
    let working = diff_base_tree_id(dir.path(), &DiffScope::Working).unwrap();
    assert_eq!(head, working);
    assert!(head.is_some());
}

#[test]
fn changed_files_can_reuse_a_previously_resolved_base_tree() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let source = dir.path().join("lib.rs");
    fs::write(&source, "fn value() -> u8 { 1 }\n").unwrap();
    commit_all(&repo, "first");
    let base = diff_base_tree_id(dir.path(), &DiffScope::Working)
        .unwrap()
        .unwrap();
    fs::write(&source, "fn value() -> u8 { 2 }\n").unwrap();
    commit_all(&repo, "second");

    let changed = changed_files_with_base(dir.path(), &DiffScope::Working, Some(&base)).unwrap();
    assert!(changed.contains(Path::new("lib.rs")));
}
