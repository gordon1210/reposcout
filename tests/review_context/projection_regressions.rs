use super::regressions::revisions;
use super::*;
use std::fmt::Write as _;

#[test]
fn post_review_restores_impact_and_requested_bodies_after_large_change_eviction() {
    let large = format!("a/{}/lib.ts", vec!["q".repeat(140); 5].join("/"));
    let fixture = revisions(
        &[
            (&large, "export function large() { return 1; }\n"),
            ("m.ts", "export function small() { return 1; }\n"),
            (
                "consumer.ts",
                "import { small } from './m';\nexport function run() { return small(); }\n",
            ),
        ],
        &[
            (&large, "export function large() { return 2; }\n"),
            ("m.ts", "export function small() { return 2; }\n"),
        ],
    );
    for content_flag in [None, Some("--diff"), Some("--source")] {
        let mut command = test_command::reposcout_command();
        command.arg("review-context").arg(fixture.path()).args([
            "--base",
            &fixture.base.to_string(),
            "--head",
            &fixture.head.to_string(),
            "--no-cache",
            "--no-project-config",
            "--budget",
            "65536",
            "--max-output-bytes",
            "5000",
            "-f",
            "json",
        ]);
        if let Some(flag) = content_flag {
            command.arg(flag);
        }
        let output = command.assert().success().get_output().stdout.clone();
        assert!(output.len() <= 5000);
        let report: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(report["changes"].as_array().unwrap().len(), 1);
        assert_eq!(report["changes"][0]["head"]["path"], "m.ts");
        let relations = report["relations"].as_array().unwrap();
        let context = report["context"].as_array().unwrap();
        assert_ne!(relations.len(), 0, "{content_flag:?}: {report}");
        assert_ne!(context.len(), 0, "{content_flag:?}: {report}");
        assert_eq!(
            report["changes"][0]["diff"].is_string(),
            content_flag == Some("--diff")
        );
        assert_eq!(
            context.iter().any(|file| file["source"].is_string()),
            content_flag == Some("--source")
        );
        for (key, retained) in [
            ("relations", relations.len()),
            ("candidates", context.len()),
        ] {
            assert_eq!(
                report["totals"][key].as_u64().unwrap(),
                retained as u64 + report["totals"][format!("{key}_omitted")].as_u64().unwrap()
            );
        }
    }
}

#[test]
fn post_review_token_budget_readmits_a_larger_but_affordable_identity() {
    // Keep physical fixture paths below Darwin's limit, including the temporary root.
    let cheap = format!("cheap/{}/lib.ts", vec!["a".repeat(180); 3].join("/"));
    let mut state = 42_u64;
    let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let expensive = format!(
        "expensive/{}/lib.ts",
        (0..3)
            .map(|_| {
                (0..150)
                    .map(|_| {
                        state = state
                            .wrapping_mul(6_364_136_223_846_793_005)
                            .wrapping_add(1);
                        char::from(alphabet[((state >> 32) % alphabet.len() as u64) as usize])
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("/")
    );
    let fixture = revisions(
        &[
            (&cheap, "export function value() { return 1; }\n"),
            (&expensive, "export function value() { return 1; }\n"),
        ],
        &[
            (&cheap, "export function value() { return 2; }\n"),
            (&expensive, "export function value() { return 2; }\n"),
        ],
    );
    let mut command = test_command::reposcout_command();
    let output = command
        .arg("review-context")
        .arg(fixture.path())
        .args([
            "--base",
            &fixture.base.to_string(),
            "--head",
            &fixture.head.to_string(),
            "--no-cache",
            "--no-project-config",
            "--budget",
            "1500",
            "-f",
            "json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(report["changes"].as_array().unwrap().len(), 1);
    assert_eq!(report["changes"][0]["head"]["path"], cheap);
    assert_eq!(report["totals"]["changes_omitted"], 1);
    let rendered = std::str::from_utf8(&output).unwrap();
    assert!(TokenCounter::new("o200k_base").unwrap().count(rendered) <= 1500);
    assert!(rendered.len() <= 65536);
}

#[test]
fn post_review_oversized_identity_does_not_displace_small_change_details() {
    for prefix in ["a", "z"] {
        let directories = vec!["q".repeat(180); 3].join("/");
        let large = format!("{prefix}/{directories}/lib.ts");
        let small = "m.ts";
        let fixture = revisions(
            &[
                (&large, "export function large() { return 1; }\n"),
                (small, "export function small() { return 1; }\n"),
            ],
            &[
                (&large, "export function large() { return 2; }\n"),
                (small, "export function small() { return 2; }\n"),
            ],
        );
        let mut command = test_command::reposcout_command();
        let output = command
            .arg("review-context")
            .arg(fixture.path())
            .args([
                "--base",
                &fixture.base.to_string(),
                "--head",
                &fixture.head.to_string(),
                "--no-cache",
                "--no-project-config",
                "--budget",
                "65536",
                "--max-output-bytes",
                "5000",
                "-f",
                "json",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        assert!(output.len() <= 5000);
        let report: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(report["totals"]["changes_omitted"], 1);
        assert_eq!(report["changes"].as_array().unwrap().len(), 1);
        let change = &report["changes"][0];
        for side in ["base", "head"] {
            assert_eq!(change[side]["path"], small);
            assert_eq!(
                change[side]["ranges"],
                serde_json::json!([{"start": 1, "end": 1}])
            );
            assert_eq!(change[side]["definitions"].as_array().unwrap().len(), 1);
        }
    }
}

#[test]
fn post_review_default_budget_preserves_small_changes_under_path_permutation() {
    for (large, small) in [("a.ts", "z.ts"), ("z.ts", "a.ts")] {
        let mut old = String::new();
        for i in 0..20 {
            writeln!(old, "export function f{i}() {{ return 1; }}").unwrap();
        }
        let new = old.replace("return 1", "return 2");
        let fixture = revisions(
            &[(large, &old), (small, "export const z = 1\n")],
            &[(large, &new), (small, "export const z = 2\n")],
        );
        let mut command = test_command::reposcout_command();
        let output = command
            .arg("review-context")
            .arg(fixture.path())
            .args([
                "--base",
                &fixture.base.to_string(),
                "--head",
                &fixture.head.to_string(),
                "--no-cache",
                "--no-project-config",
                "-f",
                "json",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let report: Value = serde_json::from_slice(&output).unwrap();
        let paths: Vec<_> = report["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| change["head"]["path"].as_str().unwrap())
            .collect();
        assert!(paths.contains(&small), "small change lost: {report}");
        assert!(
            paths.contains(&large),
            "large change identity lost: {report}"
        );
        assert_eq!(report["totals"]["changes_omitted"], 0);
        let side_omissions: u64 = report["changes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|change| [&change["base"], &change["head"]])
            .map(|side| side["definitions_omitted"].as_u64().unwrap_or(0))
            .sum();
        assert_eq!(
            side_omissions,
            report["totals"]["definitions_omitted"].as_u64().unwrap()
        );
        let shown: usize = report["changes"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|change| [&change["base"], &change["head"]])
            .map(|side| side["definitions"].as_array().unwrap().len())
            .sum();
        assert_eq!(
            shown as u64 + report["totals"]["definitions_omitted"].as_u64().unwrap(),
            40
        );
        let counter = TokenCounter::new("o200k_base").unwrap();
        assert!(counter.count(std::str::from_utf8(&output).unwrap()) <= 4096);
        assert!(output.len() <= 65_536);
    }
}
