use super::support::{Assessment, CostLedger, EvidenceFragment, Limits, PacketCost, PhaseBudget};
use crate::journeys::support::Journey;
use serde_json::{Value, json};

mod driver;
mod evaluator;
mod fixture;

use driver::Task;
use evaluator::{Evaluation, assert_packet_sensitivity, evaluate};
use fixture::{InvoiceCase, Variant};

struct Episode {
    label: &'static str,
    case: InvoiceCase,
    truth: Value,
    task: Task,
    evaluation: Evaluation,
    costs: Assessment,
}

impl Episode {
    fn json(&self) -> Value {
        json!({"label": self.label, "task": self.task, "independent_truth": self.truth,
            "evidence": self.evaluation.json(), "costs": self.costs})
    }

    fn positive_failures(&self) -> Vec<String> {
        self.evaluation
            .missing
            .iter()
            .map(|item| format!("missing obligation: {item}"))
            .chain(
                self.evaluation
                    .wrong_identity
                    .iter()
                    .map(|item| format!("wrong identity: {item}")),
            )
            .chain(
                self.evaluation
                    .cli_errors
                    .iter()
                    .map(|item| format!("CLI error: {item}")),
            )
            .chain(
                self.costs
                    .excesses
                    .iter()
                    .map(|item| format!("cost excess: {item}")),
            )
            .chain(
                self.costs
                    .accounting_gaps
                    .iter()
                    .map(|item| format!("accounting gap: {item}")),
            )
            .collect()
    }
}

fn episode(label: &'static str, variant: Variant, encoding: &'static str) -> Episode {
    let case = InvoiceCase::new(label, variant);
    assert_packet_sensitivity(&case);
    let truth = case.assert_truth();
    let fragments: Vec<_> = case
        .required
        .iter()
        .map(|piece| EvidenceFragment {
            path: piece.fragment.path,
            source: piece.fragment.source,
        })
        .collect();
    let packet = PacketCost::new(encoding, &fragments);
    assert!(packet.nonblank_lines <= 42 && packet.escaped_source_bytes <= 3 * 1024);
    assert!(packet.paths.len() <= 4);
    let limits = Limits {
        calls: 4,
        response_bytes: 9 * 1024,
        response_tokens: packet.tokens + 2400,
        source_paths: 4,
        source_nonblank_lines: 56,
        source_tokens: None,
        body_nonblank_lines: None,
    };
    let task = Task {
        description: "Why does src/invoices.py::net_due charge 905 cents for a 1005-cent invoice with a 1000-basis-point discount instead of 904? Discounts round to the nearest cent, halves upward. Supply the current change site and a genuine production regression check.",
        file: "src/invoices.py",
        symbol: "net_due",
        encoding,
        response_tokens: limits.response_tokens,
        response_bytes: limits.response_bytes,
    };
    let mut ledger = CostLedger::new(
        &case.fixture,
        label,
        encoding,
        vec![PhaseBudget {
            label: "known-definition investigation",
            limits,
            packet,
        }],
        limits,
    );
    let mut journey = Journey::bounded(&case.fixture, limits.calls);
    let reports = driver::investigate(&mut journey, &mut ledger, &task);
    let evaluation = evaluate(&case, &reports);
    let phase = ledger.checkpoint();
    let costs = ledger.finish();
    assert_eq!(phase.metrics.response_bytes, costs.metrics.response_bytes);
    let result = Episode {
        label,
        case,
        truth,
        task,
        evaluation,
        costs,
    };
    std::fs::write(
        result
            .case
            .fixture
            .state_path()
            .join("invoice-evaluation.json"),
        serde_json::to_vec_pretty(&result.json()).unwrap(),
    )
    .unwrap();
    result
}

fn group_summary(episodes: &[Episode]) -> Value {
    let summary = json!(
        episodes
            .iter()
            .map(|episode| json!({
                "label": episode.label,
                "failures": episode.positive_failures(),
            }))
            .collect::<Vec<_>>()
    );
    eprintln!("[invoice efficiency] {summary}");
    summary
}

#[test]
#[ignore = "user efficiency scenario; run scripts/test-scenarios.sh efficiency::invoice"]
fn known_invoice_rounding_and_helper_repair_keep_original_budgets() {
    let episodes = [
        episode("invoice-original-o200k", Variant::Broken, "o200k_base"),
        episode(
            "invoice-helper-repair-o200k",
            Variant::Repaired,
            "o200k_base",
        ),
        episode("invoice-original-cl100k", Variant::Broken, "cl100k_base"),
    ];
    for path in [
        "src/invoices.py",
        "src/models.py",
        "tests/test_invoices.py",
        "demo/invoices.py",
    ] {
        assert_eq!(
            episodes[0].case.source(path),
            episodes[1].case.source(path),
            "the independent repair changes only the active helper"
        );
    }
    let report = group_summary(&episodes);
    assert!(
        episodes
            .iter()
            .all(|episode| episode.positive_failures().is_empty()),
        "positive invoice tasks require every obligation within both response limits: {report}"
    );
}

#[test]
#[ignore = "user efficiency scenario; run scripts/test-scenarios.sh efficiency::invoice"]
fn active_invoice_alias_and_counterfeit_regression_remain_distinguishable() {
    let episodes = [
        episode("invoice-switched-binding", Variant::Switched, "o200k_base"),
        episode(
            "invoice-counterfeit-test",
            Variant::Counterfeit,
            "o200k_base",
        ),
    ];
    let report = group_summary(&episodes);
    let counterfeit = &episodes[1];
    assert!(
        counterfeit.evaluation.observed_counterfeit,
        "negative control must actually deliver the counterfeit import and literal assertion: {report}"
    );
    assert!(
        counterfeit
            .evaluation
            .missing
            .iter()
            .any(|item| item == "production regression"),
        "demo-bound assertion must be rejected as production regression evidence: {report}"
    );
    assert!(
        counterfeit.evaluation.wrong_identity.is_empty()
            && counterfeit.evaluation.cli_errors.is_empty()
            && counterfeit.costs.excesses.is_empty()
            && counterfeit.costs.accounting_gaps.is_empty(),
        "counterfeit rejection must not hide wrong identities or oversized delivery: {report}"
    );
    assert!(
        episodes[0].positive_failures().is_empty(),
        "positive binding-swap task needs its current actual helper and genuine check: {report}"
    );
}

#[test]
#[ignore = "user efficiency scenario; run scripts/test-scenarios.sh efficiency::invoice"]
fn unrelated_invoice_handler_changes_do_not_expand_the_evidence_envelope() {
    let episodes = [episode(
        "invoice-unrelated-handler-noise",
        Variant::Noise,
        "o200k_base",
    )];
    let report = group_summary(&episodes);
    assert!(
        episodes[0].positive_failures().is_empty(),
        "renamed and added natural handlers retain the original source/response envelope: {report}"
    );
}

#[test]
#[ignore = "user efficiency scenario; run scripts/test-scenarios.sh efficiency::invoice"]
fn multiline_invoice_bindings_after_natural_helpers_fit_original_budgets() {
    let episodes = [episode(
        "invoice-multiline-bindings",
        Variant::MultilineBinding,
        "o200k_base",
    )];
    let report = group_summary(&episodes);
    assert!(
        episodes[0].positive_failures().is_empty(),
        "actual multiline bindings beyond the preamble require every original obligation within the original envelope: {report}"
    );
}
