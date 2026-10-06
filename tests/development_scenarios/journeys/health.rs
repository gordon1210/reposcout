//! Health investigations select their next reads from observed findings, never fixture answers.

use super::support::Journey;
use crate::support::Fixture;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

struct BaselineTask<'a> {
    root: &'a str,
    baseline: &'a Path,
    max_complexity: u32,
}

enum Investigation {
    Unavailable {
        comparison: Value,
        reason: String,
    },
    Complete {
        comparison: Value,
        read: Option<Value>,
        gate: Value,
    },
}

fn health_arguments(task: &BaselineTask<'_>) -> Vec<String> {
    [
        task.root,
        "--only",
        "complexity,markers",
        "--max-complexity",
        &task.max_complexity.to_string(),
        "--no-project-config",
        "--no-cache",
        "-f",
        "json",
        "--quiet",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn save_baseline(journey: &mut Journey<'_>, task: &BaselineTask<'_>, compact: &str) {
    let mut arguments = health_arguments(task);
    arguments.extend([
        compact.to_owned(),
        "--output".to_owned(),
        task.baseline.to_str().unwrap().to_owned(),
    ]);
    journey.step(
        "save a health baseline outside the source tree",
        &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        0,
    );
}

fn assert_complete_scan(report: &Value) {
    let analyzers = &report["analysis_profile"]["analyzers"];
    assert_eq!(analyzers["complexity"], true);
    assert_eq!(analyzers["markers"], true);
    assert_complete_discovery(report);
    assert_eq!(
        report["files"].as_array().unwrap().len(),
        usize::try_from(report["diagnostics"]["analyzed_files"].as_u64().unwrap()).unwrap()
    );
}

fn assert_complete_discovery(report: &Value) {
    let diagnostics = &report["diagnostics"];
    let analyzed = diagnostics["analyzed_files"].as_u64().unwrap();
    assert!(analyzed > 0, "an empty scan cannot establish a clean task");
    assert_eq!(diagnostics["discovered_files"].as_u64().unwrap(), analyzed);
    assert_eq!(report["summary"]["files"].as_u64().unwrap(), analyzed);
    for key in ["unsupported_files", "unreadable_files", "walker_errors"] {
        assert_eq!(diagnostics[key].as_u64().unwrap(), 0, "{key}");
    }
    for key in [
        "oversized_files",
        "ignore_files_rejected",
        "files_omitted_by_limit",
    ] {
        if let Some(value) = diagnostics.get(key) {
            assert_eq!(value.as_u64().unwrap(), 0, "{key}");
        }
    }
    for key in [
        "scan_truncated",
        "duration_limit_reached",
        "files_omitted_count_incomplete",
    ] {
        if let Some(value) = diagnostics.get(key) {
            assert!(!value.as_bool().unwrap(), "{key}");
        }
    }
}

fn investigate_baseline(journey: &mut Journey<'_>, task: &BaselineTask<'_>) -> Investigation {
    let mut arguments = health_arguments(task);
    arguments.extend([
        "--baseline".to_owned(),
        task.baseline.to_str().unwrap().to_owned(),
    ]);
    let comparison = journey
        .step(
            "compare current health with the saved baseline",
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
            0,
        )
        .stdout_json();
    assert_complete_scan(&comparison);
    let delta = &comparison["baseline"]["finding_changes"];
    if delta["comparison"] == "unavailable" {
        let reason = delta["reason"].as_str().unwrap().to_owned();
        assert!(
            !reason.is_empty(),
            "unknown comparison needs an explanation"
        );
        return Investigation::Unavailable { comparison, reason };
    }
    assert_eq!(delta["comparison"], "complete");
    let targets: Vec<_> = delta["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|change| matches!(change["state"].as_str().unwrap(), "new" | "worsened"))
        .map(|change| {
            let location = &change["after"]["primary_location"];
            let path = location["path"].as_str().unwrap().to_owned();
            let line = location["start_line"].as_u64().unwrap();
            assert!(!path.is_empty() && line > 0, "finding must locate source");
            (path, line)
        })
        .collect();
    let read = (!targets.is_empty()).then(|| read_finding_locations(journey, task.root, &targets));
    let regressed = comparison["baseline"]["regressed"].as_bool().unwrap();
    arguments.push("--fail-on-regression".to_owned());
    let gate = journey
        .step(
            "apply the observed baseline regression gate",
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
            if regressed { 2 } else { 0 },
        )
        .stdout_json();
    assert_eq!(gate["baseline"], comparison["baseline"]);
    assert_eq!(gate["finding_catalog"], comparison["finding_catalog"]);
    Investigation::Complete {
        comparison,
        read,
        gate,
    }
}

fn read_finding_locations(
    journey: &mut Journey<'_>,
    root: &str,
    targets: &[(String, u64)],
) -> Value {
    let mut arguments: Vec<String> = [
        "read",
        root,
        "--budget",
        "16384",
        "--max-output-bytes",
        "262144",
        "--no-project-config",
        "--no-cache",
        "-f",
        "json",
        "--quiet",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (path, line) in targets {
        arguments.extend(["--line".to_owned(), path.clone(), line.to_string()]);
    }
    // Scan finding locations have no hashes. The fixture stays unchanged during this handoff;
    // the first read establishes content identity, rather than claiming an atomic scan snapshot.
    let read = journey
        .step(
            "read declarations enclosing the selected report locations",
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
            0,
        )
        .stdout_json();
    assert_eq!(read["requested_targets"], targets.len());
    assert_eq!(read["omitted_targets"], 0);
    let results = read["results"].as_array().unwrap();
    assert_eq!(results.len(), targets.len());
    let mut identities = BTreeSet::new();
    for result in results {
        assert_eq!(result["status"], "complete");
        let target = usize::try_from(result["target"].as_u64().unwrap()).unwrap();
        assert!(identities.insert(target), "duplicate result target");
        let (path, line) = &targets[target.checked_sub(1).unwrap()];
        let file = by_id(&read["files"], &result["file"]);
        assert_eq!(file["path"], *path);
        assert_eq!(file["snapshot"], json!({"kind":"worktree"}));
        assert_eq!(file["extraction"], "available");
        let span = &result["definition"]["declaration_span"];
        assert!(span["start_line"].as_u64().unwrap() <= *line);
        assert!(span["end_line"].as_u64().unwrap() >= *line);
        let source = by_id(&read["sources"], &result["source"]);
        assert_eq!(source["file"], file["id"]);
        assert_eq!(source["span"], result["definition"]["source_span"]);
        assert!(!source["content"].as_str().unwrap().is_empty());
    }
    read
}

fn by_id<'a>(entries: &'a Value, id: &Value) -> &'a Value {
    let id = id.as_u64().unwrap();
    let matching: Vec<_> = entries
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["id"].as_u64().unwrap() == id)
        .collect();
    assert_eq!(matching.len(), 1, "one record for referenced identity {id}");
    matching[0]
}

struct Roles {
    worsened: &'static str,
    improved: &'static str,
    resolved: &'static str,
    added: &'static str,
    old_marker: &'static str,
    old_complexity: &'static str,
}

const CLEAN_ACCEPT: &str = "def accept_request(payload):\n    return payload[\"account\"]";
const NEW_DEBT: &str = "def accept_request(payload):\n    # FIXME validate the account scope\n    return payload[\"account\"]";
const OLD_DEBT: &str = "def legacy_export(payload):\n    # TODO replace the legacy wire format\n    return payload[\"id\"]";
const STABLE_DEBT: &str = "def compatibility_mode(payload):\n    # TODO retire the old client contract\n    return payload[\"mode\"]";
const WORSE_ROUTE: &str = "def route(value):\n    if value == 0:\n        return 0\n    if value == 1:\n        return 1\n    if value == 2:\n        return 2\n    return -1";

fn write_source(fixture: &Fixture, path: &str, source: &str) {
    let language = reposcout::lang::detect(Path::new(path)).unwrap();
    let tree = reposcout::parse::parse(language.first_class.unwrap(), source).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "invalid fixture syntax: {path}"
    );
    fixture.write(path, source);
}

fn decision_source(name: &str, branches: usize) -> String {
    let mut source = format!("def {name}(value):\n");
    for branch in 0..branches {
        writeln!(source, "    if value == {branch}:\n        return {branch}").unwrap();
    }
    source.push_str("    return -1");
    source
}

fn setup_health_repository(fixture: &Fixture, roles: &Roles) {
    for area in ["accounts", "billing", "catalog", "delivery", "search"] {
        for stage in 0..6 {
            let source = if stage == 0 {
                format!("def transform(payload):\n    return payload[\"{area}\"]")
            } else {
                format!(
                    "from .stage_{} import transform as previous\n\nMESSAGE = \"TODO and FIXME are user-visible labels\"\n\ndef transform(payload):\n    return previous(payload) + {stage}",
                    stage - 1
                )
            };
            write_source(fixture, &format!("app/{area}/stage_{stage}.py"), &source);
        }
    }
    for index in 0..3 {
        fixture.write(
            &format!("docs/contract_{index}.md"),
            "# Compatibility contract\nTODO and FIXME describe response labels, not source debt.\n",
        );
        fixture.write(
            &format!("content/labels_{index}.json"),
            "{\"labels\":[\"TODO\",\"FIXME\"]}\n",
        );
    }
    write_source(fixture, roles.worsened, &decision_source("route", 2));
    write_source(fixture, roles.improved, &decision_source("choose", 3));
    write_source(fixture, roles.old_complexity, &decision_source("legacy", 4));
    write_source(fixture, roles.resolved, OLD_DEBT);
    write_source(fixture, roles.added, CLEAN_ACCEPT);
    write_source(fixture, roles.old_marker, STABLE_DEBT);
}

fn apply_mixed_health_changes(fixture: &Fixture, roles: &Roles) {
    write_source(fixture, roles.worsened, &decision_source("route", 3));
    write_source(fixture, roles.improved, &decision_source("choose", 2));
    write_source(
        fixture,
        roles.resolved,
        "def legacy_export(payload):\n    return payload[\"id\"]",
    );
    write_source(fixture, roles.added, NEW_DEBT);
    write_source(fixture, roles.old_marker, &format!("\n\n{STABLE_DEBT}"));
}

fn finding_paths(report: &Value) -> BTreeSet<String> {
    let findings = report["finding_catalog"]["findings"].as_array().unwrap();
    let paths: BTreeSet<_> = findings
        .iter()
        .map(|finding| {
            finding["primary_location"]["path"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        paths.len(),
        findings.len(),
        "this fixture has exactly one authored health finding per affected file"
    );
    let fingerprints: BTreeSet<_> = findings
        .iter()
        .map(|finding| finding["fingerprint"].as_str().unwrap())
        .collect();
    assert_eq!(
        fingerprints.len(),
        findings.len(),
        "duplicate finding identities"
    );
    paths
}

fn assert_health_catalog(report: &Value, expected: &[(&str, &str)]) {
    assert_eq!(finding_paths(report).len(), expected.len());
    let actual: BTreeSet<_> = report["finding_catalog"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["kind"].as_str().unwrap(),
                finding["primary_location"]["path"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(actual, expected.iter().copied().collect());
}

fn assert_delta(report: &Value, expected: &[(&str, &str)]) {
    let delta = &report["baseline"]["finding_changes"];
    assert_eq!(delta["comparison"], "complete");
    let changes = delta["changes"].as_array().unwrap();
    let actual: BTreeSet<_> = changes
        .iter()
        .map(|change| {
            let finding = change.get("after").unwrap_or(&change["before"]);
            (
                change["state"].as_str().unwrap(),
                finding["primary_location"]["path"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(actual.len(), changes.len(), "no duplicate finding changes");
    assert_eq!(actual, expected.iter().copied().collect());
    for state in ["new", "worsened", "resolved", "improved"] {
        assert_eq!(
            delta["counts"][state],
            expected.iter().filter(|(kind, _)| *kind == state).count()
        );
    }
}

fn assert_complexity_change(report: &Value, path: &str, before: u32, after: u32) {
    let changes: Vec<_> = report["baseline"]["finding_changes"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|change| change["after"]["primary_location"]["path"] == path)
        .collect();
    assert_eq!(changes.len(), 1);
    for (side, expected) in [("before", before), ("after", after)] {
        assert_eq!(changes[0][side]["kind"], "complexity");
        assert_eq!(
            changes[0][side]["metrics"]["cyclomatic"],
            json!(f64::from(expected))
        );
        assert_eq!(changes[0][side]["metrics"]["threshold"], json!(2.0));
    }
}

fn assert_investigation_sources(read: &Value, roles: &Roles) {
    assert_selected_sources(
        read,
        &[
            (roles.worsened, "route", WORSE_ROUTE),
            (roles.added, "accept_request", NEW_DEBT),
        ],
    );
}

fn assert_selected_sources(read: &Value, expected: &[(&str, &str, &str)]) {
    let expected: BTreeMap<_, _> = expected
        .iter()
        .map(|(path, name, content)| (*path, (*name, *content)))
        .collect();
    let results = read["results"].as_array().unwrap();
    assert_eq!(results.len(), expected.len());
    assert_eq!(read["files"].as_array().unwrap().len(), expected.len());
    assert_eq!(read["sources"].as_array().unwrap().len(), expected.len());
    let mut observed = BTreeSet::new();
    for result in results {
        let file = by_id(&read["files"], &result["file"]);
        let path = file["path"].as_str().unwrap();
        assert!(observed.insert(path), "duplicate source identity");
        let (name, content) = expected.get(path).unwrap();
        assert_eq!(result["definition"]["name"], *name);
        assert_eq!(result["definition"]["kind"], "function");
        let source = by_id(&read["sources"], &result["source"]);
        assert_eq!(source["content"], *content);
        assert_eq!(source["span"]["start_byte"], 0);
        assert_eq!(source["span"]["end_byte"], content.len());
        let mut hash = String::with_capacity(64);
        for byte in Sha256::digest(content.as_bytes()) {
            write!(hash, "{byte:02x}").unwrap();
        }
        assert_eq!(file["sha256"], hash);
    }
    assert_eq!(observed, expected.keys().copied().collect());
}

fn assert_regression_packet(result: &Investigation, roles: &Roles) {
    let Investigation::Complete {
        comparison,
        read,
        gate,
    } = result
    else {
        panic!("a finding-complete baseline must support investigation");
    };
    assert_eq!(comparison["summary"]["files"], 42);
    assert_eq!(comparison["summary"]["source"]["files"], 36);
    assert_eq!(gate["baseline"]["regressed"], true);
    assert_delta(
        comparison,
        &[
            ("worsened", roles.worsened),
            ("improved", roles.improved),
            ("resolved", roles.resolved),
            ("new", roles.added),
        ],
    );
    // One entry path plus respectively two/three independent `if` branches.
    assert_complexity_change(comparison, roles.worsened, 3, 4);
    assert_complexity_change(comparison, roles.improved, 4, 3);
    assert_eq!(
        finding_paths(comparison),
        [
            roles.worsened,
            roles.improved,
            roles.added,
            roles.old_marker,
            roles.old_complexity,
        ]
        .map(str::to_owned)
        .into_iter()
        .collect()
    );
    assert_investigation_sources(read.as_ref().unwrap(), roles);
}

fn assert_no_new_debt(
    result: &Investigation,
    expected_changes: &[(&str, &str)],
    expected_catalog: &[(&str, &str)],
) {
    let Investigation::Complete {
        comparison,
        read,
        gate,
    } = result
    else {
        panic!("a complete comparison must not turn into unknown");
    };
    assert_eq!(gate["baseline"]["regressed"], false);
    assert!(
        read.is_none(),
        "old or improving debt needs no regression read"
    );
    assert_delta(comparison, expected_changes);
    assert_health_catalog(comparison, expected_catalog);
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn baseline_findings_drive_source_investigation_and_a_remediation_recheck() {
    let layouts = [
        Roles {
            worsened: "app/edge_0.py",
            improved: "app/edge_1.py",
            resolved: "app/edge_2.py",
            added: "app/edge_3.py",
            old_marker: "app/edge_4.py",
            old_complexity: "app/edge_5.py",
        },
        Roles {
            worsened: "app/edge_5.py",
            improved: "app/edge_3.py",
            resolved: "app/edge_0.py",
            added: "app/edge_1.py",
            old_marker: "app/edge_2.py",
            old_complexity: "app/edge_4.py",
        },
    ];
    for roles in layouts {
        let fixture = Fixture::new("baseline-investigation");
        setup_health_repository(&fixture, &roles);
        let baseline = fixture.state_path().join("findings.json");
        let aggregate = fixture.state_path().join("aggregate.json");
        let task = BaselineTask {
            root: ".",
            baseline: &baseline,
            max_complexity: 2,
        };
        let aggregate_task = BaselineTask {
            baseline: &aggregate,
            ..task
        };
        let mut journey = Journey::new(&fixture);
        save_baseline(&mut journey, &task, "--baseline-ready");
        save_baseline(&mut journey, &aggregate_task, "--summary");
        assert_no_new_debt(
            &investigate_baseline(&mut journey, &task),
            &[],
            &[
                ("complexity", roles.worsened),
                ("complexity", roles.improved),
                ("complexity", roles.old_complexity),
                ("marker", roles.resolved),
                ("marker", roles.old_marker),
            ],
        );

        apply_mixed_health_changes(&fixture, &roles);
        let regression = investigate_baseline(&mut journey, &task);
        assert_regression_packet(&regression, &roles);
        let unknown = investigate_baseline(&mut journey, &aggregate_task);
        let Investigation::Unavailable { comparison, reason } = unknown else {
            panic!("an aggregate-only baseline cannot establish finding-level absence");
        };
        assert!(reason.contains("catalog"));
        assert_eq!(
            comparison["baseline"]["finding_changes"]["comparison"],
            "unavailable"
        );
        assert_eq!(finding_paths(&comparison).len(), 5);

        // Authored fixture edits model the human fix, not a driver with an embedded answer key.
        write_source(&fixture, roles.worsened, &decision_source("route", 2));
        write_source(&fixture, roles.added, CLEAN_ACCEPT);
        let remediated = investigate_baseline(&mut journey, &task);
        assert_no_new_debt(
            &remediated,
            &[("resolved", roles.resolved), ("improved", roles.improved)],
            &[
                ("complexity", roles.worsened),
                ("complexity", roles.improved),
                ("complexity", roles.old_complexity),
                ("marker", roles.old_marker),
            ],
        );
    }
}

struct CleanupTask<'a> {
    root: &'a str,
    baseline: &'a Path,
}

struct CleanupInvestigation {
    summary: Value,
    read: Option<Value>,
    gate: Value,
}

fn duplication_arguments(root: &str) -> Vec<String> {
    [
        "dup",
        root,
        "--no-project-config",
        "--no-cache",
        "-f",
        "json",
        "--quiet",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn save_cleanup_baseline(journey: &mut Journey<'_>, task: &CleanupTask<'_>) {
    let mut arguments = duplication_arguments(task.root);
    arguments.extend([
        "--baseline-ready".to_owned(),
        "--output".to_owned(),
        task.baseline.to_str().unwrap().to_owned(),
    ]);
    journey.step(
        "save the existing duplication debt before extraction",
        &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
        0,
    );
}

fn assert_complete_duplication(report: &Value) {
    assert_complete_discovery(report);
    assert_eq!(report["analysis_profile"]["analyzers"]["duplication"], true);
    assert_eq!(report["analysis_profile"]["health"]["scope"], "source");
    assert_eq!(
        report["analysis_profile"]["duplication"]["artifact_policy"],
        "exclude"
    );
    for key in ["type1_analysis_partial", "type2_analysis_partial"] {
        if let Some(value) = report["diagnostics"].get(key) {
            assert!(!value.as_bool().unwrap(), "{key}");
        }
    }
    let production = &report["summary"]["assessment"]["production_duplication"];
    assert_eq!(production["corpus"], "production-source");
    assert_eq!(production["complete"], true);
    assert!(production["analyzed_lines"].as_u64().unwrap() > 0);
}

fn compact_location(location: &str) -> (String, u64, u64) {
    let (path, range) = location.rsplit_once(':').expect("path:start-end location");
    let (start, end) = range.split_once('-').expect("bounded duplicate line range");
    let start = start.parse::<u64>().unwrap();
    let end = end.parse::<u64>().unwrap();
    assert!(!path.is_empty() && start > 0 && end >= start, "{location}");
    (path.to_owned(), start, end)
}

fn investigate_production_clones(
    journey: &mut Journey<'_>,
    task: &CleanupTask<'_>,
) -> CleanupInvestigation {
    let mut arguments = duplication_arguments(task.root);
    arguments.extend([
        "--baseline".to_owned(),
        task.baseline.to_str().unwrap().to_owned(),
    ]);
    let summary_arguments: Vec<_> = arguments
        .iter()
        .map(String::as_str)
        .chain(["--summary"])
        .collect();
    let summary = journey
        .step(
            "rank production clone cleanup candidates",
            &summary_arguments,
            0,
        )
        .stdout_json();
    assert_complete_duplication(&summary);
    let blocks = summary["summary"]
        .get("top_production_duplicates")
        .map_or(&[][..], |value| value.as_array().unwrap().as_slice());
    let read = if let Some(first) = blocks.first() {
        // The public ranking prioritizes removable lines. Do not choose known fixture filenames.
        let locations = first["locations"].as_array().unwrap();
        assert!(first["copies"].as_u64().unwrap() > 1);
        assert_eq!(
            first["copies"],
            locations.len(),
            "all selected copies must be addressable"
        );
        let targets: Vec<_> = locations
            .iter()
            .map(|location| {
                let (path, start, _) = compact_location(location.as_str().unwrap());
                (path, start)
            })
            .collect();
        Some(read_finding_locations(journey, task.root, &targets))
    } else {
        assert_eq!(
            summary["summary"]["assessment"]["production_duplication"]["duplicated_lines"], 0,
            "an absent ranking cannot hide observed production duplication"
        );
        None
    };
    let regressed = summary["baseline"]["regressed"].as_bool().unwrap();
    arguments.push("--fail-on-regression".to_owned());
    let gate = journey
        .step(
            "check the duplication baseline and retain complete corpus evidence",
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
            if regressed { 2 } else { 0 },
        )
        .stdout_json();
    assert_eq!(gate["baseline"], summary["baseline"]);
    assert_eq!(gate["summary"], summary["summary"]);
    CleanupInvestigation {
        summary,
        read,
        gate,
    }
}

const INVOICE_SOURCE: &str = r#"export function compute_invoice(items) {
    let subtotal = 0;
    for (const item of items) {
        const net = item.price * item.quantity;
        const discount = item.member ? 0.9 : 1;
        const taxed = net * discount * 1.19;
        subtotal += Math.round(taxed * 100);
    }
    const delivery = subtotal > 5000 ? 0 : 490;
    const total = Math.max(0, subtotal + delivery);
    const currency = "EUR";
    const reference = items.map(item => item.sku).join(":");
    return { subtotal, delivery, total, currency, reference };
}"#;

const TEST_FACTORY: &str = r#"export function fixture_invoice() {
    const customer = Object.freeze({ id: "guest", country: "DE" });
    const items = [
        { sku: "book", price: 1200, quantity: 2, member: false },
        { sku: "cable", price: 500, quantity: 1, member: true },
        { sku: "stand", price: 2500, quantity: 3, member: false }
    ];
    const request = {
        customer,
        items,
        delivery: {
            street: "Example Road 4",
            postcode: "12345",
            city: "Sample City"
        },
        payment: {
            method: "invoice",
            account: "fixture-account",
            reference: "ordered-fixture"
        },
        metadata: {
            channel: "synthetic",
            version: 3,
            flags: ["retail", "domestic"]
        }
    };
    return JSON.parse(JSON.stringify(request));
}"#;

const TEST_CLONES: [&str; 4] = [
    "tests/orders_a.test.js",
    "tests/orders_b.test.js",
    "tests/returns_a.test.js",
    "tests/returns_b.test.js",
];
const BUNDLE_CLONES: [&str; 2] = ["public/invoice.bundle.js", "static/js/main.ab12cd.chunk.js"];

struct CloneRoles {
    production: [&'static str; 2],
    unrelated: [&'static str; 2],
}

fn setup_clone_repository(fixture: &Fixture, roles: &CloneRoles) -> BTreeSet<String> {
    let mut inventory = BTreeSet::new();
    for path in roles.production {
        write_source(fixture, path, INVOICE_SOURCE);
        inventory.insert(path.to_owned());
    }
    for (index, path) in roles.unrelated.iter().enumerate() {
        write_source(
            fixture,
            path,
            &format!("export const preview_{index} = {index};"),
        );
        inventory.insert((*path).to_owned());
    }
    write_source(fixture, "src/shared.js", "export const currency = \"EUR\";");
    inventory.insert("src/shared.js".to_owned());
    for index in 0..18 {
        let path = format!("src/features/feature_{index}/label.js");
        write_source(
            fixture,
            &path,
            &format!(
                "export function label_{index}(value) {{ return String(value) + \"/{index}\"; }}"
            ),
        );
        inventory.insert(path);
    }
    for path in TEST_CLONES {
        write_source(fixture, path, TEST_FACTORY);
        inventory.insert(path.to_owned());
    }
    for path in BUNDLE_CLONES {
        write_source(fixture, path, &format!("{INVOICE_SOURCE}\n{TEST_FACTORY}"));
        inventory.insert(path.to_owned());
    }
    for index in 0..2 {
        let documentation = format!("docs/contracts_{index}.md");
        fixture.write(
            &documentation,
            &"# Invoice contract\nThe example is content, not source.\n".repeat(40),
        );
        inventory.insert(documentation);
        let content = format!("content/catalog_{index}.json");
        fixture.write(
            &content,
            &json!({"examples": vec!["invoice"; 100]}).to_string(),
        );
        inventory.insert(content);
    }
    inventory
}

fn assert_compact_clone(block: &Value, paths: &[&str], source: &str) {
    assert_eq!(block["copies"], paths.len());
    assert_eq!(block["lines"], source.lines().count());
    assert_eq!(
        block["duplicated_lines"],
        source.lines().count() * (paths.len() - 1)
    );
    assert_eq!(block["similarity"], json!(1.0));
    let locations = block["locations"].as_array().unwrap();
    let actual: BTreeSet<_> = locations
        .iter()
        .map(|location| {
            let (path, start, end) = compact_location(location.as_str().unwrap());
            assert_eq!(start, 1);
            assert_eq!(usize::try_from(end).unwrap(), source.lines().count());
            path
        })
        .collect();
    assert_eq!(actual.len(), locations.len(), "duplicate compact locations");
    assert_eq!(
        actual,
        paths.iter().map(|path| (*path).to_owned()).collect()
    );
}

fn assert_clone_corpus(result: &CleanupInvestigation, inventory: &BTreeSet<String>) {
    let files = result.gate["files"].as_array().unwrap();
    let paths: BTreeSet<_> = files
        .iter()
        .map(|file| file["path"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(paths.len(), files.len(), "duplicate inventory entries");
    assert_eq!(&paths, inventory);
    assert_eq!(files.len(), 33);
    let coverage = result.gate["duplicates"]["file_coverage"]
        .as_array()
        .unwrap();
    let corpus: BTreeSet<_> = coverage
        .iter()
        .map(|file| file["path"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(corpus.len(), coverage.len(), "duplicate coverage entries");
    let expected = inventory
        .iter()
        .filter(|path| {
            !path.starts_with("docs/")
                && !path.starts_with("content/")
                && !BUNDLE_CLONES.contains(&path.as_str())
        })
        .cloned()
        .collect();
    assert_eq!(corpus, expected);
    // The largest raw family is deliberately test-only, before and after the production fix.
    assert_compact_clone(
        &result.summary["summary"]["top_duplicates"][0],
        &TEST_CLONES,
        TEST_FACTORY,
    );
    let expected_test_paths: BTreeSet<_> = TEST_CLONES.into_iter().collect();
    let families: Vec<_> = result.gate["duplicates"]["exact"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|family| {
            let instances = family["instances"].as_array().unwrap();
            let paths: BTreeSet<_> = instances
                .iter()
                .map(|instance| instance["path"].as_str().unwrap())
                .collect();
            paths == expected_test_paths && instances.len() == TEST_CLONES.len()
        })
        .collect();
    assert_eq!(
        families.len(),
        1,
        "the unchanged full test factory remains an exact family"
    );
}

fn assert_selected_production_clones(result: &CleanupInvestigation, roles: &CloneRoles) {
    let blocks = result.summary["summary"]["top_production_duplicates"]
        .as_array()
        .unwrap();
    assert_eq!(blocks.len(), 1);
    assert_compact_clone(&blocks[0], &roles.production, INVOICE_SOURCE);
    assert_eq!(
        result.summary["summary"]["assessment"]["production_duplication"]["duplicated_lines"],
        2 * INVOICE_SOURCE.lines().count()
    );
    assert_selected_sources(
        result.read.as_ref().unwrap(),
        &roles
            .production
            .map(|path| (path, "compute_invoice", INVOICE_SOURCE)),
    );
    assert_eq!(
        result.gate["baseline"]["regressed"], false,
        "existing debt matches its baseline"
    );
    assert_delta(&result.gate, &[]);
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn production_clone_locations_drive_reads_and_extraction_leaves_test_debt_visible() {
    for roles in [
        CloneRoles {
            production: ["src/edge_0.js", "src/edge:1.js"],
            unrelated: ["src/edge_2.js", "src/edge_3.js"],
        },
        CloneRoles {
            production: ["src/edge_2.js", "src/edge_3.js"],
            unrelated: ["src/edge_0.js", "src/edge:1.js"],
        },
    ] {
        let fixture = Fixture::new("production-clone-cleanup");
        let inventory = setup_clone_repository(&fixture, &roles);
        let baseline = fixture.state_path().join("duplication.json");
        let task = CleanupTask {
            root: ".",
            baseline: &baseline,
        };
        let mut journey = Journey::new(&fixture);
        save_cleanup_baseline(&mut journey, &task);
        let before = investigate_production_clones(&mut journey, &task);
        assert_selected_production_clones(&before, &roles);
        assert_clone_corpus(&before, &inventory);

        // The test author models extracting the reviewed algorithm; the driver cannot edit source.
        write_source(&fixture, "src/shared.js", INVOICE_SOURCE);
        for path in roles.production {
            write_source(
                &fixture,
                path,
                "export { compute_invoice } from \"./shared.js\";",
            );
        }
        let after = investigate_production_clones(&mut journey, &task);
        assert_clone_corpus(&after, &inventory);
        assert!(
            after.read.is_none(),
            "no production candidate remains to investigate"
        );
        assert_eq!(after.gate["baseline"]["regressed"], false);
        let delta = &after.gate["baseline"]["finding_changes"];
        assert_eq!(delta["comparison"], "complete");
        assert_eq!(
            delta["counts"],
            json!({"new":0,"worsened":0,"resolved":1,"improved":0})
        );
        let changes = delta["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0]["state"], "resolved");
        assert_eq!(changes[0]["before"]["kind"], "duplication");
    }
}
