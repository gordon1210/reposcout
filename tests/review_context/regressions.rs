use super::*;

fn revisions(old: &[(&str, &str)], new: &[(&str, &str)]) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    Repository::init(directory.path()).unwrap();
    for (path, source) in old {
        write(directory.path(), path, source);
    }
    let base = commit(directory.path());
    for (path, source) in new {
        write(directory.path(), path, source);
    }
    let head = commit(directory.path());
    Fixture {
        directory,
        base,
        head,
    }
}

#[test]
fn unavailable_side_is_unknown_change_evidence_in_both_directions() {
    let old = format!(
        "export function changed() {{ return 1; }}\n// {}\n",
        "x".repeat(1024)
    );
    let mut fixture = revisions(
        &[
            ("lib.ts", &old),
            (
                "consumer.ts",
                "import { changed } from './lib';\nexport function useIt() { return changed(); }\n",
            ),
        ],
        &[("lib.ts", "export function changed() { return 2; }\n")],
    );
    for unavailable in ["base", "head"] {
        let full = fixture.report(&["--max-file-bytes", "4096"]);
        assert_eq!(full["changes"][0]["hunk_status"], "available");
        assert_eq!(full["changes"][0]["hunks"], 1);
        let report = fixture.report(&["--max-file-bytes", "256"]);
        let change = &report["changes"][0];
        assert_eq!(change[unavailable]["status"], "oversized");
        assert_eq!(change["hunk_status"], "unavailable");
        assert_eq!(change.get("hunks"), Some(&Value::Null));
        assert_eq!(change["base"]["mapping_status"], "unavailable");
        assert_eq!(change["head"]["mapping_status"], "unavailable");
        assert_eq!(report["totals"]["changes_without_hunks"], 1);
        assert!(change["diff_tokens"].is_null());
        std::mem::swap(&mut fixture.base, &mut fixture.head);
    }
}

#[test]
fn half_captured_pair_is_not_complete_change_analysis() {
    let fixture = revisions(
        &[("lib.ts", "export const value = 1;\n")],
        &[("lib.ts", "export const value = 2;\n")],
    );
    let report = fixture.report(&["--max-files", "1"]);
    assert_eq!(
        report["changes"][0]["head"]["status"],
        "input-budget-exceeded"
    );
    assert_eq!(report["changes"][0]["hunk_status"], "unavailable");
    assert_eq!(report["changes"][0].get("hunks"), Some(&Value::Null));
    assert_eq!(report["totals"]["changes_without_hunks"], 1);
    assert_eq!(report["totals"]["changes_omitted"], 0);
}

#[test]
fn unrelated_resolver_configuration_cannot_displace_proven_consumers() {
    let config = r#"{"compilerOptions":{"baseUrl":".","paths":{"@local/*":["./*"]}}}"#;
    let fixture = revisions(
        &[
            ("a/tsconfig.json", config),
            ("b/tsconfig.json", config),
            ("a/lib.ts", "export function value() { return 1; }\n"),
            ("b/lib.ts", "export function value() { return 1; }\n"),
            (
                "a/main.ts",
                "import { value } from '@local/lib'; export function run() { return value(); }\n",
            ),
            (
                "b/main.ts",
                "import { value } from '@local/lib'; export function run() { return value(); }\n",
            ),
        ],
        &[("b/lib.ts", "export function value() { return 2; }\n")],
    );
    let report = fixture.report(&["--context-max-files", "4"]);
    for side in ["base", "head"] {
        assert_eq!(
            candidate(&report, side, "b/main.ts")["selection"],
            "selected"
        );
        assert!(has_role(
            candidate(&report, side, "b/main.ts"),
            "concrete-reference-source"
        ));
    }
    assert!(
        report["context"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["path"] != "a/tsconfig.json")
    );
}

#[test]
fn relation_projection_preserves_both_revision_sides() {
    let directory = tempfile::tempdir().unwrap();
    Repository::init(directory.path()).unwrap();
    write(directory.path(), "lib.ts", "export const value = 1;\n");
    for index in 0..101 {
        write(
            directory.path(),
            &format!("consumer{index:03}.ts"),
            "import './lib';\n",
        );
    }
    let base = commit(directory.path());
    write(directory.path(), "lib.ts", "export const value = 2;\n");
    let head = commit(directory.path());
    let report = Fixture {
        directory,
        base,
        head,
    }
    .report(&[]);
    assert_eq!(report["totals"]["relations"], 202);
    assert_eq!(report["totals"]["relations_omitted"], 102);
    for side in ["base", "head"] {
        assert_eq!(
            report["relations"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|relation| relation["side"] == side)
                .count(),
            50
        );
    }
}

#[test]
fn human_output_preserves_identity_mapping_and_type_evidence() {
    let fixture = revisions(
        &[
            ("types.ts", "export class Base { value() { return 1; } }\n"),
            (
                "derived.ts",
                "import { Base } from './types'; export class Derived extends Base {}\n",
            ),
        ],
        &[("types.ts", "export class Base { value() { return 2; } }\n")],
    );
    let json = fixture.report(&[]);
    for format in ["table", "markdown"] {
        let output = fixture
            .command_format(&[], format)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let text = std::str::from_utf8(&output).unwrap();
        assert!(text.contains(json["changes"][0]["head"]["blob"].as_str().unwrap()));
        assert!(text.contains("extends"));
        assert!(text.contains("distance=1"));
        assert!(text.contains("ranges=1-1"));
    }
}

#[test]
fn unsupported_unix_graph_paths_cannot_alias_a_regular_path() {
    let mut fixture = revisions(
        &[
            ("foo\\bar.ts", "export function value() { return 1; }\n"),
            ("foo/bar.ts", "export function value() { return 7; }\n"),
            (
                "consumer.ts",
                "import { value } from './foo/bar'; export function useIt() { return value(); }\n",
            ),
        ],
        &[("foo\\bar.ts", "export function value() { return 2; }\n")],
    );
    let report = fixture.report(&[]);
    for coverage in report["coverage"].as_array().unwrap() {
        assert_eq!(coverage["unsupported_graph_paths"], 1);
        assert_eq!(coverage["changed_graph_files"], 0);
        assert_eq!(coverage["changed_files_without_graph"], 1);
        assert_eq!(coverage["unresolved_imports"], 0);
    }
    assert_eq!(report["relations"].as_array().unwrap().len(), 0);
    assert_eq!(
        candidate(&report, "head", "foo\\bar.ts")["status"],
        "captured"
    );
    fixture.base = fixture.head;
    write(
        fixture.path(),
        "foo/bar.ts",
        "export function value() { return 8; }\n",
    );
    fixture.head = commit(fixture.path());
    let ordinary = fixture.report(&[]);
    assert_eq!(ordinary["coverage"][1]["changed_graph_files"], 1);
    assert!(has_role(
        candidate(&ordinary, "head", "consumer.ts"),
        "concrete-reference-source"
    ));
}
