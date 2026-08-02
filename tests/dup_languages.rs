#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration tests intentionally fail immediately when fixtures or assertions are invalid"
)]

mod support;

use reposcout::dup::{DupInput, exact, fuzzy};
use reposcout::lang::{FIRST_CLASS_LANGUAGE_NAMES, RECOGNIZED_LANGUAGE_NAMES, detect};
use serde_json::Value;
use std::path::PathBuf;
use support::command::reposcout_command;
use support::dup_languages::{LanguageFixture, language_fixtures, materialize_fixture_tree};

const MIN_TOKENS: usize = 8;
const MIN_SIMILARITY: f64 = 0.85;

fn input(path: PathBuf, content: &str) -> DupInput {
    DupInput {
        path,
        content: content.to_string(),
    }
}

fn fixture_path(case: &LanguageFixture, copy: &str) -> PathBuf {
    PathBuf::from(&case.slug).join(copy).join(&case.filename)
}

#[test]
fn every_language_fixture_has_actionable_exact_and_type2_detection() {
    for case in language_fixtures() {
        let exact_a = fixture_path(&case, "exact_a");
        let exact_b = fixture_path(&case, "exact_b");
        let exact_groups = exact::detect(
            &[
                input(exact_a.clone(), &case.exact),
                input(exact_b.clone(), &case.exact),
            ],
            MIN_TOKENS,
        );
        let exact_group = exact_groups
            .iter()
            .find(|group| {
                group.format == case.name
                    && group
                        .instances
                        .iter()
                        .any(|instance| instance.path == exact_a)
                    && group
                        .instances
                        .iter()
                        .any(|instance| instance.path == exact_b)
            })
            .unwrap_or_else(|| panic!("{} must yield an exact clone", case.name));
        assert!(exact_group.tokens >= MIN_TOKENS, "{}", case.name);
        assert!(
            (exact_group.similarity - 1.0).abs() < f64::EPSILON,
            "{}",
            case.name
        );
        assert!(
            exact_group
                .instances
                .iter()
                .all(|instance| instance.end_line >= instance.start_line + 2),
            "{} exact clone must span at least three lines",
            case.name
        );

        let near_a = fixture_path(&case, "near_a");
        let near_b = fixture_path(&case, "near_b");
        let near_groups = fuzzy::detect(
            &[
                input(near_a.clone(), &case.near_a),
                input(near_b.clone(), &case.near_b),
            ],
            MIN_TOKENS,
            MIN_SIMILARITY,
        );
        let near_group = near_groups
            .iter()
            .find(|group| {
                group.format == case.name
                    && group
                        .instances
                        .iter()
                        .any(|instance| instance.path == near_a)
                    && group
                        .instances
                        .iter()
                        .any(|instance| instance.path == near_b)
            })
            .unwrap_or_else(|| panic!("{} must yield a Type-2 clone", case.name));
        assert!(near_group.tokens >= MIN_TOKENS, "{}", case.name);
        assert!(
            (MIN_SIMILARITY..1.0).contains(&near_group.similarity),
            "{} Type-2 similarity was {}",
            case.name,
            near_group.similarity
        );
        assert!(near_group.instances.iter().all(|instance| {
            instance.start_column > 0
                && instance.end_column > 0
                && instance.end_byte > instance.start_byte
                && instance.end_token >= instance.start_token
                && instance.end_line >= instance.start_line + 2
        }));
    }
}

#[test]
fn fixture_matrix_covers_capability_languages_and_detection() {
    let fixtures = language_fixtures();
    for case in &fixtures {
        let detected = detect(std::path::Path::new(&case.filename))
            .unwrap_or_else(|| panic!("{} fixture is not detected", case.name));
        assert_eq!(detected.name, case.name, "{} fixture", case.name);
    }

    let mut actual = fixtures
        .iter()
        .map(|case| case.name.clone())
        .collect::<Vec<_>>();
    actual.sort();
    let mut expected = RECOGNIZED_LANGUAGE_NAMES
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    expected.sort();

    assert_eq!(actual, expected, "recognized language capability drift");

    let mut actual_first_class = fixtures
        .iter()
        .filter_map(|case| detect(std::path::Path::new(&case.filename)))
        .filter(|language| language.is_first_class())
        .map(|language| language.name.to_string())
        .collect::<Vec<_>>();
    actual_first_class.sort();
    let mut expected_first_class = FIRST_CLASS_LANGUAGE_NAMES
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    expected_first_class.sort();

    assert_eq!(
        actual_first_class, expected_first_class,
        "first-class language capability drift"
    );
}

#[test]
fn cli_reports_exact_and_type2_findings_for_every_language_fixture() {
    let temp = tempfile::tempdir().expect("temporary fixture root");
    materialize_fixture_tree(temp.path()).expect("materialize language fixtures");

    let mut command = reposcout_command();
    let output = command
        .args([
            "dup",
            "-f",
            "json",
            "--health-scope",
            "all",
            "--no-cache",
            "--quiet",
            temp.path().to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).expect("valid reposcout JSON");

    for case in language_fixtures() {
        for (kind, left_copy, right_copy) in [
            ("exact", "exact_a", "exact_b"),
            ("near", "near_a", "near_b"),
        ] {
            let left = fixture_path(&case, left_copy)
                .to_string_lossy()
                .into_owned();
            let right = fixture_path(&case, right_copy)
                .to_string_lossy()
                .into_owned();
            let group = report["duplicates"][kind]
                .as_array()
                .unwrap()
                .iter()
                .find(|group| {
                    group["format"] == case.name
                        && group["instances"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|instance| instance["path"] == left)
                        && group["instances"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|instance| instance["path"] == right)
                })
                .unwrap_or_else(|| panic!("CLI must report {kind} {} clone", case.name));
            assert!(group["tokens"].as_u64().unwrap() >= MIN_TOKENS as u64);
            assert!(
                group["instances"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|instance| {
                        instance["end_line"].as_u64().unwrap()
                            >= instance["start_line"].as_u64().unwrap() + 2
                            && instance["start_column"].as_u64().unwrap() >= 1
                            && instance["end_byte"].as_u64().unwrap()
                                > instance["start_byte"].as_u64().unwrap()
                    })
            );
        }

        let near_a = fixture_path(&case, "near_a").to_string_lossy().into_owned();
        let near_b = fixture_path(&case, "near_b").to_string_lossy().into_owned();
        let detailed = report["duplicates"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|finding| {
                finding["kind"] == "type2"
                    && finding["format"] == case.name
                    && finding["fragment_a"]["path"] == near_a
                    && finding["fragment_b"]["path"] == near_b
            })
            .unwrap_or_else(|| panic!("CLI must detail the {} Type-2 pair", case.name));
        assert_eq!(detailed["id"].as_str().unwrap().len(), 32);
        assert_eq!(detailed["family_id"].as_str().unwrap().len(), 32);

        let language_summary = report["summary"]["duplication"]["by_language"]
            .as_array()
            .unwrap()
            .iter()
            .find(|language| language["name"] == case.name)
            .unwrap_or_else(|| panic!("CLI must aggregate {} duplication", case.name));
        assert!(language_summary["exact_groups"].as_u64().unwrap() >= 1);
        assert!(language_summary["near_groups"].as_u64().unwrap() >= 1);
        assert!(language_summary["duplicated_lines"].as_u64().unwrap() > 0);
        assert!(language_summary["duplicated_tokens"].as_u64().unwrap() > 0);
    }
}
