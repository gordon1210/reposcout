use super::support::Fixture;
use reposcout::metrics::tokens::TokenCounter;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;

fn array(value: &Value) -> &[Value] {
    value.as_array().expect("expected a JSON array")
}

fn strings(value: &Value) -> BTreeSet<&str> {
    array(value)
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect()
}

fn paths(value: &Value) -> BTreeSet<&str> {
    array(value)
        .iter()
        .map(|item| item["path"].as_str().unwrap())
        .collect()
}

fn consumers(report: &Value) -> BTreeSet<(&str, &str, u64)> {
    let identities: BTreeSet<_> = array(&report["hits"])
        .iter()
        .map(|hit| {
            (
                hit["symbol"]["path"].as_str().unwrap(),
                hit["symbol"]["name"].as_str().unwrap(),
                hit["depth"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        identities.len(),
        array(&report["hits"]).len(),
        "each reachable symbol appears once"
    );
    identities
}

fn edges(report: &Value) -> BTreeSet<(&str, &str, &str)> {
    let relationships: BTreeSet<_> = array(&report["graph"]["edge_list"])
        .iter()
        .map(|edge| {
            (
                edge["source"].as_str().unwrap(),
                edge["target"].as_str().unwrap(),
                edge["resolver"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        relationships.len(),
        array(&report["graph"]["edge_list"]).len(),
        "graph edges are unique"
    );
    assert_eq!(report["graph"]["edges"], relationships.len());
    relationships
}

fn source_contents(report: &Value) -> BTreeSet<&str> {
    report["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|source| source["content"].as_str().unwrap())
        .collect()
}

fn source_hash(source: &str) -> String {
    let mut hash = String::with_capacity(64);
    for byte in Sha256::digest(source.as_bytes()) {
        write!(hash, "{byte:02x}").unwrap();
    }
    hash
}

fn follow_read(fixture: &Fixture, target: &Value) -> Value {
    fixture.json(&[
        "read",
        ".",
        "--symbol",
        target["path"].as_str().unwrap(),
        target["selector"]["value"].as_str().unwrap(),
        "--expect-hash",
        target["path"].as_str().unwrap(),
        target["expected_hash"].as_str().unwrap(),
        "--no-project-config",
        "--no-cache",
    ])
}

fn assert_consumer_coverage(report: &Value, available: u64, unsupported: u64, unavailable: u64) {
    assert_eq!(
        report["coverage"]["files_total"],
        available + unsupported + unavailable
    );
    assert_eq!(report["coverage"]["files_available"], available);
    assert_eq!(report["coverage"]["files_unsupported"], unsupported);
    assert_eq!(report["coverage"]["files_unavailable"], unavailable);
    for field in [
        "files_parse_errors",
        "files_truncated",
        "declarations_omitted",
        "relations_omitted",
        "discovery_omitted",
        "unreadable_files",
        "oversized_files",
        "walker_errors",
    ] {
        assert_eq!(report["coverage"][field], 0, "{field}: {report}");
    }
    for field in ["path_omitted", "limit_omitted", "budget_omitted"] {
        assert_eq!(report[field], 0, "{field}: {report}");
    }
    for field in [
        "scan_truncated",
        "deadline_reached",
        "discovery_omitted_count_incomplete",
    ] {
        assert_eq!(report["coverage"][field], false, "{field}: {report}");
    }
    assert_eq!(report["coverage"]["resolution"]["omitted"], 0);
    assert_eq!(report["unresolved_omitted"], 0);
}

const ORIGINAL_DISPATCH: &str = r#"pub fn dispatch_request(attempt: u32) -> &'static str {
    let duplicate_payment_retry = attempt < 3;
    if duplicate_payment_retry { "ORIGINAL_PAYMENT_BODY" } else { "stop" }
}"#;

const UPDATED_DISPATCH: &str = r#"pub fn dispatch_request(attempt: u32) -> &'static str {
    let duplicate_payment_retry = attempt < 5;
    if duplicate_payment_retry { "UPDATED_PAYMENT_BODY" } else { "stop" }
}"#;

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn find_to_read_preserves_identity_through_a_payment_fix_and_rediscovery() {
    let fixture = Fixture::new("navigation-payment-search");
    fixture.write("payments/retry.rs", ORIGINAL_DISPATCH);
    for (path, source) in [
        (
            "payments/settle.rs",
            "pub fn settle_payment() -> bool { true }\n",
        ),
        ("http/retry.rs", "pub fn retry_request() -> u32 { 3 }\n"),
        (
            "archive/duplicates.rs",
            "pub fn duplicate_documents() -> u32 { 0 }\n",
        ),
        ("payments/store.rs", "pub struct Receipt { pub id: u64 }\n"),
        ("tests/retry.rs", "fn retry_limit() { assert_eq!(3, 3); }\n"),
        (
            "docs/runbook.md",
            "A duplicate payment must never be retried blindly.\n",
        ),
    ] {
        fixture.write(path, source);
    }
    let query = [
        "find",
        "duplicate payment retry",
        ".",
        "--health-exclude",
        "payments/**",
        "--no-project-config",
        "--no-cache",
        "--budget",
        "16384",
    ];
    let found = fixture.json(&query);
    assert_eq!(found["total_matches"], 1);
    assert_eq!(found["returned_matches"], 1);
    assert_eq!(found["coverage"]["parse_error_files"], 0);
    assert_eq!(found["coverage"]["unsupported_files"], 1);
    assert_eq!(found["limit_omitted"], 0);
    assert_eq!(found["budget_omitted"], 0);
    assert_eq!(found["hits"][0]["path"], "payments/retry.rs");
    assert!(!found.to_string().contains("ORIGINAL_PAYMENT_BODY"));
    let target = &found["hits"][0]["read"];
    assert_eq!(target["expected_hash"], source_hash(ORIGINAL_DISPATCH));
    let original = follow_read(&fixture, target);
    assert_eq!(original["results"][0]["status"], "complete");
    assert_eq!(
        source_contents(&original),
        BTreeSet::from([ORIGINAL_DISPATCH])
    );

    // Moving the declaration as well as changing its body makes a stale-span read detectable.
    let updated = format!("pub fn unrelated_prefix() -> u32 {{ 999 }}\n\n{UPDATED_DISPATCH}");
    fixture.write("payments/retry.rs", &updated);
    let stale = follow_read(&fixture, target);
    assert_eq!(stale["results"][0]["status"], "stale");
    assert!(source_contents(&stale).is_empty());
    let rediscovered = fixture.json(&query);
    assert_eq!(rediscovered["total_matches"], 1);
    let fresh_target = &rediscovered["hits"][0]["read"];
    assert_eq!(fresh_target["expected_hash"], source_hash(&updated));
    assert_ne!(fresh_target["expected_hash"], target["expected_hash"]);
    let current = follow_read(&fixture, fresh_target);
    assert_eq!(current["results"][0]["status"], "complete");
    assert_eq!(
        source_contents(&current),
        BTreeSet::from([UPDATED_DISPATCH])
    );
}

const CHECKOUT: &str = "export function checkout() { return calculate([3, 5]); }";

fn typescript_consumers_fixture() -> Fixture {
    let fixture = Fixture::new("navigation-typescript-consumers");
    fixture.write(
        "billing/amount.ts",
        "export function calculateTotal(lines: number[]) { return lines.length; }\n",
    );
    fixture.write(
        "checkout/service.ts",
        &format!(
            "import {{ calculateTotal as calculate }} from '../billing/amount.js';\n{CHECKOUT}\n\
         export function callback() {{ return calculate; }}\n\
         export function shadowed(calculate: () => number) {{ return calculate(); }}\n\
         export function locallyShadowed() {{ const calculate = () => 99; return calculate(); }}\n"
        ),
    );
    for (path, source) in [
        (
            "checkout/preview.ts",
            "import * as billing from '../billing/amount.js';\nexport function preview() { return billing.calculateTotal([7]); }\n",
        ),
        (
            "routes/checkout.ts",
            "import { checkout } from '../checkout/service.js';\nexport function route() { return checkout(); }\n",
        ),
        (
            "analytics/amount.ts",
            "export function calculateTotal() { return -1; }\nexport function unrelated() { return calculateTotal(); }\n",
        ),
        (
            "dynamic/adapter.ts",
            "export function invoke(receiver: { calculateTotal(): number }) { return receiver.calculateTotal(); }\n",
        ),
        (
            "docs/billing.md",
            "Billing consumers include both call sites and passed callbacks.\n",
        ),
        (
            "tools/invoice.py",
            "def calculate_total(lines):\n    return len(lines)\n",
        ),
    ] {
        fixture.write(path, source);
    }
    fixture
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn imported_aliases_and_callbacks_form_a_proven_consumer_plan_without_shadow_decoys() {
    let fixture = typescript_consumers_fixture();
    let mut args = vec![
        "consumers",
        ".",
        "--symbol",
        "billing/amount.ts",
        "calculateTotal",
        "--depth",
        "1",
        "--budget",
        "32768",
        "--max-output-bytes",
        "262144",
        "--no-project-config",
        "--no-cache",
    ];
    let direct = fixture.json(&args);
    // Python has explicitly unsupported call binding; prose has no captured call facts.
    assert_consumer_coverage(&direct, 6, 1, 1);
    assert_eq!(
        consumers(&direct),
        BTreeSet::from([
            ("checkout/service.ts", "checkout", 1),
            ("checkout/service.ts", "callback", 1),
            ("checkout/preview.ts", "preview", 1),
        ])
    );
    assert_eq!(direct["depth_omitted"], 1);
    assert!(
        array(&direct["unresolved"])
            .iter()
            .any(|item| item["reason"] == "shadowed-binding")
    );
    assert!(
        array(&direct["unresolved"])
            .iter()
            .any(|item| item["reason"] == "dynamic-receiver")
    );
    let callback = array(&direct["hits"])
        .iter()
        .find(|hit| hit["symbol"]["name"] == "callback")
        .unwrap();
    assert_eq!(array(&callback["evidence"]).len(), 1);
    let reference = &callback["evidence"][0];
    assert_eq!(reference["kind"], "reference");
    assert_eq!(reference["source"]["path"], "checkout/service.ts");
    assert_eq!(reference["source"]["name"], "callback");
    assert_eq!(reference["target"]["path"], "billing/amount.ts");
    assert_eq!(reference["target"]["name"], "calculateTotal");
    assert_eq!(reference["site"]["start_line"], 3);

    args[6] = "2";
    let transitive = fixture.json(&args);
    assert_consumer_coverage(&transitive, 6, 1, 1);
    let mut expected = consumers(&direct);
    expected.insert(("routes/checkout.ts", "route", 2));
    assert_eq!(consumers(&transitive), expected);
    assert_eq!(transitive["depth_omitted"], 0);

    let checkout = array(&direct["hits"])
        .iter()
        .find(|hit| hit["symbol"]["name"] == "checkout")
        .unwrap();
    let read = follow_read(&fixture, &checkout["read"]);
    assert_eq!(read["results"][0]["status"], "complete");
    assert_eq!(source_contents(&read), BTreeSet::from([CHECKOUT]));
    let target = &checkout["read"];
    let plan = fixture.json(&[
        "plan",
        ".",
        "--symbol",
        target["path"].as_str().unwrap(),
        target["selector"]["value"].as_str().unwrap(),
        "--expect-hash",
        target["path"].as_str().unwrap(),
        target["expected_hash"].as_str().unwrap(),
        "--source",
        "--budget",
        "16384",
        "--no-project-config",
        "--no-cache",
    ]);
    assert_eq!(plan["unavailable_seeds"], 0);
    assert_eq!(plan["unresolved_seeds"], 0);
    assert_eq!(plan["selected"].as_array().unwrap().len(), 1);
    assert_eq!(plan["selected"][0]["name"], "checkout");
    assert_eq!(plan["selected"][0]["role"], "direct");
    assert!(
        array(&plan["selected"][0]["environment_gaps"])
            .iter()
            .any(|gap| gap == "body-dependencies-not-expanded"),
        "a caller body is not a complete dependency closure"
    );
    assert_eq!(source_contents(&plan["source"]), BTreeSet::from([CHECKOUT]));
}

fn rust_consumers_fixture() -> Fixture {
    let fixture = Fixture::new("navigation-rust-consumers");
    for (path, source) in [
        (
            "Cargo.toml",
            "[package]\nname='ledger-scenario'\nversion='0.1.0'\nedition='2024'\n",
        ),
        (
            "src/lib.rs",
            "pub mod ledger;\npub mod checkout;\npub mod analytics;\npub mod app;\n",
        ),
        ("src/ledger.rs", "pub fn total() -> i32 { 42 }\n"),
        (
            "src/checkout.rs",
            "use crate::ledger::total as ledger_total;\npub fn by_alias() -> i32 { ledger_total() }\npub fn by_path() -> i32 { crate::ledger::total() }\npub fn shadowed(ledger_total: fn() -> i32) -> i32 { ledger_total() }\npub trait Price { fn total(&self) -> i32; }\npub fn receiver(item: &impl Price) -> i32 { item.total() }\n",
        ),
        (
            "src/analytics.rs",
            "pub fn total() -> i32 { -1 }\npub fn unrelated() -> i32 { total() }\n",
        ),
        (
            "src/app.rs",
            "pub fn launch() -> i32 { crate::checkout::by_path() }\n",
        ),
    ] {
        fixture.write(path, source);
    }
    fixture
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn rust_module_evidence_reaches_real_callers_without_global_name_or_receiver_guesses() {
    let fixture = rust_consumers_fixture();
    let report = fixture.json(&[
        "consumers",
        ".",
        "--symbol",
        "src/ledger.rs",
        "total",
        "--depth",
        "2",
        "--budget",
        "32768",
        "--max-output-bytes",
        "262144",
        "--no-project-config",
        "--no-cache",
    ]);
    assert_consumer_coverage(&report, 5, 0, 1);
    assert_eq!(
        consumers(&report),
        BTreeSet::from([
            ("src/checkout.rs", "by_alias", 1),
            ("src/checkout.rs", "by_path", 1),
            ("src/app.rs", "launch", 2),
        ])
    );
    assert_eq!(report["depth_omitted"], 0);
    for reason in ["shadowed-binding", "dynamic-receiver"] {
        assert!(
            array(&report["unresolved"])
                .iter()
                .any(|item| item["reason"] == reason),
            "{reason}: {report}"
        );
    }
    let by_path = array(&report["hits"])
        .iter()
        .find(|hit| hit["symbol"]["name"] == "by_path")
        .unwrap();
    assert!(array(&by_path["evidence"]).iter().any(|edge| {
        edge["target"]["path"] == "src/ledger.rs" && edge["syntax"] == "module-qualified"
    }));
    let read = follow_read(&fixture, &by_path["read"]);
    assert_eq!(read["results"][0]["status"], "complete");
    assert_eq!(
        source_contents(&read),
        BTreeSet::from(["pub fn by_path() -> i32 { crate::ledger::total() }"])
    );

    // The identically named analytics function has a different, independently proven caller.
    let decoy = fixture.json(&[
        "consumers",
        ".",
        "--symbol",
        "src/analytics.rs",
        "total",
        "--depth",
        "2",
        "--budget",
        "32768",
        "--no-project-config",
        "--no-cache",
    ]);
    assert_eq!(
        consumers(&decoy),
        BTreeSet::from([("src/analytics.rs", "unrelated", 1)])
    );
    assert_consumer_seed_identity(&fixture, &report);
}

fn assert_consumer_seed_identity(fixture: &Fixture, report: &Value) {
    let seed_hash = report["seeds"][0]["source_hash"].as_str().unwrap();
    let current = fixture.json(&[
        "consumers",
        ".",
        "--symbol",
        "src/ledger.rs",
        "total",
        "--depth",
        "2",
        "--expect-hash",
        "src/ledger.rs",
        seed_hash,
        "--budget",
        "32768",
        "--max-output-bytes",
        "262144",
        "--no-project-config",
        "--no-cache",
    ]);
    assert_eq!(
        &current, report,
        "matching hash must preserve the complete result"
    );
    fixture.write("src/ledger.rs", "pub fn total() -> i32 { 43 }\n");
    let stale = fixture
        .command(&[
            "consumers",
            ".",
            "--symbol",
            "src/ledger.rs",
            "total",
            "--expect-hash",
            "src/ledger.rs",
            seed_hash,
            "--error-format",
            "json",
            "--no-project-config",
            "--no-cache",
        ])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .clone();
    assert!(
        stale.stdout.is_empty(),
        "a stale seed must not expose a graph"
    );
    let error: Value = serde_json::from_slice(&stale.stderr).unwrap();
    assert_eq!(error["kind"], "error");
    assert_eq!(error["category"], "runtime");
    assert_eq!(error["exit_code"], 1);
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("hash mismatch"),
        "unexpected rejection: {error}"
    );
    assert!(
        message.contains("src/ledger.rs"),
        "missing stale file identity: {error}"
    );
}

fn package_graph_fixture() -> Fixture {
    let fixture = Fixture::new("navigation-package-impact");
    for (path, source) in [
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@prices/*":["packages/core/src/*"]}}}"#,
        ),
        (
            "packages/core/package.json",
            r#"{"name":"@scenario/core","exports":{"./price":"./src/price.js"}}"#,
        ),
        (
            "packages/api/package.json",
            r#"{"name":"@scenario/api","exports":{".":"./src/index.js"}}"#,
        ),
        (
            "packages/unrelated/package.json",
            r#"{"name":"@scenario/unrelated","exports":{".":"./src/index.js"}}"#,
        ),
        (
            "packages/core/src/round.ts",
            "export function round(value: number) { return value; }\n",
        ),
        (
            "packages/core/src/price.ts",
            "import { round } from './round.js';\nexport function price(value: number) { return round(value); }\n",
        ),
        (
            "packages/api/src/index.ts",
            "import { price } from '@scenario/core/price';\nexport function quote() { return price(10); }\n",
        ),
        (
            "apps/web/checkout.ts",
            "import { quote } from '@scenario/api';\nexport function checkout() { return quote(); }\n",
        ),
        (
            "apps/cli/main.ts",
            "import { price } from '@prices/price';\nexport function main() { return price(20); }\n",
        ),
        (
            "packages/unrelated/src/index.ts",
            "export function price() { return -1; }\n",
        ),
        (
            "apps/admin/index.ts",
            "import { price } from '@scenario/unrelated';\nexport function dashboard() { return price(); }\n",
        ),
        (
            "tests/price.test.ts",
            "import { price } from '../packages/core/src/price.js';\nexport function checksPrice() { return price(5) === 5; }\n",
        ),
    ] {
        fixture.write(path, source);
    }
    fixture.commit("baseline package relationships");
    fixture
}

fn assert_package_graph(graph: &Value) {
    assert_eq!(
        edges(graph),
        BTreeSet::from([
            (
                "packages/core/src/price.ts",
                "packages/core/src/round.ts",
                "relative"
            ),
            (
                "packages/api/src/index.ts",
                "packages/core/src/price.ts",
                "package-exports"
            ),
            (
                "apps/web/checkout.ts",
                "packages/api/src/index.ts",
                "package-exports"
            ),
            (
                "apps/cli/main.ts",
                "packages/core/src/price.ts",
                "tsconfig-paths"
            ),
            (
                "apps/admin/index.ts",
                "packages/unrelated/src/index.ts",
                "package-exports"
            ),
            (
                "tests/price.test.ts",
                "packages/core/src/price.ts",
                "relative"
            ),
        ])
    );
    assert_eq!(graph["graph"]["unresolved_imports"], 0);
    assert_eq!(
        graph["graph"]
            .get("parse_errors")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        0
    );
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn package_graph_to_impact_and_context_keeps_unchanged_consumers_but_scoped_metrics() {
    let fixture = package_graph_fixture();
    let graph = fixture.json(&[
        ".",
        "--graph",
        "--profile",
        "agent",
        "--no-project-config",
        "--no-cache",
    ]);
    assert_package_graph(&graph);
    fixture.write("packages/core/src/price.ts", "import { round } from './round.js';\nexport function price(value: number) { return round(value) + 1; }\n");
    let report = fixture.json(&[
        ".",
        "--working",
        "--impact",
        "--context",
        "--context-budget",
        "20000",
        "--context-max-files",
        "20",
        "--profile",
        "agent",
        "--no-project-config",
        "--no-cache",
    ]);
    assert_eq!(
        paths(&report["files"]),
        BTreeSet::from(["packages/core/src/price.ts"])
    );
    assert_eq!(report["summary"]["files"], 1);
    assert_eq!(
        strings(&report["impact"]["changed_files"]),
        BTreeSet::from(["packages/core/src/price.ts"])
    );
    assert_eq!(
        strings(&report["impact"]["direct_dependents"]),
        BTreeSet::from([
            "packages/api/src/index.ts",
            "apps/cli/main.ts",
            "tests/price.test.ts",
        ])
    );
    assert_eq!(
        strings(&report["impact"]["transitive_dependents"]),
        BTreeSet::from(["apps/web/checkout.ts"])
    );
    assert_eq!(report["impact"]["confidence"], "high");
    assert_eq!(
        report["context"]["planning_diagnostics"]["analyzed_files"],
        12
    );
    let role_paths = |role: &str| -> BTreeSet<&str> {
        array(&report["context"]["files"])
            .iter()
            .filter(|file| {
                file.get("evidence")
                    .and_then(Value::as_array)
                    .is_some_and(|evidence| evidence.iter().any(|item| item["role"] == role))
            })
            .map(|file| file["path"].as_str().unwrap())
            .collect()
    };
    assert_eq!(
        role_paths("dependency"),
        BTreeSet::from(["packages/core/src/round.ts"])
    );
    assert_eq!(
        role_paths("dependent"),
        BTreeSet::from([
            "packages/api/src/index.ts",
            "apps/cli/main.ts",
            "tests/price.test.ts",
            "apps/web/checkout.ts",
        ])
    );
    let web = array(&report["context"]["files"])
        .iter()
        .find(|file| file["path"] == "apps/web/checkout.ts")
        .unwrap();
    let evidence = array(&web["evidence"])
        .iter()
        .find(|evidence| evidence["role"] == "dependent")
        .unwrap();
    assert_eq!(evidence["distance"], 2);
    assert_eq!(evidence["confidence"], "partial");
    assert_eq!(report["work_scope"]["inventory"]["source_files"], 1);
    assert_eq!(report["work_scope"]["impact"]["direct_dependents"], 3);
    assert_eq!(report["work_scope"]["impact"]["transitive_dependents"], 1);
}

fn godot_fixture() -> Fixture {
    let fixture = Fixture::new("navigation-godot-projects");
    for (path, source) in [
        (
            "project.godot",
            "config_version=5\n[application]\nrun/main_scene=\"res://main.tscn\"\n[autoload]\nGlobalBank=\"*res://global_bank.gd\"\n",
        ),
        ("global_bank.gd", "extends Node\nvar score = 0\n"),
        (
            "shared/actor.gd",
            "class_name Actor\nextends Node\nfunc score():\n    return 1\n",
        ),
        ("shared/actor.gd.uid", "uid://actor1\n"),
        ("alternate_actor.gd", "extends Node\n"),
        (
            "main.tscn",
            "[gd_scene format=3]\n[ext_resource type=\"Script\" uid=\"uid://actor1\" path=\"res://alternate_actor.gd\" id=\"1\"]\n[ext_resource type=\"ShaderMaterial\" path=\"res://material.tres\" id=\"2\"]\n[node name=\"Main\" type=\"Node\"]\nscript=ExtResource(\"1\")\n",
        ),
        (
            "world.gd",
            "extends Actor\nfunc run():\n    GlobalBank.score += 1\n",
        ),
        (
            "decoys.gd",
            "extends Node\n# preload(\"res://shared/actor.gd\")\nvar label = \"res://shared/actor.gd\"\n@onready var actor = $Actor\nfunc run(Actor):\n    return Actor.new()\nfunc dynamic(path):\n    return load(path)\n",
        ),
        (
            "material.tres",
            "[gd_resource type=\"ShaderMaterial\" format=3]\n[ext_resource type=\"Shader\" path=\"res://effect.gdshader\" id=\"1\"]\n[resource]\nshader=ExtResource(\"1\")\n",
        ),
        (
            "effect.gdshader",
            "shader_type canvas_item;\n#include \"res://math.gdshaderinc\"\nvoid fragment() { COLOR = vec4(1.0); }\n",
        ),
        (
            "math.gdshaderinc",
            "float scale(float value) { return value * 2.0; }\n",
        ),
        (
            "nested/project.godot",
            "config_version=5\n[application]\nrun/main_scene=\"res://main.tscn\"\n",
        ),
        ("nested/actor.gd", "class_name Actor\nextends Node\n"),
        ("nested/actor.gd.uid", "uid://actor1\n"),
        (
            "nested/main.tscn",
            "[gd_scene format=3]\n[ext_resource type=\"Script\" uid=\"uid://actor1\" path=\"res://actor.gd\" id=\"1\"]\n[node name=\"Nested\" type=\"Node\"]\nscript=ExtResource(\"1\")\n",
        ),
        ("nested/world.gd", "extends Actor\n"),
    ] {
        fixture.write(path, source);
    }
    fixture
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn godot_resources_uids_and_globals_stay_in_their_project_and_ambiguity_stays_unknown() {
    let fixture = godot_fixture();
    let args = [
        ".",
        "--graph",
        "--profile",
        "agent",
        "--no-project-config",
        "--no-cache",
    ];
    let report = fixture.json(&args);
    let expected = BTreeSet::from([
        ("project.godot", "main.tscn", "godot-resource"),
        ("project.godot", "global_bank.gd", "godot-resource"),
        ("main.tscn", "shared/actor.gd", "godot-uid"),
        ("main.tscn", "material.tres", "godot-resource"),
        ("world.gd", "shared/actor.gd", "godot-global"),
        ("world.gd", "global_bank.gd", "godot-global"),
        ("material.tres", "effect.gdshader", "godot-resource"),
        ("effect.gdshader", "math.gdshaderinc", "godot-resource"),
        ("nested/project.godot", "nested/main.tscn", "godot-resource"),
        ("nested/main.tscn", "nested/actor.gd", "godot-uid"),
        ("nested/world.gd", "nested/actor.gd", "godot-global"),
    ]);
    assert_eq!(edges(&report), expected);
    assert_eq!(
        report["graph"]["unresolved_imports"], 1,
        "only load(path) is unresolved"
    );
    assert_eq!(
        report["graph"]
            .get("parse_errors")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        0
    );
    assert_eq!(report["diagnostics"]["unsupported_files"], 0);
    for path in [
        "project.godot",
        "main.tscn",
        "material.tres",
        "nested/project.godot",
        "nested/main.tscn",
    ] {
        let files: Vec<_> = array(&report["files"])
            .iter()
            .filter(|file| file["path"] == path)
            .collect();
        assert_eq!(files.len(), 1, "expected one inventory entry for {path}");
        let file = files[0];
        assert!(file["tokens"].as_u64().unwrap() > 0);
        assert!(file["complexity"].is_null());
    }

    // One ambiguous root-project UID must not choose the path fallback or poison the nested UID.
    fixture.write("alternate_actor.gd.uid", "uid://actor1\n");
    let ambiguous = fixture.json(&args);
    let mut after = expected;
    after.remove(&("main.tscn", "shared/actor.gd", "godot-uid"));
    assert_eq!(edges(&ambiguous), after);
    assert_eq!(ambiguous["graph"]["unresolved_imports"], 2);
    assert!(!fixture.path().join(".godot").exists());
    assert!(!fixture.path().join("nested/.godot").exists());
}

fn large_parser_source() -> String {
    let mut source =
        String::from("pub fn parse_record(mode: ParseMode) -> usize {\n    let mut total = 0;\n");
    for index in 0..240 {
        writeln!(source, "    if mode.strict {{ total += {index}; }}").unwrap();
    }
    source.push_str("    total\n}");
    source
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn definition_plans_distinguish_source_selection_from_whole_source_delivery() {
    let fixture = Fixture::new("navigation-plan-budgets");
    let parser = large_parser_source();
    let mode = "pub struct ParseMode { pub strict: bool }";
    let recover = "pub fn recover_record() -> usize { 17 }";
    fixture.write("codec/parser.rs", &format!("{mode}\n\n{parser}\n"));
    fixture.write("codec/recovery.rs", recover);
    fixture.write("codec/unrelated.rs", "pub fn unrelated() -> usize { 99 }\n");
    let base_args = [
        "plan",
        ".",
        "--symbol",
        "codec/parser.rs",
        "parse_record",
        "--symbol",
        "codec/recovery.rs",
        "recover_record",
        "--source",
        "--no-project-config",
        "--no-cache",
    ];
    let run = |context, budget, bytes| {
        let mut args = base_args.to_vec();
        args.extend([
            "--context-budget",
            context,
            "--budget",
            budget,
            "--max-output-bytes",
            bytes,
        ]);
        fixture.json(&args)
    };
    let selection_limited = run("64", "16384", "262144");
    assert_eq!(selection_limited["selected"].as_array().unwrap().len(), 1);
    assert_eq!(selection_limited["selected"][0]["name"], "recover_record");
    assert!(
        array(&selection_limited["omissions"])
            .iter()
            .any(|omission| {
                omission["name"] == "parse_record"
                    && omission["reason"] == "oversized-definition"
                    && omission["explicit"] == true
            })
    );
    assert_eq!(selection_limited["output_omitted"], 0);
    assert_eq!(
        source_contents(&selection_limited["source"]),
        BTreeSet::from([recover])
    );

    let full = run("60000", "32768", "262144");
    let expected = BTreeSet::from([parser.as_str(), mode, recover]);
    assert_eq!(source_contents(&full["source"]), expected);
    assert_eq!(full["omitted_definitions"], 0);
    assert_eq!(full["output_omitted"], 0);
    assert_eq!(full["selected"].as_array().unwrap().len(), 3);
    assert!(
        array(&full["selected"])
            .iter()
            .any(|item| item["name"] == "ParseMode" && item["role"] == "environment")
    );
    assert!(
        array(&full["source"]["results"])
            .iter()
            .all(|item| item["status"] == "complete")
    );

    let limited = run("60000", "1024", "4096");
    assert_eq!(limited["selected_tokens"], full["selected_tokens"]);
    assert_eq!(
        limited["omitted_definitions"], 0,
        "output admission must not rewrite planning facts"
    );
    assert_eq!(
        limited["source"]["requested_targets"],
        array(&limited["selected"]).len()
    );
    assert!(
        source_contents(&limited["source"]).is_subset(&expected),
        "a returned body must be complete"
    );
    assert!(
        !source_contents(&limited["source"]).contains(parser.as_str()),
        "the large body cannot fit 4096 bytes"
    );
    assert!(
        limited["output_omitted"].as_u64().unwrap() > 0
            || limited["source"]["omitted_targets"].as_u64().unwrap() > 0
            || array(&limited["source"]["results"])
                .iter()
                .any(|item| item["status"] == "budget-omitted")
    );
    let rendered = format!("{}\n", serde_json::to_string(&limited).unwrap());
    assert!(rendered.len() <= 4096);
    assert!(TokenCounter::new("o200k_base").unwrap().count(&rendered) <= 1024);
}
