use super::support::Journey;
use crate::support::Fixture;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

mod diagnostics;

fn array(value: &Value) -> &[Value] {
    value.as_array().expect("expected a report array")
}

fn text(value: &Value) -> &str {
    value.as_str().expect("expected report text")
}

#[derive(Clone)]
struct ObservedTarget {
    path: String,
    symbol: String,
    hash: String,
}

impl ObservedTarget {
    fn from_read(target: &Value) -> Self {
        assert_eq!(target["selector"]["kind"], "symbol");
        assert_eq!(target["snapshot"]["kind"], "worktree");
        Self {
            path: text(&target["path"]).to_owned(),
            symbol: text(&target["selector"]["value"]).to_owned(),
            hash: text(&target["expected_hash"]).to_owned(),
        }
    }
}

struct DebugTask {
    symptom: &'static str,
    context_tokens: &'static str,
    initial_depth: u8,
    maximum_depth: u8,
}

struct Discovery {
    report: Value,
    target: ObservedTarget,
    source: Value,
}

struct DebugEvidence {
    recheck: Value,
    refreshes: usize,
    current: Discovery,
    traversal: Vec<Value>,
    plan: Value,
}

fn read_observed(journey: &mut Journey<'_>, target: &ObservedTarget, label: &str) -> Value {
    journey
        .step(
            label,
            &[
                "read",
                ".",
                "--symbol",
                &target.path,
                &target.symbol,
                "--expect-hash",
                &target.path,
                &target.hash,
                "--budget",
                "32768",
                "--max-output-bytes",
                "262144",
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

fn discover_symptom(journey: &mut Journey<'_>, task: &DebugTask) -> Discovery {
    let report = journey
        .step(
            "search original symptom",
            &[
                "find",
                task.symptom,
                ".",
                "--budget",
                "32768",
                "--max-output-bytes",
                "262144",
                "--no-project-config",
                "--no-cache",
                "-f",
                "json",
                "--quiet",
            ],
            0,
        )
        .stdout_json();
    let hits = array(&report["hits"]);
    assert_eq!(
        hits.len(),
        1,
        "this task requires an unambiguous search: {report}"
    );
    let target = ObservedTarget::from_read(&hits[0]["read"]);
    let source = read_observed(journey, &target, "read discovered definition");
    Discovery {
        report,
        target,
        source,
    }
}

fn incoming_callers(journey: &mut Journey<'_>, target: &ObservedTarget, depth: u8) -> Value {
    journey
        .step(
            "follow observed caller identity",
            &[
                "consumers",
                ".",
                "--symbol",
                &target.path,
                &target.symbol,
                "--expect-hash",
                &target.path,
                &target.hash,
                "--direction",
                "incoming",
                "--depth",
                &depth.to_string(),
                "--budget",
                "32768",
                "--max-output-bytes",
                "262144",
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

fn plan_observed(
    journey: &mut Journey<'_>,
    task: &DebugTask,
    seed: &ObservedTarget,
    callers: &Value,
) -> Value {
    let mut targets = vec![seed.clone()];
    targets.extend(
        array(&callers["hits"])
            .iter()
            .map(|hit| ObservedTarget::from_read(&hit["read"])),
    );
    let mut identities = BTreeSet::new();
    let mut hashes = BTreeMap::new();
    let mut arguments = vec!["plan".to_owned(), ".".to_owned()];
    for target in &targets {
        assert!(
            identities.insert((&target.path, &target.symbol)),
            "duplicated observed definition"
        );
        if let Some(previous) = hashes.insert(&target.path, &target.hash) {
            assert_eq!(
                previous, &target.hash,
                "one file has inconsistent identities"
            );
        }
        arguments.extend([
            "--symbol".to_owned(),
            target.path.clone(),
            target.symbol.clone(),
        ]);
    }
    for (path, hash) in hashes {
        arguments.extend(["--expect-hash".to_owned(), path.clone(), hash.clone()]);
    }
    arguments.extend(
        [
            "--source",
            "--context-budget",
            task.context_tokens,
            "--max-plan-files",
            "16",
            "--max-definitions",
            "32",
            "--budget",
            "32768",
            "--max-output-bytes",
            "262144",
            "--no-project-config",
            "--no-cache",
            "-f",
            "json",
            "--quiet",
        ]
        .map(str::to_owned),
    );
    journey
        .step(
            "assemble source from observed seed and caller targets",
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
            0,
        )
        .stdout_json()
}

fn resume_debugging(
    journey: &mut Journey<'_>,
    task: &DebugTask,
    previous: &Discovery,
) -> DebugEvidence {
    let recheck = read_observed(
        journey,
        &previous.target,
        "recheck retained source identity",
    );
    assert_eq!(array(&recheck["results"]).len(), 1);
    let (refreshes, current) = match text(&recheck["results"][0]["status"]) {
        "stale" => (1, discover_symptom(journey, task)),
        "complete" => (
            0,
            Discovery {
                report: previous.report.clone(),
                target: previous.target.clone(),
                source: recheck.clone(),
            },
        ),
        status => panic!("cannot continue from source state {status}: {recheck}"),
    };
    let mut traversal = vec![incoming_callers(
        journey,
        &current.target,
        task.initial_depth,
    )];
    if traversal[0]["depth_omitted"].as_u64().unwrap() > 0 {
        assert!(task.maximum_depth > task.initial_depth);
        traversal.push(incoming_callers(
            journey,
            &current.target,
            task.maximum_depth,
        ));
    }
    let plan = plan_observed(journey, task, &current.target, traversal.last().unwrap());
    DebugEvidence {
        recheck,
        refreshes,
        current,
        traversal,
        plan,
    }
}

// Fixture roles and their authored oracle never cross the driver boundary above.
struct RetryLayout {
    charge: &'static str,
    checkout: &'static str,
    service: &'static str,
    entry: &'static str,
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CallSite {
    caller: (String, String),
    callee: (String, String),
    line: u64,
    spelling: String,
    syntax: String,
}

struct RetryOracle {
    files: BTreeMap<String, String>,
    seed_path: String,
    checkout_path: String,
    old_seed: String,
    new_seed: String,
    definitions: BTreeMap<(String, String), String>,
    calls: BTreeSet<CallSite>,
    callers: BTreeSet<(String, String, u64)>,
}

const ATTEMPT: &str = "pub struct Attempt { pub ordinal: u32, pub already_settled: bool }";
const INITIAL_DISPATCH: &str = r"pub fn dispatch_request(attempt: Attempt) -> bool {
    let duplicate_payment_retry = attempt.ordinal < 3;
    duplicate_payment_retry
}";
const FIXED_DISPATCH: &str = r"pub fn dispatch_request(attempt: Attempt) -> bool {
    let duplicate_payment_retry = attempt.ordinal < 3 && !attempt.already_settled;
    duplicate_payment_retry
}";

fn module_path(module: &str) -> String {
    format!("src/{module}.rs")
}

fn retry_fixture(layout: &RetryLayout) -> (Fixture, RetryOracle) {
    let fixture = Fixture::new("journey-symptom-to-callers");
    let seed_path = module_path(layout.charge);
    let old_seed = format!("{ATTEMPT}\n\n{INITIAL_DISPATCH}\n");
    let new_seed =
        format!("pub fn request_timeout() -> u32 {{ 30 }}\n\n{ATTEMPT}\n\n{FIXED_DISPATCH}\n");
    let mut files = BTreeMap::from([
        (seed_path.clone(), old_seed.clone()),
        ("Cargo.toml".to_owned(), "[package]\nname='retry-service'\nversion='0.1.0'\nedition='2024'\n".to_owned()),
        ("src/analytics.rs".to_owned(), "pub fn dispatch_request() -> bool { false }\npub fn summarize() -> bool { dispatch_request() }\n".to_owned()),
        ("src/decoys.rs".to_owned(), "pub trait Sender { fn dispatch_request(&self) -> bool; }\npub fn through_object(sender: &impl Sender) -> bool { sender.dispatch_request() }\n".to_owned()),
        ("src/support.rs".to_owned(), "pub fn retry_request() -> u32 { 3 }\npub fn duplicate_documents() -> bool { false }\n".to_owned()),
        ("docs/runbook.md".to_owned(), "Investigate duplicate payment retry before changing the service.\n".to_owned()),
        ("scripts/integration.py".to_owned(), "def invoke(sender):\n    return sender.dispatch_request()\n".to_owned()),
    ]);
    let mut lib = String::new();
    for module in [
        layout.charge,
        layout.checkout,
        layout.service,
        layout.entry,
        "analytics",
        "decoys",
        "support",
    ] {
        writeln!(lib, "pub mod {module};").unwrap();
    }
    files.insert("src/lib.rs".to_owned(), lib);
    let mut oracle = RetryOracle {
        files,
        seed_path: seed_path.clone(),
        checkout_path: module_path(layout.checkout),
        old_seed,
        new_seed,
        definitions: BTreeMap::from([
            (
                (seed_path.clone(), "Attempt".to_owned()),
                ATTEMPT.to_owned(),
            ),
            (
                (seed_path, "dispatch_request".to_owned()),
                FIXED_DISPATCH.to_owned(),
            ),
        ]),
        calls: BTreeSet::new(),
        callers: BTreeSet::new(),
    };
    write_callers(layout, &mut oracle);
    for (path, source) in &oracle.files {
        fixture.write(path, source);
    }
    (fixture, oracle)
}

fn write_callers(layout: &RetryLayout, oracle: &mut RetryOracle) {
    let charge = layout.charge;
    let checkout = layout.checkout;
    let service = layout.service;
    let submit = "pub fn submit(attempt: Attempt) -> bool { charge(attempt) }";
    let once = format!(
        "pub fn submit_once(attempt: Attempt) -> bool {{ crate::{charge}::dispatch_request(attempt) }}"
    );
    let place = format!(
        "pub fn place(attempt: crate::{charge}::Attempt) -> bool {{ crate::{checkout}::submit(attempt) }}"
    );
    let handle = format!(
        "pub fn handle(attempt: crate::{charge}::Attempt) -> bool {{ crate::{service}::place(attempt) }}"
    );
    oracle.files.insert(oracle.checkout_path.clone(), format!(
        "use crate::{charge}::dispatch_request as charge;\nuse crate::{charge}::Attempt;\n{submit}\n{once}\npub fn ignored_callback(charge: fn(Attempt) -> bool, attempt: Attempt) -> bool {{ charge(attempt) }}\n"
    ));
    oracle
        .files
        .insert(module_path(service), format!("{place}\n"));
    oracle
        .files
        .insert(module_path(layout.entry), format!("{handle}\n"));
    for (module, name, depth, source, target_module, target, line, spelling, syntax) in [
        (
            checkout,
            "submit",
            1,
            submit.to_owned(),
            charge,
            "dispatch_request",
            3,
            "charge(attempt)".to_owned(),
            "imported-binding",
        ),
        (
            checkout,
            "submit_once",
            1,
            once,
            charge,
            "dispatch_request",
            4,
            format!("crate::{charge}::dispatch_request(attempt)"),
            "module-qualified",
        ),
        (
            service,
            "place",
            2,
            place,
            checkout,
            "submit",
            1,
            format!("crate::{checkout}::submit(attempt)"),
            "module-qualified",
        ),
        (
            layout.entry,
            "handle",
            3,
            handle,
            service,
            "place",
            1,
            format!("crate::{service}::place(attempt)"),
            "module-qualified",
        ),
    ] {
        let path = module_path(module);
        oracle
            .definitions
            .insert((path.clone(), name.to_owned()), source);
        oracle
            .callers
            .insert((path.clone(), name.to_owned(), depth));
        oracle.calls.insert(CallSite {
            caller: (path, name.to_owned()),
            callee: (module_path(target_module), target.to_owned()),
            line,
            spelling,
            syntax: syntax.to_owned(),
        });
    }
}

fn sha256(source: &str) -> String {
    let mut hash = String::with_capacity(64);
    for byte in Sha256::digest(source.as_bytes()) {
        write!(hash, "{byte:02x}").unwrap();
    }
    hash
}

fn assert_search(report: &Value, oracle: &RetryOracle, expected_hash: &str) {
    assert_eq!(report["total_matches"], 1);
    assert_eq!(report["returned_matches"], 1);
    assert_eq!(array(&report["hits"]).len(), 1);
    for key in ["limit_omitted", "budget_omitted"] {
        assert_eq!(report[key], 0, "{key}: {report}");
    }
    let coverage = &report["coverage"];
    assert_eq!(coverage["files_total"], 11);
    assert_eq!(coverage["files_inspected"], 9);
    assert_eq!(coverage["unsupported_files"], 2);
    for key in [
        "unavailable_files",
        "parse_error_files",
        "field_truncated_files",
        "definitions_omitted",
    ] {
        assert_eq!(coverage[key], 0, "{key}: {report}");
    }
    let hit = &report["hits"][0];
    assert_eq!(hit["path"], oracle.seed_path);
    assert_eq!(hit["name"], "dispatch_request");
    assert_eq!(hit["sha256"], expected_hash);
    assert_eq!(hit["read"]["expected_hash"], expected_hash);
    assert!(!report.to_string().contains("attempt.ordinal < 3"));
}

fn assert_source_bundle(
    report: &Value,
    files: &BTreeMap<String, String>,
    expected: &BTreeMap<(String, String), String>,
) {
    assert!(!expected.is_empty());
    assert_eq!(report["requested_targets"], expected.len());
    assert_eq!(report["omitted_targets"], 0);
    let file_records = array(&report["files"]);
    let file_ids: BTreeSet<_> = file_records
        .iter()
        .map(|file| file["id"].as_u64().unwrap())
        .collect();
    assert_eq!(
        file_ids.len(),
        file_records.len(),
        "duplicate source file IDs"
    );
    for file in file_records {
        assert_eq!(file["snapshot"]["kind"], "worktree");
        let path = text(&file["path"]);
        assert_eq!(file["sha256"], sha256(&files[path]));
    }
    let sources = array(&report["sources"]);
    let source_ids: BTreeSet<_> = sources
        .iter()
        .map(|source| source["id"].as_u64().unwrap())
        .collect();
    assert_eq!(
        source_ids.len(),
        sources.len(),
        "duplicate source chunk IDs"
    );
    let mut actual = BTreeMap::new();
    for result in array(&report["results"]) {
        assert_eq!(result["status"], "complete", "{result}");
        let file = file_records
            .iter()
            .find(|file| file["id"] == result["file"])
            .unwrap();
        let source = sources
            .iter()
            .find(|source| source["id"] == result["source"])
            .unwrap();
        assert_eq!(source["file"], file["id"]);
        let identity = (
            text(&file["path"]).to_owned(),
            text(&result["definition"]["name"]).to_owned(),
        );
        assert!(
            actual
                .insert(identity, text(&source["content"]).to_owned())
                .is_none(),
            "duplicate source definition"
        );
    }
    assert_eq!(
        &actual, expected,
        "complete definition bodies, not substrings"
    );
    assert_eq!(sources.len(), expected.len(), "no extra source chunks");
}

fn assert_call_coverage(report: &Value) {
    let coverage = &report["coverage"];
    assert_eq!(coverage["files_total"], 11);
    assert_eq!(coverage["files_available"], 8);
    assert_eq!(coverage["files_unsupported"], 1);
    assert_eq!(coverage["files_unavailable"], 2);
    for key in [
        "files_parse_errors",
        "files_truncated",
        "declarations_omitted",
        "relations_omitted",
        "discovery_omitted",
        "unreadable_files",
        "oversized_files",
        "walker_errors",
    ] {
        assert_eq!(coverage[key], 0, "{key}: {report}");
    }
    for key in [
        "scan_truncated",
        "deadline_reached",
        "discovery_omitted_count_incomplete",
    ] {
        assert_eq!(coverage[key], false, "{key}: {report}");
    }
    for key in [
        "path_omitted",
        "limit_omitted",
        "budget_omitted",
        "unresolved_omitted",
    ] {
        assert_eq!(report[key], 0, "{key}: {report}");
    }
    assert_eq!(coverage["resolution"]["omitted"], 0);
    assert_eq!(
        coverage["resolution"]["unresolved"],
        array(&report["unresolved"]).len()
    );
}

fn caller_identities(report: &Value) -> BTreeSet<(String, String, u64)> {
    let mut found = BTreeSet::new();
    for hit in array(&report["hits"]) {
        let symbol = &hit["symbol"];
        assert!(
            found.insert((
                text(&symbol["path"]).to_owned(),
                text(&symbol["name"]).to_owned(),
                hit["depth"].as_u64().unwrap()
            )),
            "duplicate caller identity"
        );
        assert_eq!(hit["read"]["path"], symbol["path"]);
        assert_eq!(hit["read"]["selector"]["value"], symbol["name"]);
        assert_eq!(hit["read"]["expected_hash"], symbol["source_hash"]);
    }
    assert_eq!(report["total_matches"], found.len());
    assert_eq!(report["returned_matches"], found.len());
    found
}

fn assert_call_sites(report: &Value, oracle: &RetryOracle) {
    let mut sites = BTreeSet::new();
    for hit in array(&report["hits"]) {
        assert_ne!(array(&hit["evidence"]), &[] as &[Value]);
        for edge in array(&hit["evidence"]) {
            assert_eq!(edge["kind"], "call");
            let source = &edge["source"];
            let target = &edge["target"];
            let source_path = text(&source["path"]);
            let target_path = text(&target["path"]);
            assert_eq!(source["source_hash"], sha256(&oracle.files[source_path]));
            assert_eq!(target["source_hash"], sha256(&oracle.files[target_path]));
            let start = usize::try_from(edge["site"]["start_byte"].as_u64().unwrap()).unwrap();
            let end = usize::try_from(edge["site"]["end_byte"].as_u64().unwrap()).unwrap();
            assert_eq!(edge["site"]["start_line"], edge["site"]["end_line"]);
            let spelling = oracle.files[source_path]
                .get(start..end)
                .unwrap()
                .to_owned();
            assert!(
                sites.insert(CallSite {
                    caller: (source_path.to_owned(), text(&source["name"]).to_owned()),
                    callee: (target_path.to_owned(), text(&target["name"]).to_owned()),
                    line: edge["site"]["start_line"].as_u64().unwrap(),
                    spelling,
                    syntax: text(&edge["syntax"]).to_owned(),
                }),
                "duplicate call-site evidence"
            );
        }
    }
    assert_eq!(sites, oracle.calls);
    assert_unresolved_sites(report, oracle);
}

fn assert_unresolved_sites(report: &Value, oracle: &RetryOracle) {
    let mut sites = BTreeSet::new();
    let mut controls = BTreeSet::new();
    for item in array(&report["unresolved"]) {
        let path = text(&item["source_path"]);
        let start = usize::try_from(item["site"]["start_byte"].as_u64().unwrap()).unwrap();
        let end = usize::try_from(item["site"]["end_byte"].as_u64().unwrap()).unwrap();
        let kind = text(&item["kind"]);
        assert_eq!(item["source_hash"], sha256(&oracle.files[path]));
        assert!(start < end);
        let spelling = oracle.files[path].get(start..end).unwrap();
        // A call and its argument can be separate unresolved sites on the same line.
        assert!(
            sites.insert((path, start, end, kind)),
            "duplicate unresolved syntax site: {item}"
        );
        controls.insert((
            path,
            item["site"]["start_line"].as_u64().unwrap(),
            kind,
            spelling,
            text(&item["reason"]),
        ));
    }
    for expected in [
        (
            oracle.checkout_path.as_str(),
            5,
            "call",
            "charge(attempt)",
            "shadowed-binding",
        ),
        (
            "src/decoys.rs",
            2,
            "call",
            "sender.dispatch_request()",
            "dynamic-receiver",
        ),
    ] {
        assert!(
            controls.contains(&expected),
            "missing explicit gap {expected:?}"
        );
    }
}

fn assert_debug_plan(plan: &Value, oracle: &RetryOracle) {
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
    assert_eq!(plan["selected_files"], 4);
    assert_eq!(array(&plan["selected"]).len(), 6);
    let mut selected = BTreeSet::new();
    for definition in array(&plan["selected"]) {
        let file = array(&plan["files"])
            .iter()
            .find(|file| file["id"] == definition["file"])
            .unwrap();
        let path = text(&file["path"]);
        let name = text(&definition["name"]);
        assert!(
            selected.insert((path.to_owned(), name.to_owned())),
            "duplicate selected definition"
        );
        assert_eq!(file["sha256"], sha256(&oracle.files[path]));
        if name == "Attempt" {
            assert_eq!(definition["role"], "environment");
        } else {
            assert_eq!(definition["role"], "direct");
            assert!(
                array(&definition["environment_gaps"])
                    .iter()
                    .any(|gap| gap == "body-dependencies-not-expanded")
            );
        }
    }
    assert_eq!(selected, oracle.definitions.keys().cloned().collect());
    assert_source_bundle(&plan["source"], &oracle.files, &oracle.definitions);
}

fn assert_debug_evidence(evidence: &DebugEvidence, oracle: &RetryOracle) {
    assert_eq!(
        evidence.refreshes, 1,
        "staleness must restart from the symptom"
    );
    assert_eq!(evidence.recheck["requested_targets"], 1);
    assert_eq!(evidence.recheck["omitted_targets"], 0);
    assert_eq!(evidence.recheck["results"][0]["status"], "stale");
    assert!(evidence.recheck.get("sources").is_none());
    assert_search(&evidence.current.report, oracle, &sha256(&oracle.new_seed));
    let fixed = BTreeMap::from([(
        (oracle.seed_path.clone(), "dispatch_request".to_owned()),
        FIXED_DISPATCH.to_owned(),
    )]);
    assert_source_bundle(&evidence.current.source, &oracle.files, &fixed);
    assert_eq!(
        evidence.traversal.len(),
        2,
        "the depth gap must trigger a bounded retry"
    );
    let shallow = &evidence.traversal[0];
    assert_eq!(shallow["depth"], 1);
    assert!(shallow["depth_omitted"].as_u64().unwrap() > 0);
    assert_call_coverage(shallow);
    assert_eq!(
        caller_identities(shallow),
        oracle
            .callers
            .iter()
            .filter(|(_, _, depth)| *depth == 1)
            .cloned()
            .collect()
    );
    let full = &evidence.traversal[1];
    assert_eq!(full["depth"], 3);
    assert_eq!(full["depth_omitted"], 0);
    assert_call_coverage(full);
    assert_eq!(caller_identities(full), oracle.callers);
    assert_call_sites(full, oracle);
    assert_debug_plan(&evidence.plan, oracle);
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn symptom_search_recovers_stale_identity_and_collects_proven_caller_context() {
    let task = DebugTask {
        symptom: "duplicate payment retry",
        context_tokens: "12000",
        initial_depth: 1,
        maximum_depth: 3,
    };
    for layout in [
        RetryLayout {
            charge: "billing",
            checkout: "checkout",
            service: "orders",
            entry: "gateway",
        },
        RetryLayout {
            charge: "z_rules",
            checkout: "m_cart",
            service: "b_orders",
            entry: "a_gateway",
        },
    ] {
        let (fixture, mut oracle) = retry_fixture(&layout);
        let mut journey = Journey::new(&fixture);
        let initial = discover_symptom(&mut journey, &task);
        assert_search(&initial.report, &oracle, &sha256(&oracle.old_seed));
        let expected = BTreeMap::from([(
            (oracle.seed_path.clone(), "dispatch_request".to_owned()),
            INITIAL_DISPATCH.to_owned(),
        )]);
        assert_source_bundle(&initial.source, &oracle.files, &expected);

        // Another developer fixes and moves the function after its identity was observed.
        fixture.write(&oracle.seed_path, &oracle.new_seed);
        oracle
            .files
            .insert(oracle.seed_path.clone(), oracle.new_seed.clone());
        let evidence = resume_debugging(&mut journey, &task, &initial);
        assert_ne!(initial.target.hash, evidence.current.target.hash);
        assert_debug_evidence(&evidence, &oracle);
    }
}
