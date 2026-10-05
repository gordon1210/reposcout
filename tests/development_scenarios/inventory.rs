//! Repository-level inventory and health journeys, with hand-derived expectations.

use super::support::Fixture;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

fn files(report: &Value) -> &[Value] {
    report["files"].as_array().unwrap()
}

fn file<'a>(report: &'a Value, path: &str) -> &'a Value {
    files(report)
        .iter()
        .find(|file| file["path"] == path)
        .unwrap_or_else(|| panic!("missing inventory entry {path}"))
}

fn paths(report: &Value) -> BTreeSet<String> {
    let identities: BTreeSet<_> = files(report)
        .iter()
        .map(|file| file["path"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        identities.len(),
        files(report).len(),
        "duplicate inventory paths"
    );
    identities
}

fn inventory(report: &Value) -> BTreeMap<String, Value> {
    let facts: BTreeMap<_, _> = files(report)
        .iter()
        .map(|file| {
            (
                file["path"].as_str().unwrap().to_owned(),
                json!({
                    "language": file["language"].as_str().unwrap(),
                    "bytes": file["bytes"].as_u64().unwrap(),
                    "tokens": file["tokens"].as_u64().unwrap(),
                    "loc": file["loc"].as_u64().unwrap(),
                    "sloc": file["sloc"].as_u64().unwrap(),
                    "comment_lines": file["comment_lines"].as_u64().unwrap(),
                    "symbols": file["symbols"],
                }),
            )
        })
        .collect();
    assert_eq!(
        facts.len(),
        files(report).len(),
        "duplicate inventory paths"
    );
    facts
}

fn semantic_report(report: &Value) -> Value {
    let mut facts = report.clone();
    let object = facts.as_object_mut().unwrap();
    object.remove("generated_at");
    object.remove("execution");
    facts
}

fn count(value: &Value, key: &str) -> u64 {
    value.get(key).map_or(0, |number| {
        number
            .as_u64()
            .unwrap_or_else(|| panic!("invalid count {key}: {number}"))
    })
}

fn write_code(fixture: &Fixture, path: &str, source: &str) {
    let language = reposcout::lang::detect(Path::new(path)).unwrap();
    let tree = reposcout::parse::parse(language.first_class.unwrap(), source).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "scenario source must be valid {}: {path}",
        language.name
    );
    fixture.write(path, source);
}

fn write_mixed_product(fixture: &Fixture) -> BTreeSet<String> {
    let mut expected = BTreeSet::new();
    for module in 0..8 {
        for (path, source) in [
            (
                format!("services/module_{module}/api.rs"),
                format!(
                    "// TODO validate account {module}\npub fn answer(value: i32) -> i32 {{\n    if value > {module} {{ value }} else {{ 0 }}\n}}\n"
                ),
            ),
            (
                format!("ui/module_{module}/view.ts"),
                format!(
                    "// FIXME migrate view {module}\nexport function render(value: number): number {{\n    return value + {module};\n}}\n"
                ),
            ),
            (
                format!("tools/module_{module}/filter.py"),
                format!(
                    "# HACK compatibility path {module}\ndef filter_value(value):\n    return value + {module}\n"
                ),
            ),
        ] {
            write_code(fixture, &path, &source);
            expected.insert(path);
        }
        for (path, source) in [
            (
                format!("content/module_{module}.json"),
                format!("{{\"note\":\"TODO translate product {module}\"}}\n"),
            ),
            (
                format!("docs/module_{module}.md"),
                format!("# Product {module}\nHACK describes the documented migration.\n"),
            ),
            (
                format!("scenes/module_{module}.tscn"),
                format!("[gd_scene format=3]\n[node name=\"Product{module}\" type=\"Node\"]\n"),
            ),
        ] {
            fixture.write(&path, &source);
            expected.insert(path);
        }
    }
    expected
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn mixed_product_inventory_survives_health_policy_changes() {
    let fixture = Fixture::new("mixed-product-health");
    let expected = write_mixed_product(&fixture);
    let base = [
        "--only",
        "tokens,complexity,markers",
        "--no-project-config",
        "--no-cache",
    ];
    let source = fixture.json(&base);
    assert_eq!(paths(&source), expected);
    assert_eq!(source["summary"]["files"], 48);
    assert_eq!(source["summary"]["source"]["files"], 24);
    assert_eq!(
        source["summary"]["markers"],
        json!({"TODO":8,"FIXME":8,"HACK":8})
    );

    let cases: &[(&[&str], Value)] = &[
        (
            &["--health-include", "json"],
            json!({"TODO":16,"FIXME":8,"HACK":8}),
        ),
        (
            &["--health-scope", "all"],
            json!({"TODO":16,"FIXME":8,"HACK":16}),
        ),
        (
            &[
                "--health-scope",
                "all",
                "--health-include",
                "json",
                "--health-exclude",
                "content/**",
                "--health-exclude",
                "services/module_0/**",
            ],
            json!({"TODO":7,"FIXME":8,"HACK":16}),
        ),
    ];
    for (options, markers) in cases {
        let report = fixture.json(&[base.as_slice(), *options].concat());
        assert_eq!(
            paths(&report),
            expected,
            "health flags must not filter inventory"
        );
        assert_eq!(inventory(&report), inventory(&source));
        assert_eq!(report["summary"]["source"]["files"], 24);
        assert_eq!(report["summary"]["markers"], *markers);
        for module in 0..8 {
            for path in [
                format!("content/module_{module}.json"),
                format!("docs/module_{module}.md"),
                format!("scenes/module_{module}.tscn"),
            ] {
                assert!(
                    file(&report, &path)["complexity"].is_null(),
                    "data is not code: {path}"
                );
            }
        }
        if options.contains(&"services/module_0/**") {
            let excluded = file(&report, "services/module_0/api.rs");
            assert!(excluded["complexity"].is_null());
            assert!(excluded["markers"].is_null());
            assert!(file(&report, "services/module_1/api.rs")["complexity"].is_object());
        }
    }
}

struct MarkerCase {
    path: &'static str,
    source: &'static str,
    comment_lines: [usize; 3],
}

const MARKER_CASES: &[MarkerCase] = &[
    MarkerCase {
        path: "rust/message.rs",
        source: "// TODO real\npub fn message() -> &'static str {\n    let text = \"TODO FIXME HACK\";\n    /* FIXME real */\n    // HACK real\n    text\n}\n",
        comment_lines: [1, 4, 5],
    },
    MarkerCase {
        path: "python/message.py",
        source: "# TODO real\ndef message():\n    \"\"\"TODO FIXME HACK\"\"\"\n    text = \"TODO FIXME HACK\"\n    # FIXME real\n    # HACK real\n    return text\n",
        comment_lines: [1, 5, 6],
    },
    MarkerCase {
        path: "javascript/message.js",
        source: "// TODO real\nexport function message() {\n    const text = `TODO ${\"FIXME\"} HACK`;\n    /* FIXME real */\n    // HACK real\n    return text;\n}\n",
        comment_lines: [1, 4, 5],
    },
    MarkerCase {
        path: "typescript/message.ts",
        source: "// TODO real\nexport function message(): string {\n    const text: string = \"TODO FIXME HACK\";\n    /* FIXME real */\n    // HACK real\n    return text;\n}\n",
        comment_lines: [1, 4, 5],
    },
    MarkerCase {
        path: "tsx/Message.tsx",
        source: "// TODO real\nexport function Message() {\n    const text = \"TODO FIXME HACK\";\n    /* FIXME real */\n    // HACK real\n    return <div>TODO FIXME HACK{text}</div>;\n}\n",
        comment_lines: [1, 4, 5],
    },
    MarkerCase {
        path: "go/message.go",
        source: "package message\n// TODO real\nfunc Message() string {\n    text := \"TODO FIXME HACK\"\n    /* FIXME real */\n    // HACK real\n    return text\n}\n",
        comment_lines: [2, 5, 6],
    },
    MarkerCase {
        path: "php/message.php",
        source: "<?php\n// TODO real\nfunction message() {\n    $text = \"TODO FIXME HACK\";\n    /* FIXME real */\n    // HACK real\n    return $text;\n}\n",
        comment_lines: [2, 5, 6],
    },
    MarkerCase {
        path: "csharp/Message.cs",
        source: "// TODO real\npublic static class Notice\n{\n    public static string Message() {\n        var text = \"TODO FIXME HACK\";\n        /* FIXME real */\n        // HACK real\n        return text;\n    }\n}\n",
        comment_lines: [1, 6, 7],
    },
    MarkerCase {
        path: "godot/message.gd",
        source: "extends Node\n# TODO real\nfunc message():\n    var text = \"TODO FIXME HACK\"\n    # FIXME real\n    # HACK real\n    return text\n",
        comment_lines: [2, 5, 6],
    },
    MarkerCase {
        path: "godot/message.gdshader",
        source: "shader_type canvas_item;\n// TODO real\nvoid fragment() {\n    float FIXME = 0.0;\n    /* FIXME real */\n    // HACK real\n    COLOR = vec4(FIXME);\n}\n",
        comment_lines: [2, 5, 6],
    },
];

fn without_real_comments(case: &MarkerCase) -> String {
    let mut source = String::new();
    for (index, line) in case.source.lines().enumerate() {
        if !case.comment_lines.contains(&(index + 1)) {
            source.push_str(line);
        }
        source.push('\n');
    }
    source
}

fn expected_occurrences(case: &MarkerCase) -> Vec<Value> {
    ["TODO", "FIXME", "HACK"]
        .into_iter()
        .zip(case.comment_lines)
        .map(|(marker, line)| {
            let source_line = case.source.lines().nth(line - 1).unwrap();
            let column = source_line[..source_line.find(marker).unwrap()]
                .chars()
                .count()
                + 1;
            json!({"marker": marker, "line": line, "column": column})
        })
        .collect()
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn polyglot_marker_cleanup_does_not_turn_strings_into_findings() {
    let fixture = Fixture::new("polyglot-marker-cleanup");
    for service in ["checkout", "billing", "delivery"] {
        for case in MARKER_CASES {
            write_code(&fixture, &format!("{service}/{}", case.path), case.source);
        }
    }
    let args = [
        "--only",
        "tokens,markers",
        "--no-project-config",
        "--no-cache",
    ];
    let before = fixture.json(&args);
    assert_eq!(before["summary"]["files"], 30);
    assert_eq!(
        before["summary"]["markers"],
        json!({"TODO":30,"FIXME":30,"HACK":30})
    );
    let marker_findings = &before["finding_catalog"]["findings"];
    assert_eq!(marker_findings.as_array().unwrap().len(), 90);
    for service in ["checkout", "billing", "delivery"] {
        for case in MARKER_CASES {
            let path = format!("{service}/{}", case.path);
            let occurrences: Vec<_> = file(&before, &path)["marker_occurrences"]
                .as_array().unwrap().iter()
                .map(|item| json!({"marker": item["marker"], "line": item["line"], "column": item["column"]}))
                .collect();
            assert_eq!(occurrences, expected_occurrences(case), "{path}");
        }
    }
    for case in MARKER_CASES {
        write_code(
            &fixture,
            &format!("billing/{}", case.path),
            &without_real_comments(case),
        );
    }
    let after = fixture.json(&args);
    assert_eq!(paths(&after), paths(&before));
    assert_eq!(
        after["summary"]["markers"],
        json!({"TODO":20,"FIXME":20,"HACK":20})
    );
    let findings = after["finding_catalog"]["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 60);
    assert!(findings.iter().all(|finding| {
        finding["kind"] == "marker"
            && !finding["primary_location"]["path"]
                .as_str()
                .unwrap()
                .starts_with("billing/")
    }));
    for case in MARKER_CASES {
        assert!(file(&after, &format!("billing/{}", case.path))["markers"].is_null());
    }
}

fn duplicate_corpus(report: &Value) -> BTreeSet<String> {
    let entries = report["duplicates"]["file_coverage"].as_array().unwrap();
    let identities: BTreeSet<_> = entries
        .iter()
        .map(|entry| entry["path"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(identities.len(), entries.len(), "duplicate corpus paths");
    identities
}

fn duplicate_family_contains(report: &Value, wanted: &[&str]) -> bool {
    report["duplicates"]["exact"]
        .as_array()
        .unwrap()
        .iter()
        .any(|group| {
            let present: BTreeSet<_> = group["instances"]
                .as_array()
                .unwrap()
                .iter()
                .map(|instance| instance["path"].as_str().unwrap())
                .collect();
            wanted.iter().all(|path| present.contains(path))
        })
}

fn write_support_modules(fixture: &Fixture, count: usize) -> BTreeSet<String> {
    (0..count)
        .map(|index| {
            let path = format!("support/value_{index}.rs");
            write_code(
                fixture,
                &path,
                &format!("pub fn value_{index}() -> usize {{ {index} }}\n"),
            );
            path
        })
        .collect()
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn deployed_assets_are_inventory_but_not_default_duplicate_evidence() {
    let fixture = Fixture::new("deployed-assets");
    let mut expected = write_support_modules(&fixture, 24);
    let mut body = String::from("export function checksum(value) {\n");
    for index in 0..24 {
        writeln!(body, "    value = value * 31 + {index};").unwrap();
    }
    body.push_str("    return value;\n}\n");
    for path in [
        "src/codec.js",
        "src/compat.js",
        "generated/codec.js",
        "public/codec.min.js",
        "public/codec.bundle.js",
        "static/js/main.a1b2c3.chunk.js",
    ] {
        let source = if path.starts_with("generated/") {
            format!("// Code generated by fixture. DO NOT EDIT.\n{body}")
        } else {
            body.clone()
        };
        write_code(&fixture, path, &source);
        expected.insert(path.to_owned());
    }
    let args = [
        "--only",
        "tokens,complexity,dup",
        "--no-project-config",
        "--no-cache",
    ];
    let ordinary = fixture.json(&args);
    assert_eq!(paths(&ordinary), expected);
    assert_eq!(ordinary["summary"]["files"], 30);
    let artifacts = [
        "public/codec.min.js",
        "public/codec.bundle.js",
        "static/js/main.a1b2c3.chunk.js",
    ];
    let default_corpus = expected
        .iter()
        .filter(|path| !artifacts.contains(&path.as_str()))
        .cloned()
        .collect();
    assert_eq!(duplicate_corpus(&ordinary), default_corpus);
    assert!(duplicate_family_contains(
        &ordinary,
        &["src/codec.js", "src/compat.js"]
    ));
    assert_eq!(
        file(&ordinary, "generated/codec.js")["skip_hint"],
        "generated"
    );
    for path in artifacts {
        assert!(file(&ordinary, path)["tokens"].as_u64().unwrap() > 0);
        assert_eq!(file(&ordinary, path)["symbols"]["functions"], 1);
    }

    let included = fixture.json(&[args.as_slice(), &["--dup-include-artifacts"]].concat());
    assert_eq!(inventory(&included), inventory(&ordinary));
    assert_eq!(duplicate_corpus(&included), expected);
    assert!(duplicate_family_contains(
        &included,
        &["src/codec.js", "public/codec.min.js"]
    ));
    assert_eq!(
        included["analysis_profile"]["duplication"]["artifact_policy"],
        "include"
    );

    let excluded = fixture.json(
        &[
            args.as_slice(),
            &[
                "--dup-include-artifacts",
                "--health-exclude",
                "generated/**",
            ],
        ]
        .concat(),
    );
    expected.remove("generated/codec.js");
    assert_eq!(duplicate_corpus(&excluded), expected);
    assert_eq!(inventory(&excluded), inventory(&ordinary));
    assert!(duplicate_family_contains(
        &excluded,
        &["src/codec.js", "public/codec.bundle.js"]
    ));
}

fn cache_scan(fixture: &Fixture, options: &[&str]) -> Value {
    fixture.json(&[&["--only", "tokens,complexity,markers"][..], options].concat())
}

fn assert_cache_matches_fresh(fixture: &Fixture, options: &[&str]) -> Value {
    let cached = cache_scan(fixture, options);
    let fresh = cache_scan(fixture, &[options, &["--no-cache"]].concat());
    assert_eq!(semantic_report(&cached), semantic_report(&fresh));
    assert_eq!(fresh["execution"]["cache_enabled"], false);
    cached
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn cache_tracks_content_deletion_and_analysis_policy_without_changing_answers() {
    let fixture = Fixture::new("cache-lifecycle");
    fixture.write("reposcout.toml", "markers = [\"TODO\"]\n");
    for index in 0..24 {
        write_code(
            &fixture,
            &format!("src/module_{index}.rs"),
            &format!(
                "// TODO audit module {index}\n// NOTE document module {index}\npub fn value_{index}() -> usize {{ {index} }}\n"
            ),
        );
        fixture.write(
            &format!("data/module_{index}.json"),
            &format!("{{\"note\":\"TODO setting {index}\"}}\n"),
        );
    }
    let cold = cache_scan(&fixture, &[]);
    assert_eq!(cold["summary"]["files"], 49);
    assert_eq!(cold["summary"]["markers"], json!({"TODO":24}));
    assert_eq!(count(&cold["execution"], "cache_hits"), 0);
    assert_eq!(cold["execution"]["cache_misses"], 49);
    let warm = assert_cache_matches_fresh(&fixture, &[]);
    assert_eq!(warm["execution"]["cache_hits"], 49);
    assert_eq!(semantic_report(&warm), semantic_report(&cold));
    assert!(fixture.cache_path().is_dir());
    assert!(!fixture.path().join(".reposcout").exists());

    write_code(
        &fixture,
        "src/module_0.rs",
        "// TODO audit module 0\n// NOTE document module 0\n// TODO new rule\n// NOTE new rule\npub fn value_0() -> usize { 100 }\n",
    );
    fixture.remove("src/module_1.rs");
    write_code(
        &fixture,
        "src/replacement.ts",
        "// TODO replacement\n// NOTE replacement\nexport const replacement = 1;\n",
    );
    let changed = assert_cache_matches_fresh(&fixture, &[]);
    assert_eq!(changed["execution"]["cache_hits"], 47);
    assert_eq!(changed["execution"]["cache_misses"], 2);
    assert_eq!(changed["summary"]["files"], 49);
    assert_eq!(changed["summary"]["markers"], json!({"TODO":25}));
    assert!(!paths(&changed).contains("src/module_1.rs"));
    assert!(paths(&changed).contains("src/replacement.ts"));

    let content = assert_cache_matches_fresh(&fixture, &["--health-include", "json"]);
    assert_eq!(inventory(&content), inventory(&changed));
    assert_eq!(content["summary"]["markers"], json!({"TODO":49}));
    fixture.write("reposcout.toml", "markers = [\"NOTE\"]\n");
    let new_markers = assert_cache_matches_fresh(&fixture, &["--health-include", "json"]);
    assert_eq!(new_markers["summary"]["markers"], json!({"NOTE":25}));
    fixture.command(&["cache", "clear", "."]).assert().success();
    let reset = cache_scan(&fixture, &["--health-include", "json"]);
    assert_eq!(count(&reset["execution"], "cache_hits"), 0);
    assert_eq!(reset["execution"]["cache_misses"], 49);
    assert_eq!(semantic_report(&reset), semantic_report(&new_markers));
    assert!(!fixture.path().join(".reposcout").exists());
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn repeated_reports_exclude_the_exact_output_without_hiding_lookalike_files() {
    let fixture = Fixture::new("self-excluded-report");
    let mut expected = write_support_modules(&fixture, 24);
    for path in ["reports/reporta.json", "reports/reportb.json"] {
        fixture.write(path, "{\"note\":\"TODO preserve this sibling\"}\n");
        expected.insert(path.to_owned());
    }
    write_code(
        &fixture,
        "reports/report[ab].json.ts",
        "export const note = \"TODO string\";\n",
    );
    expected.insert("reports/report[ab].json.ts".to_owned());
    fixture.write(
        "reports/report[ab].json",
        "{\"note\":\"TODO obsolete report\"}\n",
    );
    let output = fixture.path().join("reports/report[ab].json");
    let args = [
        "--only",
        "tokens,markers",
        "--health-scope",
        "all",
        "--no-project-config",
        "--no-cache",
        "-f",
        "json",
        "--quiet",
        "--output",
        "./reports/../reports/report[ab].json",
    ];
    let mut previous = None;
    for _ in 0..3 {
        fixture.command(&args).assert().success();
        let report: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(paths(&report), expected);
        assert_eq!(report["summary"]["files"], 27);
        assert_eq!(report["summary"]["markers"], json!({"TODO":2}));
        if let Some(previous) = &previous {
            assert_eq!(semantic_report(&report), *previous);
        }
        previous = Some(semantic_report(&report));
    }
    write_code(
        &fixture,
        "support/value_0.rs",
        "// FIXME changed after the report\npub fn value_0() -> usize { 0 }\n",
    );
    fixture.command(&args).assert().success();
    let refreshed: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(paths(&refreshed), expected);
    assert_eq!(refreshed["summary"]["markers"], json!({"TODO":2,"FIXME":1}));
}

fn decision_source(branches: usize) -> String {
    let mut source = String::from("def decide(value):\n");
    for branch in 0..branches {
        writeln!(source, "    if value == {branch}:\n        return {branch}").unwrap();
    }
    source.push_str("    return -1\n");
    source
}

fn baseline_gate(fixture: &Fixture, baseline: &Path, code: i32) -> Value {
    let output = fixture
        .command(&[
            "--only",
            "complexity,markers",
            "--max-complexity",
            "1",
            "--no-project-config",
            "--no-cache",
            "--baseline",
            baseline.to_str().unwrap(),
            "--fail-on-regression",
            "-f",
            "json",
            "--quiet",
        ])
        .assert()
        .code(code)
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

fn assert_decision_complexity(report: &Value, path: &str, expected: u32) {
    let complexity = &file(report, path)["complexity"];
    assert_eq!(complexity["cyclomatic"], expected, "{path}");
    let functions = complexity["functions"].as_array().unwrap();
    assert_eq!(functions.len(), 1, "one authored function in {path}");
    assert_eq!(functions[0]["name"], "decide");
    assert_eq!(functions[0]["cyclomatic"], expected, "{path}");
    let findings: Vec<_> = report["finding_catalog"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| {
            finding["kind"] == "complexity" && finding["primary_location"]["path"] == path
        })
        .collect();
    assert_eq!(findings.len(), 1, "one over-threshold function in {path}");
    assert_eq!(
        findings[0]["metrics"]["cyclomatic"],
        json!(f64::from(expected))
    );
    assert_eq!(findings[0]["metrics"]["threshold"], json!(1.0));
}

fn assert_decision_delta(delta: &Value, path: &str, before: u32, after: u32) {
    let changes: Vec<_> = delta["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|change| {
            change["after"]["kind"] == "complexity"
                && change["after"]["primary_location"]["path"] == path
        })
        .collect();
    assert_eq!(changes.len(), 1, "one complexity change for {path}");
    assert_eq!(
        changes[0]["before"]["metrics"]["cyclomatic"],
        json!(f64::from(before))
    );
    assert_eq!(
        changes[0]["after"]["metrics"]["cyclomatic"],
        json!(f64::from(after))
    );
}

fn check_baseline_regression_transition(
    fixture: &Fixture,
    baseline: &Path,
    stable_marker: &str,
    new_route: &str,
) {
    write_code(fixture, "service/worsened.py", &decision_source(3));
    write_code(fixture, "service/improved.py", &decision_source(1));
    write_code(
        fixture,
        "service/resolved.py",
        "def old_route():\n    return 1\n",
    );
    write_code(
        fixture,
        "service/stable.py",
        &format!("\n\n{stable_marker}"),
    );
    write_code(
        fixture,
        "service/new.py",
        &format!("# FIXME validate the new route\n{new_route}"),
    );
    let regression = baseline_gate(fixture, baseline, 2);
    assert_decision_complexity(&regression, "service/worsened.py", 4);
    assert_decision_complexity(&regression, "service/improved.py", 2);
    let delta = &regression["baseline"]["finding_changes"];
    assert_eq!(delta["comparison"], "complete");
    assert_eq!(
        delta["counts"],
        json!({"new":1,"resolved":1,"worsened":1,"improved":1})
    );
    assert_eq!(delta["changes"].as_array().unwrap().len(), 4);
    assert_decision_delta(delta, "service/worsened.py", 3, 4);
    assert_decision_delta(delta, "service/improved.py", 3, 2);
    let actual: BTreeSet<_> = delta["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| {
            let finding = if change["after"].is_object() {
                &change["after"]
            } else {
                &change["before"]
            };
            (
                change["state"].as_str().unwrap(),
                finding["primary_location"]["path"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        actual,
        BTreeSet::from([
            ("new", "service/new.py"),
            ("resolved", "service/resolved.py"),
            ("worsened", "service/worsened.py"),
            ("improved", "service/improved.py"),
        ])
    );
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn baseline_journey_distinguishes_regressions_improvements_and_incompatible_policy() {
    let fixture = Fixture::new("baseline-refactoring");
    write_support_modules(&fixture, 24);
    write_code(&fixture, "service/worsened.py", &decision_source(2));
    write_code(&fixture, "service/improved.py", &decision_source(2));
    let old_marker = "# TODO retire this migration\ndef old_route():\n    return 1\n";
    let stable_marker = "# TODO keep this documented debt\ndef stable_route():\n    return 2\n";
    let new_route = "def new_route():\n    return 3\n";
    write_code(&fixture, "service/resolved.py", old_marker);
    write_code(&fixture, "service/stable.py", stable_marker);
    write_code(&fixture, "service/new.py", new_route);
    let baseline = tempfile::NamedTempFile::new().unwrap();
    fixture
        .command(&[
            "--only",
            "complexity,markers",
            "--max-complexity",
            "1",
            "--no-project-config",
            "--no-cache",
            "--baseline-ready",
            "-f",
            "json",
            "--quiet",
            "--output",
            baseline.path().to_str().unwrap(),
        ])
        .assert()
        .success();
    let unchanged = baseline_gate(&fixture, baseline.path(), 0);
    assert_eq!(unchanged["summary"]["files"], 29);
    // Each function has one entry path plus exactly two independent `if` decisions.
    assert_decision_complexity(&unchanged, "service/worsened.py", 3);
    assert_decision_complexity(&unchanged, "service/improved.py", 3);
    assert_eq!(
        unchanged["baseline"]["finding_changes"]["counts"],
        json!({"new":0,"resolved":0,"worsened":0,"improved":0})
    );

    check_baseline_regression_transition(&fixture, baseline.path(), stable_marker, new_route);

    write_code(&fixture, "service/worsened.py", &decision_source(1));
    write_code(&fixture, "service/new.py", new_route);
    let improvements = baseline_gate(&fixture, baseline.path(), 0);
    assert_decision_complexity(&improvements, "service/worsened.py", 2);
    assert_decision_complexity(&improvements, "service/improved.py", 2);
    assert_decision_delta(
        &improvements["baseline"]["finding_changes"],
        "service/worsened.py",
        3,
        2,
    );
    assert_decision_delta(
        &improvements["baseline"]["finding_changes"],
        "service/improved.py",
        3,
        2,
    );
    assert_eq!(improvements["baseline"]["regressed"], false);
    assert_eq!(
        improvements["baseline"]["finding_changes"]["counts"],
        json!({"new":0,"resolved":1,"worsened":0,"improved":2})
    );
    let failure = fixture
        .command(&[
            "--only",
            "complexity,markers",
            "--max-complexity",
            "1",
            "--no-project-config",
            "--no-cache",
            "--health-scope",
            "all",
            "--baseline",
            baseline.path().to_str().unwrap(),
            "--quiet",
        ])
        .assert()
        .code(1)
        .get_output()
        .stderr
        .clone();
    assert!(
        String::from_utf8(failure)
            .unwrap()
            .contains("baseline analyzer profile does not match")
    );
}
