use super::regressions::revisions;
use super::*;
use std::fmt::Write as _;

const GUARDED: &str =
    "pub fn answer(value: i32) -> i32 {\n    assert!(value >= 0);\n    value\n}\n";
const UNGUARDED: &str = "pub fn answer(value: i32) -> i32 {\n    value\n}\n";

fn names(side: &Value) -> Vec<&str> {
    side["definitions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|definition| definition["symbol"]["name"].as_str().unwrap())
        .collect()
}

#[test]
fn post_review_body_insertions_and_deletions_retain_both_callers() {
    let mut fixture = revisions(
        &[
            ("src/lib.rs", "pub mod api;\npub mod caller;\n"),
            ("src/api.rs", GUARDED),
            (
                "src/caller.rs",
                "pub fn run() -> i32 { crate::api::answer(-1) }\n",
            ),
        ],
        &[("src/api.rs", UNGUARDED)],
    );
    for _ in 0..2 {
        let report = fixture.report(&[]);
        for side in ["base", "head"] {
            assert_eq!(names(&report["changes"][0][side]), ["answer"]);
            assert_eq!(report["changes"][0][side]["mapping_status"], "available");
            assert!(has_role(
                candidate(&report, side, "src/caller.rs"),
                "concrete-reference-source"
            ));
            assert_eq!(
                report["relations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|relation| {
                        relation["side"] == side
                            && relation["kind"] == "symbol-reference"
                            && relation["edge"]["source"] == "src/caller.rs"
                            && relation["change_basis"] == "changed-definition"
                    })
                    .count(),
                1
            );
        }
        assert_eq!(report["totals"]["definitions_omitted"], 0);
        assert_eq!(report["totals"]["relations_omitted"], 0);
        std::mem::swap(&mut fixture.base, &mut fixture.head);
    }
}

#[test]
fn post_review_counterparts_preserve_incomplete_seed_mapping_in_both_directions() {
    let mut old = String::new();
    for index in 0..1100 {
        writeln!(
            old,
            "pub fn f{index}(value: i32) -> i32 {{\n    assert!(value >= 0);\n    value\n}}\n"
        )
        .unwrap();
    }
    let new = old.replace("    assert!(value >= 0);\n", "");
    let mut fixture = revisions(
        &[
            ("src/lib.rs", "pub mod api;\npub mod caller;\n"),
            ("src/api.rs", &old),
            (
                "src/caller.rs",
                "pub fn run() -> i32 { crate::api::f1099(-1) }\n",
            ),
        ],
        &[("src/api.rs", &new)],
    );
    for counterpart_side in ["head", "base"] {
        let report = fixture.report(&[]);
        let seed_side = if counterpart_side == "head" {
            "base"
        } else {
            "head"
        };
        assert!(
            report["changes"][0][seed_side]["unprocessed_ranges"]
                .as_u64()
                .unwrap()
                > 0
        );
        let counterpart = &report["changes"][0][counterpart_side];
        assert_eq!(counterpart["counterpart_seed_mapping_incomplete"], true);
        assert_eq!(
            counterpart["unprocessed_counterparts"]
                .as_u64()
                .unwrap_or(0),
            0
        );
        for side in ["base", "head"] {
            assert_eq!(report["changes"][0][side]["mapping_status"], "partial");
            assert!(
                report["relations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|relation| {
                        relation["side"] == side
                            && relation["kind"] == "symbol-reference"
                            && relation["edge"]["source"] == "src/caller.rs"
                            && relation["change_basis"] == "changed-file"
                    })
            );
        }
        std::mem::swap(&mut fixture.base, &mut fixture.head);
    }
}

#[test]
fn post_review_counterparts_respect_nested_and_indentation_boundaries() {
    for (path, old, new, expected) in [
        (
            "lib.rs",
            "fn outer() {\n    fn inner() {\n        check();\n        work();\n    }\n}\n",
            "fn outer() {\n    fn inner() {\n        work();\n    }\n}\n",
            "inner",
        ),
        (
            "lib.ts",
            "export function answer() {\n    check();\n    return 1;\n}\n",
            "export function answer() {\n    return 1;\n}\n",
            "answer",
        ),
        (
            "lib.py",
            "def answer():\n    work()\n    check()\n",
            "def answer():\n    work()\n",
            "answer",
        ),
        (
            "lib.go",
            "package fixture\nfunc answer() {\n    check()\n    work()\n}\n",
            "package fixture\nfunc answer() {\n    work()\n}\n",
            "answer",
        ),
        (
            "lib.js",
            "export const answer = () => {\n    check();\n    return 1;\n}, stable = () => 2;\n",
            "export const answer = () => {\n    return 1;\n}, stable = () => 2;\n",
            "answer",
        ),
        (
            "lib.rs",
            "pub fn answer(\n    value: i32,\n    extra: i32,\n) -> i32 { value }\n",
            "pub fn answer(\n    value: i32,\n) -> i32 { value }\n",
            "answer",
        ),
    ] {
        let mut fixture = revisions(&[(path, old)], &[(path, new)]);
        for _ in 0..2 {
            let report = fixture.report(&[]);
            for side in ["base", "head"] {
                assert_eq!(
                    names(&report["changes"][0][side]),
                    [expected],
                    "{path} {side}"
                );
            }
            std::mem::swap(&mut fixture.base, &mut fixture.head);
        }
    }
}

#[test]
fn post_review_unchanged_header_offsets_keep_same_named_scopes_distinct() {
    let old = "mod first {\n    pub fn answer() {\n        check();\n        work();\n    }\n}\nmod second {\n    pub fn answer() {\n        work();\n    }\n}\n";
    let new = format!("// offset\n{}", old.replacen("        check();\n", "", 1));
    let mut fixture = revisions(&[("lib.rs", old)], &[("lib.rs", &new)]);
    for (base_line, head_line) in [(2, 3), (3, 2)] {
        let report = fixture.report(&[]);
        for (side, line) in [("base", base_line), ("head", head_line)] {
            assert_eq!(names(&report["changes"][0][side]), ["answer"]);
            assert_eq!(
                report["changes"][0][side]["definitions"][0]["symbol"]["line"],
                line
            );
        }
        std::mem::swap(&mut fixture.base, &mut fixture.head);
    }
}

#[test]
fn post_review_new_declarations_never_mark_unchanged_neighbors() {
    let old = "fn before() {}\nfn after() {}\n";
    for new in [
        "fn added() {}\nfn before() {}\nfn after() {}\n",
        "fn before() {}\nfn added() {}\nfn after() {}\n",
        "fn before() {}\nfn after() {}\nfn added() {}\n",
    ] {
        let mut fixture = revisions(&[("lib.rs", old)], &[("lib.rs", new)]);
        for changed in ["head", "base"] {
            let report = fixture.report(&[]);
            let unchanged = if changed == "head" { "base" } else { "head" };
            assert_eq!(names(&report["changes"][0][changed]), ["added"]);
            assert!(names(&report["changes"][0][unchanged]).is_empty());
            std::mem::swap(&mut fixture.base, &mut fixture.head);
        }
    }
}

#[test]
fn post_review_added_or_deleted_file_has_no_fabricated_side() {
    let mut fixture = revisions(&[("keep.rs", "fn keep() {}\n")], &[("lib.rs", GUARDED)]);
    for existing in ["head", "base"] {
        let report = fixture.report(&[]);
        let absent = if existing == "head" { "base" } else { "head" };
        assert!(report["changes"][0][absent].is_null());
        assert_eq!(names(&report["changes"][0][existing]), ["answer"]);
        std::mem::swap(&mut fixture.base, &mut fixture.head);
    }
}
