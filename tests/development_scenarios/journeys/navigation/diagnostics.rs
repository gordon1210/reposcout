use super::super::support::{Journey, Step};
use super::{array, assert_source_bundle, sha256, text};
use crate::support::Fixture;
use reposcout::metrics::tokens::TokenCounter;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

struct DiagnosticTask {
    scope: &'static str,
    log: &'static str,
    initial_context: &'static str,
    expanded_context: &'static str,
}

struct TriageEvidence {
    context: Value,
    locations: BTreeSet<(String, u64)>,
    plans: Vec<Value>,
    source: Value,
}

fn bounded_json(step: &Step) -> Value {
    let report = step.stdout_json();
    let rendered = std::str::from_utf8(step.stdout_bytes()).unwrap();
    let counter = TokenCounter::new(text(&report["encoding"])).unwrap();
    assert!(
        rendered.len() <= 16_384,
        "raw response exceeds requested bytes"
    );
    assert!(
        counter.count(rendered) <= 4_096,
        "raw response exceeds requested tokens"
    );
    report
}

fn diagnostic_context(journey: &mut Journey<'_>, task: &DiagnosticTask) -> Value {
    journey
        .step(
            "normalize the supplied compiler log within the task scope",
            &[
                task.scope,
                "--task-diagnostics",
                task.log,
                "--task-diagnostics-format",
                "rustc-json",
                "--context-budget",
                "1",
                "--profile",
                "agent",
                "--summary",
                "--no-project-config",
                "--no-cache",
                "-f",
                "json",
                "--quiet",
            ],
            0,
        )
        .stdout_json()
}

fn plan_diagnostic_locations(
    journey: &mut Journey<'_>,
    locations: &BTreeSet<(String, u64)>,
    context: &str,
    previous_files: &[Value],
) -> Value {
    let mut arguments = vec!["plan".to_owned(), ".".to_owned()];
    for (path, line) in locations {
        arguments.extend(["--line".to_owned(), path.clone(), line.to_string()]);
    }
    for file in previous_files {
        arguments.extend([
            "--expect-hash".to_owned(),
            text(&file["path"]).to_owned(),
            text(&file["sha256"]).to_owned(),
        ]);
    }
    arguments.extend(
        [
            "--context-budget",
            context,
            "--max-plan-files",
            "8",
            "--max-definitions",
            "16",
            "--budget",
            "4096",
            "--max-output-bytes",
            "16384",
            "--no-project-config",
            "--no-cache",
            "-f",
            "json",
            "--quiet",
        ]
        .map(str::to_owned),
    );
    bounded_json(&journey.step(
        "plan actual resolved diagnostic positions",
        &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        0,
    ))
}

fn read_diagnostic_plan(journey: &mut Journey<'_>, plan: &Value) -> Value {
    let mut arguments = vec!["read".to_owned(), ".".to_owned()];
    let files = array(&plan["files"]);
    let mut hashes = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for definition in array(&plan["selected"]) {
        let file = files
            .iter()
            .find(|file| file["id"] == definition["file"])
            .unwrap();
        let path = text(&file["path"]);
        let name = text(&definition["name"]);
        assert!(
            seen.insert((path, name)),
            "duplicate observed source target"
        );
        let hash = text(&file["sha256"]);
        if let Some(previous) = hashes.insert(path, hash) {
            assert_eq!(previous, hash);
        }
        arguments.extend(["--symbol".to_owned(), path.to_owned(), name.to_owned()]);
    }
    for (path, hash) in hashes {
        arguments.extend(["--expect-hash".to_owned(), path.to_owned(), hash.to_owned()]);
    }
    arguments.extend(
        [
            "--budget",
            "4096",
            "--max-output-bytes",
            "16384",
            "--no-project-config",
            "--no-cache",
            "-f",
            "json",
            "--quiet",
        ]
        .map(str::to_owned),
    );
    bounded_json(&journey.step(
        "read only observed selected definitions with their captured hashes",
        &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        0,
    ))
}

fn triage_diagnostics(journey: &mut Journey<'_>, task: &DiagnosticTask) -> TriageEvidence {
    let context = diagnostic_context(journey, task);
    let locations = array(&context["context"]["task_evidence"]["diagnostics"])
        .iter()
        .filter(|record| record["status"] == "resolved")
        .map(|record| {
            (
                text(&record["path"]).to_owned(),
                record["line"].as_u64().unwrap(),
            )
        })
        .collect::<BTreeSet<_>>();
    assert!(
        !locations.is_empty(),
        "the supplied log yielded no source positions"
    );
    let mut plans = vec![plan_diagnostic_locations(
        journey,
        &locations,
        task.initial_context,
        &[],
    )];
    if array(&plans[0]["omissions"]).iter().any(|omission| {
        matches!(
            text(&omission["reason"]),
            "oversized-definition" | "token-budget"
        )
    }) {
        plans.push(plan_diagnostic_locations(
            journey,
            &locations,
            task.expanded_context,
            array(&plans[0]["files"]),
        ));
    }
    let source = read_diagnostic_plan(journey, plans.last().unwrap());
    TriageEvidence {
        context,
        locations,
        plans,
        source,
    }
}

// The driver above knows only the log/scope and observed locations. Expected file roles stay here.
const CODEC_PATH: &str = "services/checkout/src/codec.rs";
const LIMITS_PATH: &str = "services/checkout/src/limits.rs";
const PACKET: &str = "pub struct Packet { pub code: u16 }";
const DECISION: &str = "pub enum Decision { Accept, Retry, Reject }";
const LIMIT: &str = "pub struct Limit { pub maximum: u32 }";
const ALLOWED: &str = "pub fn allowed(limit: Limit) -> bool { limit.maximum <= 100 }";
const DECODE: &str = r"pub fn decode(packet: Packet) -> Decision {
    match packet.code {
        0 => Decision::Accept,
        1 => Decision::Accept,
        2 => Decision::Retry,
        3 => Decision::Reject,
        4 => Decision::Retry,
        5 => Decision::Accept,
        6 => Decision::Reject,
        7 => Decision::Retry,
        8 => Decision::Accept,
        9 => Decision::Reject,
        10 => Decision::Retry,
        11 => Decision::Accept,
        12 => Decision::Reject,
        13 => Decision::Retry,
        14 => Decision::Accept,
        15 => Decision::Reject,
        16 => Decision::Accept,
        17 => Decision::Retry,
        _ => Decision::Reject,
    }
}";

struct DiagnosticOracle {
    files: BTreeMap<String, String>,
    definitions: BTreeMap<(String, String), String>,
    records: Vec<String>,
}

fn compiler_record(path: &str, line: u64, severity: &str, code: &str, message: &str) -> String {
    json!({
        "reason": "compiler-message",
        "message": {
            "message": message,
            "level": severity,
            "code": {"code": code},
            "rendered": "PRIVATE_RENDERED_DIAGNOSTIC_BODY",
            "spans": [
                {"file_name": path, "is_primary": true, "line_start": line,
                 "column_start": 1, "line_end": line, "column_end": 10},
                {"file_name": "services/checkout/src/decoy.rs", "is_primary": false,
                 "line_start": 1, "column_start": 1, "line_end": 1, "column_end": 10},
            ],
        },
    })
    .to_string()
}

fn diagnostic_fixture() -> (Fixture, DiagnosticOracle) {
    let fixture = Fixture::new("journey-compiler-diagnostic-triage");
    let files: BTreeMap<_, _> = [
        ("Cargo.toml", "[workspace]\nmembers=['services/checkout', 'services/catalog']\nresolver='3'\n".to_owned()),
        ("services/checkout/Cargo.toml", "[package]\nname='checkout'\nversion='0.1.0'\nedition='2024'\n".to_owned()),
        ("services/checkout/src/lib.rs", "pub mod codec;\npub mod limits;\npub mod order;\npub mod decoy;\n".to_owned()),
        (CODEC_PATH, format!("{PACKET}\n{DECISION}\n{DECODE}\npub fn unrelated_codec() -> u32 {{ 701 }}\n")),
        (LIMITS_PATH, format!("{LIMIT}\n{ALLOWED}\npub fn unrelated_limits() -> u32 {{ let weights = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120]; weights[0] }}\n")),
        ("services/checkout/src/order.rs", "pub fn submit() -> bool { crate::limits::allowed(crate::limits::Limit { maximum: 10 }) }\n".to_owned()),
        ("services/checkout/src/decoy.rs", "pub fn decode() -> u32 { 913 }\n".to_owned()),
        ("services/checkout/src/old/codec.rs", "pub fn decode() -> u32 { 812 }\n".to_owned()),
        ("services/checkout/tests/codec.rs", "fn decode_works() { assert_eq!(2, 2); }\n".to_owned()),
        ("services/catalog/Cargo.toml", "[package]\nname='catalog'\nversion='0.1.0'\nedition='2024'\n".to_owned()),
        ("services/catalog/src/lib.rs", "pub mod codec;\n".to_owned()),
        ("services/catalog/src/codec.rs", "pub fn decode() -> u32 { 611 }\n".to_owned()),
        ("docs/diagnostics.md", "A compiler path is an exact source identity; matching names are insufficient.\n".to_owned()),
    ].into_iter().map(|(path, source)| (path.to_owned(), source)).collect();
    for (path, source) in &files {
        fixture.write(path, source);
    }
    let error = compiler_record(
        CODEC_PATH,
        8,
        "error",
        "E0308",
        "wrong decision for retry status",
    );
    let records = vec![
        error.clone(),
        compiler_record(
            LIMITS_PATH,
            2,
            "warning",
            "unused_comparisons",
            "check the configured limit",
        ),
        error,
        compiler_record(
            "services/catalog/src/codec.rs",
            1,
            "error",
            "E0425",
            "other package fails independently",
        ),
        compiler_record(
            "codec.rs",
            1,
            "error",
            "E0433",
            "ambiguous shorthand from an old log",
        ),
        json!({"reason": "compiler-artifact", "target": {"name": "checkout"}}).to_string(),
    ];
    let definitions = [
        (CODEC_PATH, "Packet", PACKET),
        (CODEC_PATH, "Decision", DECISION),
        (CODEC_PATH, "decode", DECODE),
        (LIMITS_PATH, "Limit", LIMIT),
        (LIMITS_PATH, "allowed", ALLOWED),
    ]
    .into_iter()
    .map(|(path, name, source)| ((path.to_owned(), name.to_owned()), source.to_owned()))
    .collect();
    (
        fixture,
        DiagnosticOracle {
            files,
            definitions,
            records,
        },
    )
}

fn assert_normalized_diagnostics(context: &Value) {
    let evidence = &context["task_evidence"];
    assert_eq!(evidence["format"], "rustc-json");
    assert_eq!(
        evidence["status"], "complete",
        "parsing coverage is separate from path resolution"
    );
    // The scoped scan has no catalog inventory: neither this sibling path nor the shorthand
    // establishes a source identity. Both stay gaps without a speculative second walk.
    for (key, count) in [
        ("parsed_records", 5),
        ("deduplicated_records", 1),
        ("resolved_records", 2),
        ("out_of_scope_records", 0),
        ("unresolved_records", 2),
        ("ignored_records", 1),
        ("parse_errors", 0),
        ("omitted_records", 0),
        ("omitted_details", 0),
    ] {
        assert_eq!(evidence[key], count, "{key}: {evidence}");
    }
    assert_eq!(evidence["input_truncated"], false);
    assert_eq!(evidence["records_truncated"], false);
    assert_eq!(evidence["omitted_records_exact"], true);
    let records = array(&evidence["diagnostics"]);
    assert_eq!(records.len(), 4);
    let mut identities = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for record in records {
        assert!(
            ids.insert(text(&record["id"])),
            "duplicate normalized diagnostic ID"
        );
        assert_eq!(record["confidence"], "high");
        assert_eq!(record["column"], 1);
        let identity = if record["status"] == "resolved" {
            assert!(record.get("original_path").is_none());
            assert!(record.get("reason").is_none());
            text(&record["path"])
        } else {
            assert_eq!(record["status"], "unresolved");
            assert_eq!(record["reason"], "not-in-inventory");
            assert!(record.get("path").is_none());
            text(&record["original_path"])
        };
        assert!(
            identities.insert((
                identity,
                record["line"].as_u64().unwrap(),
                text(&record["severity"]),
                text(&record["status"]),
                text(&record["code"])
            )),
            "duplicate normalized diagnostic identity"
        );
    }
    assert_eq!(
        identities,
        BTreeSet::from([
            (CODEC_PATH, 8, "error", "resolved", "E0308"),
            (LIMITS_PATH, 2, "warning", "resolved", "unused_comparisons"),
            (
                "services/catalog/src/codec.rs",
                1,
                "error",
                "unresolved",
                "E0425"
            ),
            ("codec.rs", 1, "error", "unresolved", "E0433"),
        ])
    );
    assert!(
        !evidence
            .to_string()
            .contains("PRIVATE_RENDERED_DIAGNOSTIC_BODY")
    );
}

fn assert_diagnostic_outlines(context: &Value) {
    assert_eq!(context["budget_tokens"], 1);
    assert_eq!(context["seed_files"], 2);
    let outlines = array(&context["outline_only"]);
    assert_eq!(outlines.len(), 2);
    let mut paths = BTreeSet::new();
    for outline in outlines {
        let path = text(&outline["path"]);
        assert!(paths.insert(path), "duplicate diagnostic outline");
        assert!(outline["source_tokens"].as_u64().unwrap() > 1);
        let record = array(&context["task_evidence"]["diagnostics"])
            .iter()
            .find(|record| record["status"] == "resolved" && record["path"] == path)
            .unwrap();
        assert!(
            array(&outline["evidence"]).iter().any(|evidence| {
                evidence["confidence"] == "high"
                    && array(&evidence["diagnostic_ids"])
                        .iter()
                        .any(|id| id == &record["id"])
            }),
            "outline lost the diagnostic that justified it: {outline}"
        );
    }
    assert_eq!(paths, BTreeSet::from([CODEC_PATH, LIMITS_PATH]));
}

fn assert_complete_plan(plan: &Value, oracle: &DiagnosticOracle) {
    for key in [
        "unresolved_seeds",
        "ambiguous_seeds",
        "unavailable_seeds",
        "unavailable_files",
        "planning_omitted_definitions",
        "output_omitted_files",
        "omitted_definitions",
        "output_omitted",
        "omitted_details",
    ] {
        assert_eq!(plan[key], 0, "{key}: {plan}");
    }
    assert_eq!(plan["discovery_incomplete"], false);
    assert_eq!(plan["selected_files"], 2);
    assert_eq!(array(&plan["selected"]).len(), 5);
    assert!(plan.get("source").is_none());
    let mut actual = BTreeSet::new();
    for definition in array(&plan["selected"]) {
        let file = array(&plan["files"])
            .iter()
            .find(|file| file["id"] == definition["file"])
            .unwrap();
        let path = text(&file["path"]);
        let name = text(&definition["name"]);
        assert_eq!(file["sha256"], sha256(&oracle.files[path]));
        assert!(
            actual.insert((path.to_owned(), name.to_owned())),
            "duplicate planned definition"
        );
        if matches!(name, "decode" | "allowed") {
            assert_eq!(definition["role"], "direct");
            assert!(
                array(&definition["environment_gaps"])
                    .iter()
                    .any(|gap| gap == "body-dependencies-not-expanded")
            );
        } else {
            assert_eq!(definition["role"], "environment");
        }
    }
    assert_eq!(actual, oracle.definitions.keys().cloned().collect());
    let counter = TokenCounter::new(text(&plan["encoding"])).unwrap();
    let expected_tokens: usize = oracle
        .definitions
        .values()
        .map(|source| counter.count(source))
        .sum();
    assert_eq!(plan["selected_tokens"], expected_tokens);
}

fn assert_triage(evidence: &TriageEvidence, oracle: &DiagnosticOracle) {
    let context = &evidence.context["context"];
    assert_normalized_diagnostics(context);
    assert_diagnostic_outlines(context);
    assert_eq!(
        evidence.locations,
        BTreeSet::from([(CODEC_PATH.to_owned(), 8), (LIMITS_PATH.to_owned(), 2)])
    );
    assert_eq!(
        evidence.plans.len(),
        2,
        "explicit source-budget omission must trigger the bounded retry"
    );
    let small = &evidence.plans[0];
    assert_eq!(small["context_budget"], 64);
    assert!(small["selected_tokens"].as_u64().unwrap() <= 64);
    assert!(array(&small["omissions"]).iter().any(|omission| {
        omission["path"] == CODEC_PATH
            && omission["name"] == "decode"
            && omission["reason"] == "oversized-definition"
            && omission["explicit"] == true
    }));
    assert!(
        array(&small["selected"])
            .iter()
            .any(|definition| definition["name"] == "allowed")
    );
    let full = &evidence.plans[1];
    assert_eq!(full["context_budget"], 2000);
    assert!(full["selected_tokens"].as_u64().unwrap() > 64);
    assert_complete_plan(full, oracle);
    assert_source_bundle(&evidence.source, &oracle.files, &oracle.definitions);
    for report in [&evidence.context, small, full] {
        assert!(
            !report.to_string().contains("17 => Decision::Retry"),
            "metadata phase leaked a body"
        );
    }
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn compiler_log_drives_scoped_identity_checked_reads_with_honest_gaps() {
    let task = DiagnosticTask {
        scope: "services/checkout",
        log: "artifacts/compiler.jsonl",
        initial_context: "64",
        expanded_context: "2000",
    };
    let (fixture, oracle) = diagnostic_fixture();
    let original = format!("{}\n", oracle.records.join("\n"));
    let reverse = format!(
        "{}\n",
        oracle
            .records
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mut first_diagnostics = None;
    for log in [&original, &reverse] {
        fixture.write(task.log, log);
        let mut journey = Journey::new(&fixture);
        let evidence = triage_diagnostics(&mut journey, &task);
        assert_triage(&evidence, &oracle);
        let diagnostics = &evidence.context["context"]["task_evidence"];
        assert_eq!(diagnostics["bytes_read"], log.len());
        if let Some(previous) = &first_diagnostics {
            assert_eq!(
                diagnostics, previous,
                "log order must not change normalized evidence or IDs"
            );
        } else {
            first_diagnostics = Some(diagnostics.clone());
        }
    }
}
