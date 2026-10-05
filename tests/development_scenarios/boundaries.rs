use super::support::Fixture;
use serde_json::Value;
use std::collections::BTreeSet;

fn paths(report: &Value) -> BTreeSet<String> {
    let files = report["files"].as_array().unwrap();
    let paths = files
        .iter()
        .map(|file| file["path"].as_str().unwrap().to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(paths.len(), files.len(), "inventory paths must be unique");
    paths
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn discovery_policy_agrees_with_historical_reads_without_narrowing_health_inventory() {
    let fixture = Fixture::new("policy across discovery and historical reads");
    fixture.write(".gitignore", "scratch/\n");
    fixture.write(".reposcoutignore", "vendor/\n");
    fixture.write("app/.reposcoutignore", "internal.ts\n");
    fixture.write("reposcout.toml", "health_excludes = [\"app/core.ts\"]\n");
    let source = "// TODO verify policy\nexport function run() { return 7; }\n";
    for path in [
        "app/core.ts",
        "app/internal.ts",
        "vendor/runtime.ts",
        "scratch/local.ts",
    ] {
        fixture.write(path, source);
    }
    let mut expected: BTreeSet<String> = ["app/core.ts", "reposcout.toml"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    for index in 0..32 {
        let path = format!("app/feature_{index}/entry.ts");
        fixture.write(
            &path,
            &format!("export function feature_{index}() {{ return {index}; }}\n"),
        );
        expected.insert(path);
    }
    let revision = fixture.commit("policy fixture");
    let scan = fixture.json(&["--profile", "agent", "--no-cache"]);
    assert_eq!(paths(&scan), expected);
    let core = scan["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "app/core.ts")
        .unwrap();
    assert!(
        core["complexity"].is_null(),
        "health exclusion removes health, not inventory"
    );

    let untrusted = fixture.json(&["--profile", "agent", "--no-cache", "--no-project-config"]);
    assert_eq!(
        paths(&untrusted),
        expected,
        "project trust boundary keeps ignore policy"
    );
    let core = untrusted["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "app/core.ts")
        .unwrap();
    assert!(!core["complexity"].is_null());

    let no_git_ignore = fixture.json(&["--profile", "agent", "--no-cache", "--no-ignore"]);
    expected.insert("scratch/local.ts".to_owned());
    assert_eq!(
        paths(&no_git_ignore),
        expected,
        "custom ignore still applies under --no-ignore"
    );

    for snapshot in ["worktree", revision.as_str()] {
        let read = fixture.json(&[
            "read",
            "--snapshot",
            snapshot,
            "--symbol",
            "app/core.ts",
            "run",
            "--symbol",
            "app/internal.ts",
            "run",
            "--symbol",
            "vendor/runtime.ts",
            "run",
            "--no-cache",
            "--budget",
            "8192",
        ]);
        let statuses: Vec<_> = read["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["status"].as_str().unwrap())
            .collect();
        assert_eq!(statuses, ["complete", "excluded", "excluded"]);
        assert_eq!(read["sources"].as_array().unwrap().len(), 1);
        assert_eq!(
            read["sources"][0]["content"],
            "export function run() { return 7; }"
        );
    }
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn input_limits_preserve_small_files_and_expose_missing_analysis_in_scan_and_read() {
    let fixture = Fixture::new("input gaps remain explicit");
    let mut expected = BTreeSet::new();
    for index in 0..40 {
        let path = format!("src/feature_{index:02}.ts");
        fixture.write(
            &path,
            &format!("export function feature_{index}() {{ return {index}; }}\n"),
        );
        expected.insert(path);
    }
    let huge = format!(
        "export function huge() {{ return {:?}; }}\n",
        "distinct_payload_".repeat(256)
    );
    fixture.write("src/huge.ts", &huge);
    let revision = fixture.commit("bounded large input");
    let limited = fixture.json(&[
        "--profile",
        "agent",
        "--max-file-bytes",
        "512",
        "--no-cache",
    ]);
    assert_eq!(paths(&limited), expected);
    assert_eq!(limited["diagnostics"]["discovered_files"], 41);
    assert_eq!(limited["diagnostics"]["analyzed_files"], 40);
    assert_eq!(limited["diagnostics"]["oversized_files"], 1);
    assert_eq!(limited["diagnostics"]["oversized_bytes"], huge.len());
    assert_eq!(limited["diagnostics"]["scan_truncated"], true);

    for snapshot in ["worktree", revision.as_str()] {
        let report = fixture.json(&[
            "read",
            "--snapshot",
            snapshot,
            "--symbol",
            "src/huge.ts",
            "huge",
            "--symbol",
            "src/feature_00.ts",
            "feature_0",
            "--max-file-bytes",
            "512",
            "--no-cache",
            "--budget",
            "8192",
        ]);
        assert_eq!(report["results"][0]["status"], "oversized");
        assert_eq!(report["results"][1]["status"], "complete");
        assert_eq!(report["sources"].as_array().unwrap().len(), 1);
        assert_eq!(
            report["sources"][0]["content"],
            "export function feature_0() { return 0; }"
        );
    }
    let complete = fixture.json(&["--profile", "agent", "--no-cache"]);
    expected.insert("src/huge.ts".to_owned());
    assert_eq!(paths(&complete), expected);
    assert_eq!(complete["diagnostics"]["analyzed_files"], 41);
    assert!(
        !complete["diagnostics"]["scan_truncated"]
            .as_bool()
            .unwrap_or(false)
    );
}
