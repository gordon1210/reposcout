use super::regressions::revisions;
use super::*;

fn read_snapshot(fixture: &Fixture, snapshot: &str, path: &str, expected: Option<&str>) -> Value {
    let mut command = test_command::reposcout_command();
    command.arg("read").arg(fixture.path()).args([
        "--snapshot",
        snapshot,
        "--symbol",
        path,
        "answer",
        "--no-cache",
        "--no-project-config",
        "-f",
        "json",
    ]);
    if let Some(hash) = expected {
        command.args(["--expect-hash", path, hash]);
    }
    serde_json::from_slice(&command.assert().success().get_output().stdout).unwrap()
}

#[test]
fn post_review_snapshot_handoff_ignores_live_source_parent_type() {
    let fixture = revisions(
        &[("src/api.ts", "export function answer() { return 1; }\n")],
        &[("src/api.ts", "export function answer() { return 2; }\n")],
    );
    let report = fixture.report(&[]);
    let tree = report["comparison"]["head_tree"].as_str().unwrap();
    let hash = report["changes"][0]["head"]["sha256"].as_str().unwrap();
    let original = read_snapshot(&fixture, tree, "src/api.ts", Some(hash));
    assert_eq!(original["results"][0]["status"], "complete");
    let saved = tempfile::tempdir().unwrap();
    fs::rename(fixture.path().join("src"), saved.path().join("src")).unwrap();
    assert_eq!(
        read_snapshot(&fixture, tree, "src/api.ts", Some(hash)),
        original
    );
    assert_eq!(
        read_snapshot(&fixture, "index", "src/api.ts", Some(hash))["results"][0]["status"],
        "complete"
    );
    std::os::unix::fs::symlink(saved.path().join("src"), fixture.path().join("src")).unwrap();
    assert_eq!(fixture.report(&[]), report);
    assert_eq!(
        read_snapshot(&fixture, "index", "src/api.ts", Some(hash))["results"][0]["status"],
        "complete"
    );
    assert_eq!(
        read_snapshot(&fixture, tree, "src/api.ts", Some(hash)),
        original
    );
    assert_eq!(
        read_snapshot(&fixture, "worktree", "src/api.ts", None)["results"][0]["status"],
        "not-regular-file"
    );
    assert_eq!(
        read_snapshot(&fixture, tree, "src/api.ts", Some(&"0".repeat(64)))["results"][0]["status"],
        "stale"
    );
}

#[test]
fn post_review_snapshot_still_rejects_a_symlink_in_the_git_tree() {
    let mut fixture = revisions(
        &[("src/api.ts", "export function answer() { return 1; }\n")],
        &[],
    );
    std::os::unix::fs::symlink("src/api.ts", fixture.path().join("alias.ts")).unwrap();
    fixture.head = commit(fixture.path());
    assert_eq!(
        read_snapshot(&fixture, &fixture.head.to_string(), "alias.ts", None)["results"][0]["status"],
        "not-regular-file"
    );
    assert_eq!(
        read_snapshot(&fixture, "index", "alias.ts", None)["results"][0]["status"],
        "not-regular-file"
    );
}
