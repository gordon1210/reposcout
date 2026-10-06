use super::{
    Journey, OutputBudget, ReadEvidence, ReviewDiscovery, ReviewTask, array,
    assert_capture_complete, assert_complete_definition, assert_complete_review, capture_review,
    number, read_discovery, text, tree_for,
};
use crate::support::Fixture;
use reposcout::metrics::tokens::TokenCounter;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

struct ProjectionRecovery {
    attempts: Vec<ReviewDiscovery>,
    remaining_omissions: BTreeMap<&'static str, u64>,
}

fn optional_number(value: &Value, key: &str) -> u64 {
    value.get(key).map_or(0, |value| {
        value
            .as_u64()
            .unwrap_or_else(|| panic!("malformed optional count {key}: {value}"))
    })
}

fn output_omissions(report: &Value) -> BTreeMap<&'static str, u64> {
    let mut gaps = BTreeMap::new();
    for key in [
        "changes_omitted",
        "definitions_omitted",
        "relations_omitted",
        "candidates_omitted",
    ] {
        let count = number(&report["totals"], key);
        if count > 0 {
            gaps.insert(key, count);
        }
    }
    let ranges = array(report, "changes")
        .iter()
        .flat_map(|change| ["base", "head"].map(|side| &change[side]))
        .filter(|side| !side.is_null())
        .map(|side| optional_number(side, "ranges_omitted"))
        .sum();
    if ranges > 0 {
        gaps.insert("ranges_omitted", ranges);
    }
    gaps
}

fn recover_projection(
    journey: &mut Journey<'_>,
    task: &ReviewTask<'_>,
    ceiling_tokens: usize,
    attempt_limit: usize,
) -> ProjectionRecovery {
    assert!((1..=3).contains(&attempt_limit));
    assert!((task.budget.tokens..=65_536).contains(&ceiling_tokens));
    let mut budget = task.budget;
    let mut attempts = Vec::new();
    loop {
        let attempt = capture_review(
            journey,
            &ReviewTask {
                base: task.base,
                head: task.head,
                budget,
            },
        );
        // Only rendered omissions can trigger more output allowance. Capture or
        // mapping gaps remain in the report and are never relabeled as recovered.
        let remaining_omissions = output_omissions(&attempt.report);
        attempts.push(attempt);
        if remaining_omissions.is_empty()
            || attempts.len() == attempt_limit
            || budget.tokens == ceiling_tokens
        {
            return ProjectionRecovery {
                attempts,
                remaining_omissions,
            };
        }
        budget.tokens = (budget.tokens * 4).min(ceiling_tokens);
    }
}

// Independent fixture/oracle: no path or expected source is passed to the recovery driver.
const GUARDED_GRANT: &str = "export function grant(balance: number, requested: number) {\n    if (requested > balance) return -1;\n    return balance - requested;\n}\n";
const UNGUARDED_GRANT: &str = "export function grant(balance: number, requested: number) {\n    return balance - requested;\n}\n";
type SourceIdentity = (String, String, String);
type ExpectedSources = BTreeMap<SourceIdentity, (String, String)>;

fn dense_source(value: u32) -> String {
    let mut source = String::new();
    for index in 0..20 {
        writeln!(source, "export function f{index}() {{ return {value}; }}").unwrap();
    }
    source
}

fn client_source(small: &str) -> String {
    format!(
        "import {{ grant }} from './{}';\nexport function spend() {{ return grant(10, 20); }}\n",
        small.strip_suffix(".ts").unwrap()
    )
}

fn budget_fixture(dense: &str, small: &str) -> (Fixture, String, String) {
    let fixture = Fixture::new("journey-review-output-recovery");
    fixture.write(dense, &dense_source(1));
    fixture.write(small, GUARDED_GRANT);
    fixture.write("client.ts", &client_source(small));
    fixture.write("unrelated.ts", "export function grant(balance: number, requested: number) { return balance + requested; }\n");
    let base = fixture.commit("dense maintenance surface and guarded allocation");
    fixture.write(dense, &dense_source(2));
    fixture.write(small, UNGUARDED_GRANT);
    let head = fixture.commit("update dense exports and remove one independent guard");
    (fixture, base, head)
}

fn expected_sources(report: &Value, dense: &str, small: &str) -> ExpectedSources {
    let mut expected = BTreeMap::new();
    for (side, value, grant) in [("base", 1, GUARDED_GRANT), ("head", 2, UNGUARDED_GRANT)] {
        let tree = tree_for(report, side);
        let whole_dense = dense_source(value);
        for index in 0..20 {
            expected.insert(
                (tree.to_owned(), dense.to_owned(), format!("f{index}")),
                (
                    whole_dense.clone(),
                    format!("export function f{index}() {{ return {value}; }}"),
                ),
            );
        }
        expected.insert(
            (tree.to_owned(), small.to_owned(), "grant".to_owned()),
            (grant.to_owned(), grant.to_owned()),
        );
        let client = client_source(small);
        let declaration = client.lines().last().unwrap().to_owned();
        expected.insert(
            (tree.to_owned(), "client.ts".to_owned(), "spend".to_owned()),
            (client, declaration),
        );
    }
    assert_eq!(
        expected.len(),
        44,
        "twenty exports, guard and caller on each revision"
    );
    expected
}

fn assert_projection_accounting(report: &Value, dense: &str, small: &str) {
    assert_capture_complete(report);
    let totals = &report["totals"];
    for key in [
        "changes_not_analyzed",
        "changes_without_hunks",
        "unknown_candidate_sizes",
    ] {
        assert_eq!(number(totals, key), 0, "{key}: {report}");
    }
    for (list, total) in [("changes", 2), ("relations", 4), ("candidates", 6)] {
        let records = if list == "candidates" {
            "context"
        } else {
            list
        };
        assert_eq!(number(totals, list), total);
        assert_eq!(
            array(report, records).len() as u64 + number(totals, &format!("{list}_omitted")),
            total
        );
    }
    let identities: BTreeSet<_> = array(report, "changes")
        .iter()
        .map(|change| {
            assert_eq!(text(change, "status"), "modified");
            assert_eq!(text(&change["base"], "path"), text(&change["head"], "path"));
            text(&change["base"], "path")
        })
        .collect();
    assert_eq!(
        identities,
        BTreeSet::from([dense, small]),
        "compact independent change identities survive projection"
    );
    let mut shown = 0;
    let mut omitted = 0;
    for change in array(report, "changes") {
        for side in ["base", "head"] {
            let file = &change[side];
            assert_eq!(text(file, "mapping_status"), "available");
            let definitions = array(file, "definitions");
            shown += definitions.len() as u64;
            omitted += optional_number(file, "definitions_omitted");
            if text(file, "path") == small {
                assert_eq!(
                    definitions.len(),
                    1,
                    "tiny independent declaration is retained"
                );
                assert_eq!(text(&definitions[0]["symbol"], "name"), "grant");
            }
        }
    }
    assert_eq!(number(totals, "definitions"), 42);
    assert_eq!(number(totals, "definitions_omitted"), omitted);
    assert_eq!(shown + omitted, 42);
}

fn assert_source_costs(report: &Value, small: &str) {
    let client = client_source(small);
    let sources = [
        dense_source(1),
        dense_source(2),
        GUARDED_GRANT.to_owned(),
        UNGUARDED_GRANT.to_owned(),
        client.clone(),
        client,
    ];
    let counter = TokenCounter::new("o200k_base").unwrap();
    let bytes: usize = sources.iter().map(String::len).sum();
    let tokens: usize = sources.iter().map(|source| counter.count(source)).sum();
    assert_eq!(number(&report["totals"], "candidate_bytes"), bytes as u64);
    assert_eq!(number(&report["totals"], "candidate_tokens"), tokens as u64);
    assert_eq!(number(&report["totals"], "selected_files"), 0);
    assert_eq!(number(&report["totals"], "selected_tokens"), 0);
}

fn assert_budget_deliveries(
    evidence: &ReadEvidence,
    expected: &ExpectedSources,
) -> BTreeSet<SourceIdentity> {
    let mut requested = BTreeSet::new();
    let mut delivered = BTreeSet::new();
    let mut withheld = 0;
    for outcome in &evidence.outcomes {
        let key = outcome.target.key();
        assert!(requested.insert(key.clone()), "duplicate read target");
        let (whole_file, declaration) = expected
            .get(&key)
            .unwrap_or_else(|| panic!("unrelated response-derived target: {key:?}"));
        if let Some(result) = &outcome.result {
            match text(result, "status") {
                "complete" => {
                    assert_complete_definition(outcome, whole_file, declaration);
                    delivered.insert(key);
                    continue;
                }
                "budget-omitted" => {}
                status => panic!("unexpected read gap {status}: {result}"),
            }
        }
        withheld += 1;
        assert!(
            outcome.source.is_none(),
            "withheld target has no partial source"
        );
    }
    assert_eq!(
        evidence.omitted, withheld,
        "every withheld target is accounted separately from review projection"
    );
    delivered
}

fn assert_recovered_review(
    journey: &mut Journey<'_>,
    recovered: &ProjectionRecovery,
    dense: &str,
    small: &str,
) {
    assert!(recovered.remaining_omissions.is_empty());
    assert!(
        (2..=3).contains(&recovered.attempts.len()),
        "real output pressure triggers a bounded follow-up"
    );
    let mut previous_tokens: Option<usize> = None;
    for attempt in &recovered.attempts {
        assert_projection_accounting(&attempt.report, dense, small);
        assert_source_costs(&attempt.report, small);
        if let Some(previous) = previous_tokens {
            assert_eq!(attempt.budget.tokens, (previous * 4).min(65_536));
        }
        previous_tokens = Some(attempt.budget.tokens);
    }
    let first = &recovered.attempts[0];
    assert!(number(&first.report["totals"], "definitions_omitted") > 0);
    let final_attempt = recovered.attempts.last().unwrap();
    assert_complete_review(&final_attempt.report);
    let expected = expected_sources(&final_attempt.report, dense, small);
    assert_eq!(final_attempt.targets.len(), 44);
    let evidence = read_discovery(journey, final_attempt);
    let delivered = assert_budget_deliveries(&evidence, &expected);
    assert_eq!(
        delivered,
        expected.into_keys().collect(),
        "full recovery reaches every authored definition and the unchanged caller"
    );
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn pr_budget_recovery_follows_reported_omissions_without_losing_small_changes() {
    for (dense, small) in [("a.ts", "z.ts"), ("z.ts", "a.ts")] {
        let (fixture, base, head) = budget_fixture(dense, small);
        let mut journey = Journey::new(&fixture);
        let task = ReviewTask {
            base: &base,
            head: &head,
            budget: OutputBudget {
                tokens: 4096,
                bytes: 65_536,
            },
        };
        let recovered = recover_projection(&mut journey, &task, 65_536, 3);
        assert_recovered_review(&mut journey, &recovered, dense, small);

        // A caller granting no additional budget gets an explicit terminal gap,
        // not fixture-known paths injected into a supposedly complete handoff.
        let saturated = recover_projection(&mut journey, &task, 4096, 1);
        assert_eq!(saturated.attempts.len(), 1);
        assert!(!saturated.remaining_omissions.is_empty());
        let partial = &saturated.attempts[0];
        assert_eq!(partial.report, recovered.attempts[0].report);
        let visible_definitions: u64 = array(&partial.report, "changes")
            .iter()
            .flat_map(|change| ["base", "head"].map(|side| &change[side]))
            .map(|side| array(side, "definitions").len() as u64)
            .sum();
        let missing_definitions = 42 - visible_definitions;
        assert!(missing_definitions > 0);
        assert_eq!(
            saturated
                .remaining_omissions
                .get("definitions_omitted")
                .copied(),
            Some(missing_definitions),
            "terminal state preserves the independently counted missing declarations"
        );
        assert!(partial.targets.len() < 44);
        let evidence = read_discovery(&mut journey, partial);
        let expected = expected_sources(&partial.report, dense, small);
        let delivered = assert_budget_deliveries(&evidence, &expected);
        assert!(
            !delivered.is_empty(),
            "reported targets still provide useful complete source"
        );
        assert!(
            delivered.len() < expected.len(),
            "remaining projection gaps remain real missing context"
        );
    }
}
