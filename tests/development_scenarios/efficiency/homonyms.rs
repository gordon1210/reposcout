//! H: positive package bindings distinguish two active same-name discount applications.

use super::support::{CostLedger, EvidenceFragment, Limits, PacketCost, PhaseBudget};
use crate::journeys::support::Journey;
use serde_json::json;
use std::fs;

mod driver;
mod fixture;
mod oracle;

const LIMITS: Limits = Limits {
    calls: 6,
    response_bytes: 24 * 1024,
    response_tokens: 7_500,
    source_paths: 6,
    source_nonblank_lines: 110,
    source_tokens: Some(2_000),
    body_nonblank_lines: None,
};

fn check(variant: fixture::Variant, encoding: &str) {
    let world = fixture::freeze(variant);
    let packet = world.packet();
    let fragments: Vec<_> = packet
        .iter()
        .map(|(_, path, source)| EvidenceFragment { path, source })
        .collect();
    let packet_cost = PacketCost::new(encoding, &fragments);
    assert!(
        packet_cost.paths.len() <= 6
            && packet_cost.nonblank_lines <= 110
            && packet_cost.tokens <= 2_000
    );
    oracle::assert_packet_sensitivity(&world);
    fs::write(world.fixture.state_path().join("frozen-homonyms-packet.json"), serde_json::to_vec_pretty(&json!({
        "task": "Review whether a discount change affects the storefront API or only staff preview. Storefront accepts exactly WELCOME at subtotals >=2000 cents, discounting 500 cents.",
        "base": world.base, "head": world.head, "necessary_fragments": packet,
    })).unwrap()).unwrap();
    let mut ledger = CostLedger::new(
        &world.fixture,
        "active application homonyms",
        encoding,
        vec![PhaseBudget {
            label: "pinned application review",
            limits: LIMITS,
            packet: packet_cost,
        }],
        LIMITS,
    );
    let mut journey = Journey::bounded(&world.fixture, LIMITS.calls);
    let evidence = driver::prepare(
        &mut journey,
        &mut ledger,
        &world.base,
        &world.head,
        [fixture::STOREFRONT, fixture::STAFF],
        encoding,
    );
    let costs = ledger.finish();
    let obligations = oracle::assess(&world, &evidence);
    eprintln!(
        "H {variant:?}/{encoding}: missing={:?}; wrong identities={:?}; coverage gaps={:?}; source/response/call excesses={:?}; accounting gaps={:?}; affected={:?}",
        obligations.missing,
        obligations.wrong_identity,
        obligations.coverage_gaps,
        costs.excesses,
        costs.accounting_gaps,
        obligations.storefront_affected
    );
    assert!(
        obligations.wrong_identity.is_empty(),
        "wrong snapshot/source identity: {:?}",
        obligations.wrong_identity
    );
    assert!(
        obligations.coverage_gaps.is_empty(),
        "comparison coverage is incomplete: {:?}",
        obligations.coverage_gaps
    );
    assert!(
        costs.accounting_gaps.is_empty(),
        "source/response delivery was not fully counted: {:?}",
        costs.accounting_gaps
    );
    assert!(
        costs.excesses.is_empty(),
        "complete CLI interaction exceeds frozen limits: {:?}",
        costs.excesses
    );
    assert_eq!(
        obligations.storefront_affected,
        Some(matches!(variant, fixture::Variant::Coupled)),
        "the real API import determines application impact"
    );
    if matches!(variant, fixture::Variant::Counterfeit) {
        assert_eq!(
            obligations.storefront_request_binding,
            Some(false),
            "the actually delivered helper check must fail the genuine request-binding predicate"
        );
        assert_eq!(
            obligations.missing,
            ["head storefront genuine dispatcher request/assertion binding"],
            "a passing staff-helper check cannot certify storefront requests"
        );
    } else {
        assert_eq!(
            obligations.storefront_request_binding,
            Some(true),
            "the genuine storefront request check must be delivered"
        );
        assert!(
            obligations.missing.is_empty(),
            "active-application review evidence is insufficient: {:?}",
            obligations.missing
        );
    }
}

#[test]
#[ignore = "user efficiency acceptance; run scripts/test-scenarios.sh efficiency"]
fn staff_prefix_change_preserves_the_independently_bound_storefront_o200k() {
    check(fixture::Variant::Independent, "o200k_base");
}

#[test]
#[ignore = "user efficiency acceptance; run scripts/test-scenarios.sh efficiency"]
fn staff_prefix_change_preserves_the_independently_bound_storefront_cl100k() {
    check(fixture::Variant::Independent, "cl100k_base");
}

#[test]
#[ignore = "user efficiency acceptance; run scripts/test-scenarios.sh efficiency"]
fn importing_staff_policy_into_the_real_storefront_changes_its_impact() {
    check(fixture::Variant::Coupled, "o200k_base");
}

#[test]
#[ignore = "negative efficiency control; run scripts/test-scenarios.sh efficiency"]
fn staff_helper_assertions_cannot_counterfeit_storefront_request_evidence() {
    check(fixture::Variant::Counterfeit, "o200k_base");
}
