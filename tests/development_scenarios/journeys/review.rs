//! Response-driven PR preparation. The driver has revision/budget inputs only;
//! fixture-specific source expectations live in the oracle below it.

use super::support::Journey;
use crate::support::Fixture;
use reposcout::metrics::tokens::TokenCounter;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::symlink;

mod budget;
mod migration;

#[derive(Clone, Copy)]
struct OutputBudget {
    tokens: usize,
    bytes: usize,
}

struct ReviewTask<'a> {
    base: &'a str,
    head: &'a str,
    budget: OutputBudget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReadTarget {
    tree: String,
    path: String,
    symbol: String,
    kind: String,
    hash: String,
    declaration: Value,
}

impl ReadTarget {
    fn key(&self) -> (String, String, String) {
        (self.tree.clone(), self.path.clone(), self.symbol.clone())
    }
}

struct ReviewDiscovery {
    report: Value,
    targets: Vec<ReadTarget>,
    budget: OutputBudget,
}

struct ReadOutcome {
    target: ReadTarget,
    result: Option<Value>,
    file: Option<Value>,
    source: Option<Value>,
}

struct ReadEvidence {
    outcomes: Vec<ReadOutcome>,
    omitted: u64,
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing string {key}: {value}"))
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("missing array {key}: {value}"))
}

fn number(value: &Value, key: &str) -> u64 {
    value[key]
        .as_u64()
        .unwrap_or_else(|| panic!("missing nonnegative integer {key}: {value}"))
}

fn assert_rendered_budget(stdout: &[u8], report: &Value, budget: OutputBudget) {
    // The hermetic fixture keeps the default encoder; it is never chosen from a model.
    assert_eq!(text(report, "encoding"), "o200k_base");
    assert_eq!(number(report, "token_budget"), budget.tokens as u64);
    assert_eq!(number(report, "byte_budget"), budget.bytes as u64);
    assert!(stdout.len() <= budget.bytes);
    let counter = TokenCounter::new(text(report, "encoding")).unwrap();
    let rendered = std::str::from_utf8(stdout).expect("JSON is valid UTF-8");
    assert!(
        counter.count(rendered) <= budget.tokens,
        "complete stdout, including newline, fits the token budget"
    );
}

fn capture_review(journey: &mut Journey<'_>, task: &ReviewTask<'_>) -> ReviewDiscovery {
    let tokens = task.budget.tokens.to_string();
    let bytes = task.budget.bytes.to_string();
    let step = journey.step(
        "discover revision-pinned review evidence",
        &[
            "review-context",
            ".",
            "--base",
            task.base,
            "--head",
            task.head,
            "--budget",
            &tokens,
            "--max-output-bytes",
            &bytes,
            "--no-cache",
            "--no-project-config",
            "-f",
            "json",
            "--quiet",
        ],
        0,
    );
    let report = step.stdout_json();
    assert_rendered_budget(step.stdout_bytes(), &report, task.budget);
    assert_eq!(text(&report, "kind"), "review_context");
    assert_eq!(text(&report["comparison"], "mode"), "direct");
    assert!(report.get("sources").is_none());
    for change in array(&report, "changes") {
        assert!(change.get("diff").is_none(), "default review leaked a diff");
    }
    for candidate in array(&report, "context") {
        assert!(
            candidate.get("source").is_none(),
            "default review leaked source"
        );
    }
    let targets = discover_targets(&report);
    ReviewDiscovery {
        report,
        targets,
        budget: task.budget,
    }
}

fn tree_for<'a>(report: &'a Value, side: &str) -> &'a str {
    let field = match side {
        "base" => "base_tree",
        "head" => "head_tree",
        _ => panic!("unknown review side {side}"),
    };
    let tree = text(&report["comparison"], field);
    assert_eq!(tree.len(), 40, "expected a pinned SHA-1 Git tree: {tree}");
    assert!(tree.bytes().all(|byte| byte.is_ascii_hexdigit()));
    tree
}

fn insert_target(targets: &mut BTreeMap<(String, String, String), ReadTarget>, target: ReadTarget) {
    assert_eq!(
        target.hash.len(),
        64,
        "invalid reported SHA-256: {target:?}"
    );
    assert!(target.hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(number(&target.declaration, "start_byte") < number(&target.declaration, "end_byte"));
    if let Some(previous) = targets.get(&target.key()) {
        // The same declaration can be both changed and a relation endpoint.
        assert_eq!(
            previous, &target,
            "conflicting evidence for one read identity"
        );
    } else {
        targets.insert(target.key(), target);
    }
}

fn discover_targets(report: &Value) -> Vec<ReadTarget> {
    let mut targets = BTreeMap::new();
    let mut changed_identities = BTreeSet::new();
    for change in array(report, "changes") {
        for side in ["base", "head"] {
            let file = change
                .get(side)
                .expect("change has explicit revision sides");
            if file.is_null() {
                continue;
            }
            let definitions = array(file, "definitions");
            if definitions.is_empty() {
                continue;
            }
            assert_eq!(text(file, "status"), "captured");
            for definition in definitions {
                let target = ReadTarget {
                    tree: tree_for(report, side).to_owned(),
                    path: text(file, "path").to_owned(),
                    symbol: text(&definition["symbol"], "name").to_owned(),
                    kind: text(&definition["symbol"], "kind").to_owned(),
                    hash: text(file, "sha256").to_owned(),
                    declaration: definition["declaration_span"].clone(),
                };
                assert!(
                    changed_identities.insert(target.key()),
                    "duplicate changed definition"
                );
                insert_target(&mut targets, target);
            }
        }
    }
    let mut references = BTreeSet::new();
    for relation in array(report, "relations") {
        if text(relation, "kind") != "symbol-reference" {
            continue;
        }
        let side = text(relation, "side");
        let reference = &relation["symbol"];
        assert!(
            references.insert((side, reference.to_string())),
            "duplicate reference evidence"
        );
        for endpoint in ["source", "target"] {
            let symbol = &reference[endpoint];
            assert_eq!(text(symbol, "path"), text(&relation["edge"], endpoint));
            insert_target(
                &mut targets,
                ReadTarget {
                    tree: tree_for(report, side).to_owned(),
                    path: text(symbol, "path").to_owned(),
                    symbol: text(symbol, "name").to_owned(),
                    kind: text(symbol, "kind").to_owned(),
                    hash: text(symbol, "source_hash").to_owned(),
                    declaration: symbol["declaration_span"].clone(),
                },
            );
        }
    }
    targets.into_values().collect()
}

fn read_arguments(targets: &[&ReadTarget], budget: OutputBudget) -> Vec<String> {
    let mut args = vec![
        "read".to_owned(),
        ".".to_owned(),
        "--snapshot".to_owned(),
        targets[0].tree.clone(),
        "--budget".to_owned(),
        budget.tokens.to_string(),
        "--max-output-bytes".to_owned(),
        budget.bytes.to_string(),
    ];
    let mut hashes = BTreeMap::new();
    for target in targets {
        assert_eq!(
            target.tree, targets[0].tree,
            "a read batch has one snapshot"
        );
        args.extend([
            "--symbol".to_owned(),
            target.path.clone(),
            target.symbol.clone(),
        ]);
        if let Some(previous) = hashes.insert(target.path.as_str(), target.hash.as_str()) {
            assert_eq!(previous, target.hash, "conflicting reported file hashes");
        }
    }
    for (path, hash) in hashes {
        args.extend(["--expect-hash".to_owned(), path.to_owned(), hash.to_owned()]);
    }
    args.extend(["--no-cache", "--no-project-config", "-f", "json", "--quiet"].map(str::to_owned));
    args
}

fn records_by_id<'a>(records: &'a [Value], key: &str) -> BTreeMap<u64, &'a Value> {
    let mut indexed = BTreeMap::new();
    for record in records {
        assert!(
            indexed.insert(number(record, key), record).is_none(),
            "duplicate {key}: {record}"
        );
    }
    indexed
}

fn link_outcomes(report: &Value, targets: &[&ReadTarget]) -> Vec<ReadOutcome> {
    assert_eq!(text(report, "kind"), "source_query");
    assert_eq!(text(report, "mode"), "source");
    assert_eq!(number(report, "requested_targets"), targets.len() as u64);
    let results = records_by_id(array(report, "results"), "target");
    let files = records_by_id(array(report, "files"), "id");
    let chunks = report.get("sources").map_or(&[][..], |value| {
        value
            .as_array()
            .expect("sources, when present, is an array")
    });
    let sources = records_by_id(chunks, "id");
    let referenced_sources: BTreeSet<_> = results
        .values()
        .filter_map(|result| result.get("source"))
        .map(|id| id.as_u64().expect("source reference is numeric"))
        .collect();
    assert_eq!(
        sources.keys().copied().collect::<BTreeSet<_>>(),
        referenced_sources,
        "source chunks must belong to explicit target results"
    );
    assert!(
        results
            .keys()
            .all(|id| *id > 0 && *id <= targets.len() as u64)
    );
    targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            let result = results.get(&(index as u64 + 1)).copied();
            let file = result.and_then(|result| result.get("file")).map(|id| {
                (*files
                    .get(&id.as_u64().expect("file reference is numeric"))
                    .expect("referenced file exists"))
                .clone()
            });
            let source = result.and_then(|result| result.get("source")).map(|id| {
                (*sources
                    .get(&id.as_u64().expect("source reference is numeric"))
                    .expect("referenced source exists"))
                .clone()
            });
            if let Some(file) = &file {
                assert_eq!(text(file, "path"), target.path);
                assert_eq!(text(&file["snapshot"], "kind"), "tree");
                assert_eq!(text(&file["snapshot"], "revision"), target.tree);
            }
            if let Some(source) = &source {
                assert_eq!(number(source, "file"), number(file.as_ref().unwrap(), "id"));
            }
            ReadOutcome {
                target: (*target).clone(),
                result: result.cloned(),
                file,
                source,
            }
        })
        .collect()
}

fn read_targets(
    journey: &mut Journey<'_>,
    targets: &[ReadTarget],
    budget: OutputBudget,
) -> ReadEvidence {
    let mut by_tree: BTreeMap<&str, Vec<&ReadTarget>> = BTreeMap::new();
    for target in targets {
        by_tree.entry(&target.tree).or_default().push(target);
    }
    let mut evidence = ReadEvidence {
        outcomes: Vec::new(),
        omitted: 0,
    };
    for targets in by_tree.values() {
        for batch in targets.chunks(32) {
            let arguments = read_arguments(batch, budget);
            let argv: Vec<_> = arguments.iter().map(String::as_str).collect();
            let step = journey.step(
                "read discovered definitions from their pinned trees",
                &argv,
                0,
            );
            let report = step.stdout_json();
            assert_rendered_budget(step.stdout_bytes(), &report, budget);
            evidence.omitted += number(&report, "omitted_targets");
            evidence.outcomes.extend(link_outcomes(&report, batch));
        }
    }
    evidence
}

fn read_discovery(journey: &mut Journey<'_>, discovery: &ReviewDiscovery) -> ReadEvidence {
    read_targets(journey, &discovery.targets, discovery.budget)
}

// Everything below this boundary is fixture setup or an independent semantic oracle.
// Complete syntax evidence makes a review possible; it does not establish that an agent found a bug.
const GUARDED_DEBIT: &str = "pub fn debit(balance: i64, amount: i64) -> i64 {\n    assert!(balance >= amount);\n    balance - amount\n}";
const UNGUARDED_DEBIT: &str =
    "pub fn debit(balance: i64, amount: i64) -> i64 {\n    balance - amount\n}";
const CREDIT: &str = "pub fn credit(balance: i64, amount: i64) -> i64 { balance + amount }";
const CHARGE: &str = "pub fn charge() -> i64 { crate::ledger::debit(10, 15) }\n";
const SETTLE: &str = "pub fn settle() -> i64 { crate::ledger::debit(40, 5) }\n";

fn payment_fixture() -> (Fixture, String, String) {
    let fixture = Fixture::new("journey-review-payment-guard");
    fixture.write(
        "Cargo.toml",
        "[package]\nname = \"payments\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    fixture.write("src/lib.rs", "pub mod ledger;\npub mod checkout;\npub mod settlement;\npub mod reporting;\npub mod training;\n");
    fixture.write("src/ledger.rs", &format!("{GUARDED_DEBIT}\n{CREDIT}\n"));
    fixture.write("src/checkout.rs", CHARGE);
    fixture.write("src/settlement.rs", SETTLE);
    fixture.write(
        "src/reporting.rs",
        "pub fn incoming() -> i64 { crate::ledger::credit(30, 7) }\n",
    );
    fixture.write("src/training.rs", "fn debit(balance: i64, amount: i64) -> i64 { balance - amount }\npub fn example() -> i64 { debit(100, 5) }\n");
    let base = fixture.commit("reject overdrafts before reducing the balance");
    fixture.write("src/ledger.rs", &format!("{UNGUARDED_DEBIT}\n{CREDIT}\n"));
    let head = fixture.commit("remove overdraft guard while keeping both callers");
    (fixture, base, head)
}

fn poison_live_payment_sources(fixture: &Fixture) {
    fixture.write("src/ledger.rs", "pub fn INDEX_POISON() -> i64 { 999 }\n");
    fixture.remove("src/checkout.rs");
    fixture.stage_all();
    fixture.write(
        "src/ledger.rs",
        "pub fn WORKTREE_POISON() -> i64 { 1000 }\n",
    );
    fixture.write(
        "src/checkout.rs",
        "pub fn WORKTREE_CALLER() -> i64 { -123 }\n",
    );
    // Keep the moved files with the fixture's optional failure artifacts.
    let moved = fixture.state_path().join("moved-source");
    fs::rename(fixture.path().join("src"), &moved).unwrap();
    symlink(&moved, fixture.path().join("src")).unwrap();
}

fn hash(source: &str) -> String {
    let mut hash = String::new();
    for byte in Sha256::digest(source.as_bytes()) {
        write!(hash, "{byte:02x}").unwrap();
    }
    hash
}

fn assert_complete_review(report: &Value) {
    for key in [
        "changes_not_analyzed",
        "changes_without_hunks",
        "changes_omitted",
        "definitions_omitted",
        "relations_omitted",
        "candidates_omitted",
        "unknown_candidate_sizes",
    ] {
        assert_eq!(number(&report["totals"], key), 0, "{key}: {report}");
    }
    assert_capture_complete(report);
}

fn assert_capture_complete(report: &Value) {
    let coverage_trees: BTreeSet<_> = array(report, "coverage")
        .iter()
        .map(|coverage| text(coverage, "tree"))
        .collect();
    assert_eq!(array(report, "coverage").len(), 2);
    assert_eq!(
        coverage_trees.len(),
        2,
        "each revision has distinct coverage"
    );
    assert_eq!(
        coverage_trees,
        BTreeSet::from([tree_for(report, "base"), tree_for(report, "head")])
    );
    for coverage in array(report, "coverage") {
        assert_eq!(coverage["inventory_truncated"].as_bool(), Some(false));
        assert_eq!(number(coverage, "parse_errors"), 0);
        assert_eq!(number(coverage, "config_errors"), 0);
        assert_eq!(number(coverage, "incomplete_call_files"), 0);
        assert_eq!(number(&coverage["call_resolution"], "omitted"), 0);
        assert!(
            coverage["unavailable_files"]
                .as_object()
                .unwrap()
                .is_empty()
        );
    }
}

fn assert_guard_discovery(discovery: &ReviewDiscovery, counterpart_side: &str) {
    let report = &discovery.report;
    assert_complete_review(report);
    assert_eq!(number(&report["totals"], "changes"), 1);
    assert_eq!(number(&report["totals"], "definitions"), 2);
    assert_eq!(array(report, "changes").len(), 1);
    let change = &report["changes"][0];
    assert_eq!(text(change, "status"), "modified");
    for side in ["base", "head"] {
        let file = &change[side];
        assert_eq!(text(file, "path"), "src/ledger.rs");
        assert_eq!(text(file, "mapping_status"), "available");
        let definitions = array(file, "definitions");
        assert_eq!(definitions.len(), 1);
        assert_eq!(text(&definitions[0]["symbol"], "name"), "debit");
        if side == counterpart_side {
            assert_eq!(number(file, "counterpart_definitions"), 1);
            assert_eq!(array(file, "ranges"), &[] as &[Value]);
        }
        let references: Vec<_> = array(report, "relations")
            .iter()
            .filter(|relation| {
                text(relation, "side") == side && text(relation, "kind") == "symbol-reference"
            })
            .collect();
        let callers: BTreeSet<_> = references
            .iter()
            .map(|relation| {
                assert_eq!(text(relation, "change_basis"), "changed-definition");
                let reference = &relation["symbol"];
                assert_eq!(text(reference, "kind"), "call");
                assert_eq!(text(&reference["target"], "path"), "src/ledger.rs");
                assert_eq!(text(&reference["target"], "name"), "debit");
                assert_eq!(number(&reference["site"], "start_line"), 1);
                (
                    text(&reference["source"], "path"),
                    text(&reference["source"], "name"),
                )
            })
            .collect();
        assert_eq!(
            references.len(),
            callers.len(),
            "duplicate callers on {side}"
        );
        assert_eq!(
            callers,
            BTreeSet::from([
                ("src/checkout.rs", "charge"),
                ("src/settlement.rs", "settle")
            ])
        );
    }
    assert_eq!(
        discovery.targets.len(),
        6,
        "two changed definitions and four caller sides"
    );
}

fn assert_complete_definition(outcome: &ReadOutcome, whole_file: &str, definition: &str) {
    let result = outcome.result.as_ref().expect("target result retained");
    assert_eq!(
        text(result, "status"),
        "complete",
        "{}",
        outcome.target.path
    );
    let file = outcome.file.as_ref().expect("target file retained");
    let source = outcome
        .source
        .as_ref()
        .expect("complete definition source retained");
    assert_eq!(text(file, "extraction"), "available");
    assert_eq!(text(file, "sha256"), hash(whole_file));
    assert_eq!(text(file, "sha256"), outcome.target.hash);
    assert_eq!(text(&result["definition"], "name"), outcome.target.symbol);
    assert_eq!(text(&result["definition"], "kind"), outcome.target.kind);
    assert_eq!(
        result["definition"]["declaration_span"],
        outcome.target.declaration
    );
    assert_eq!(
        text(source, "content").trim_end_matches('\n'),
        definition.trim_end_matches('\n')
    );
}

fn assert_payment_evidence(evidence: &ReadEvidence, guarded_tree: &str, unguarded_tree: &str) {
    assert_eq!(evidence.omitted, 0);
    let mut observed = BTreeSet::new();
    for outcome in &evidence.outcomes {
        assert!(
            observed.insert(outcome.target.key()),
            "duplicate delivered target"
        );
        match (outcome.target.path.as_str(), outcome.target.symbol.as_str()) {
            ("src/ledger.rs", "debit") => {
                let definition = if outcome.target.tree == guarded_tree {
                    GUARDED_DEBIT
                } else {
                    assert_eq!(outcome.target.tree, unguarded_tree);
                    UNGUARDED_DEBIT
                };
                assert_complete_definition(
                    outcome,
                    &format!("{definition}\n{CREDIT}\n"),
                    definition,
                );
            }
            ("src/checkout.rs", "charge") => assert_complete_definition(outcome, CHARGE, CHARGE),
            ("src/settlement.rs", "settle") => assert_complete_definition(outcome, SETTLE, SETTLE),
            _ => panic!(
                "unrelated symbol injected into review handoff: {:?}",
                outcome.target
            ),
        }
    }
    let expected: BTreeSet<_> = [guarded_tree, unguarded_tree]
        .into_iter()
        .flat_map(|tree| {
            [
                ("src/ledger.rs", "debit"),
                ("src/checkout.rs", "charge"),
                ("src/settlement.rs", "settle"),
            ]
            .map(|(path, symbol)| (tree.to_owned(), path.to_owned(), symbol.to_owned()))
        })
        .collect();
    assert_eq!(
        observed, expected,
        "review must deliver both revisions and unchanged callers"
    );
}

fn assert_stale_control(journey: &mut Journey<'_>, discovery: &ReviewDiscovery) {
    let mut wrong = discovery
        .targets
        .first()
        .expect("nonempty discovery")
        .clone();
    let replacement = if wrong.hash.starts_with('0') {
        "1"
    } else {
        "0"
    };
    wrong.hash.replace_range(..1, replacement);
    let evidence = read_targets(journey, &[wrong], discovery.budget);
    assert_eq!(evidence.omitted, 0);
    assert_eq!(evidence.outcomes.len(), 1);
    let outcome = &evidence.outcomes[0];
    assert_eq!(text(outcome.result.as_ref().unwrap(), "status"), "stale");
    assert!(
        outcome.source.is_none(),
        "stale identity must never deliver source"
    );
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn pr_guard_review_discovers_callers_and_reads_pinned_evidence_after_worktree_changes() {
    let (fixture, base, head) = payment_fixture();
    let mut journey = Journey::new(&fixture);
    let task = ReviewTask {
        base: &base,
        head: &head,
        budget: OutputBudget {
            tokens: 32_768,
            bytes: 262_144,
        },
    };
    let discovery = capture_review(&mut journey, &task);
    assert_guard_discovery(&discovery, "head");
    assert_eq!(text(&discovery.report["comparison"], "base_commit"), base);
    assert_eq!(text(&discovery.report["comparison"], "head_commit"), head);
    let guarded_tree = tree_for(&discovery.report, "base");
    let unguarded_tree = tree_for(&discovery.report, "head");

    // Only the test world can mutate files. The reader still receives nothing but
    // identities selected from the earlier CLI response, including old revision hashes.
    poison_live_payment_sources(&fixture);
    let index_before = fs::read(fixture.path().join(".git/index")).unwrap();
    let evidence = read_discovery(&mut journey, &discovery);
    assert_payment_evidence(&evidence, guarded_tree, unguarded_tree);
    let repeated = capture_review(&mut journey, &task);
    assert_eq!(repeated.report, discovery.report);

    let reverse_task = ReviewTask {
        base: &head,
        head: &base,
        budget: task.budget,
    };
    let reverse = capture_review(&mut journey, &reverse_task);
    assert_guard_discovery(&reverse, "base");
    assert_eq!(tree_for(&reverse.report, "base"), unguarded_tree);
    assert_eq!(tree_for(&reverse.report, "head"), guarded_tree);
    assert_payment_evidence(
        &read_discovery(&mut journey, &reverse),
        guarded_tree,
        unguarded_tree,
    );
    assert_stale_control(&mut journey, &discovery);
    assert_eq!(
        fs::read(fixture.path().join(".git/index")).unwrap(),
        index_before
    );
}
