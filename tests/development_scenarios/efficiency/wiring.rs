//! G: the selected tariff changes without editing the endpoint or policy declarations.

use super::support::{CostLedger, EvidenceFragment, Limits, PacketCost, PhaseBudget};
use crate::journeys::support::Journey;
use serde_json::json;
use std::fs;

mod driver;
mod esm;
mod fixture;
mod oracle;

const LIMITS: Limits = Limits {
    calls: 6,
    response_bytes: 24 * 1024,
    response_tokens: 7_500,
    source_paths: 6,
    source_nonblank_lines: 100,
    source_tokens: Some(1_800),
    body_nonblank_lines: None,
};

fn check(input: &oracle::Input<'_>, encoding: &str, counterfeit: bool) {
    let fragments: Vec<_> = input
        .packet
        .iter()
        .map(|(_, path, source)| EvidenceFragment { path, source })
        .collect();
    let packet = PacketCost::new(encoding, &fragments);
    assert!(
        packet.nonblank_lines <= 100 && packet.tokens <= 1_800 && packet.paths.len() <= 6,
        "the independently frozen necessary packet must fit before any product output"
    );
    oracle::assert_packet_sensitivity(input, counterfeit);
    fs::write(input.fixture.state_path().join("frozen-wiring-packet.json"), serde_json::to_vec_pretty(&json!({"task": fixture::TASK, "base": input.base, "head": input.head, "necessary_fragments": input.packet})).unwrap()).unwrap();
    let mut ledger = CostLedger::new(
        input.fixture,
        "shipping wiring review",
        encoding,
        vec![PhaseBudget {
            label: "pinned wiring",
            limits: LIMITS,
            packet,
        }],
        LIMITS,
    );
    let mut journey = Journey::bounded(input.fixture, LIMITS.calls);
    let evidence = driver::prepare(
        &mut journey,
        &mut ledger,
        input.base,
        input.head,
        fixture::TASK,
        encoding,
    );
    let costs = ledger.finish();
    let obligations = oracle::assess(input, &evidence);
    eprintln!(
        "G obligations: missing={:?}; wrong identities={:?}; false declaration changes={:?}; coverage gaps={:?}; context diagnostics={:?}; cost excesses={:?}; accounting gaps={:?}",
        obligations.missing,
        obligations.wrong_identity,
        obligations.false_changes,
        obligations.coverage_gaps,
        obligations.context_diagnostics,
        costs.excesses,
        costs.accounting_gaps
    );
    assert!(
        obligations.wrong_identity.is_empty(),
        "wrong source identity: {:?}",
        obligations.wrong_identity
    );
    assert!(
        obligations.false_changes.is_empty(),
        "unchanged declarations mislabeled: {:?}",
        obligations.false_changes
    );
    assert!(
        obligations.coverage_gaps.is_empty(),
        "incomplete change evidence: {:?}",
        obligations.coverage_gaps
    );
    assert!(
        costs.accounting_gaps.is_empty(),
        "unaccounted delivery: {:?}",
        costs.accounting_gaps
    );
    assert!(
        costs.excesses.is_empty(),
        "complete interaction exceeded frozen limits: {:?}",
        costs.excesses
    );
    if counterfeit {
        assert_eq!(
            obligations.missing,
            ["genuine dispatcher request/assertion binding"],
            "the actual passing retail-only check must be exposed and rejected"
        );
    } else {
        assert!(
            obligations.missing.is_empty(),
            "wiring review remains insufficient: {:?}",
            obligations.missing
        );
    }
}

fn check_python(variant: fixture::Variant, encoding: &str) {
    let world = fixture::freeze(variant);
    let packet = world.packet();
    let selected = if matches!(variant, fixture::Variant::UnusedImport) {
        &world.retail_path
    } else {
        &world.supplier_path
    };
    check(
        &oracle::Input {
            fixture: &world.fixture,
            base: &world.base,
            head: &world.head,
            before: &world.before,
            after: &world.after,
            packet: &packet,
            test_path: "tests/test_quote.py",
            genuine_test: fixture::REQUEST_TEST,
            selected_policy_path: selected,
            selected_policy: &world.after[selected],
        },
        encoding,
        matches!(variant, fixture::Variant::Counterfeit),
    );
}

#[test]
#[ignore = "user efficiency acceptance; run scripts/test-scenarios.sh efficiency"]
fn shipping_import_change_preserves_unchanged_declarations_and_pinned_faulty_head_o200k() {
    check_python(fixture::Variant::ActiveImport, "o200k_base");
}

#[test]
#[ignore = "user efficiency acceptance; run scripts/test-scenarios.sh efficiency"]
fn shipping_import_change_preserves_unchanged_declarations_and_pinned_faulty_head_cl100k() {
    check_python(fixture::Variant::ActiveImport, "cl100k_base");
}

#[test]
#[ignore = "user efficiency acceptance; run scripts/test-scenarios.sh efficiency"]
fn unused_supplier_import_does_not_replace_the_active_retail_binding() {
    check_python(fixture::Variant::UnusedImport, "o200k_base");
}

#[test]
#[ignore = "negative efficiency control; run scripts/test-scenarios.sh efficiency"]
fn direct_retail_assertions_cannot_counterfeit_a_dispatcher_regression_check() {
    check_python(fixture::Variant::Counterfeit, "o200k_base");
}

#[test]
#[ignore = "user efficiency acceptance; requires Node; run scripts/test-scenarios.sh efficiency"]
fn package_imports_mapping_alone_changes_the_active_esm_tariff_o200k() {
    check_esm("o200k_base");
}

#[test]
#[ignore = "user efficiency acceptance; requires Node; run scripts/test-scenarios.sh efficiency"]
fn package_imports_mapping_alone_changes_the_active_esm_tariff_cl100k() {
    check_esm("cl100k_base");
}

fn check_esm(encoding: &str) {
    let world = esm::freeze();
    let packet = esm::World::packet();
    check(
        &oracle::Input {
            fixture: &world.fixture,
            base: &world.base,
            head: &world.head,
            before: &world.before,
            after: &world.after,
            packet: &packet,
            test_path: "tests/test_quote.js",
            genuine_test: esm::REQUEST_TEST,
            selected_policy_path: "shipping/supplier.js",
            selected_policy: &world.after["shipping/supplier.js"],
        },
        encoding,
        false,
    );
}
