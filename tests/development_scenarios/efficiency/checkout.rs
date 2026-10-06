//! Checkout compatibility is a scoped user decision, not an all-PR safety verdict.

use super::support::{Assessment, CostLedger, EvidenceFragment, Limits, PacketCost, PhaseBudget};
use crate::journeys::support::Journey;
use driver::{Evidence, Task};
use fixture::{Variant, World};

mod driver;
mod fixture;
mod oracle;

fn limits() -> Limits {
    Limits {
        calls: 6,
        response_bytes: 24 * 1024,
        response_tokens: 7_500,
        source_paths: 5,
        source_nonblank_lines: 110,
        source_tokens: Some(2_000),
        body_nonblank_lines: None,
    }
}

fn packet(world: &World, encoding: &str) -> PacketCost {
    let fragments = [
        ("shop/routes.py", fixture::ROUTES),
        ("shop/handler.py", fixture::HANDLER),
        ("shop/amounts.py", fixture::CALCULATOR),
        ("shop/presentation.py", world.base_serializer),
        ("shop/presentation.py", world.head_serializer),
        ("checks/test_checkout.py", fixture::CONTRACT),
    ]
    .map(|(path, source)| EvidenceFragment { path, source });
    let packet = PacketCost::new(encoding, &fragments);
    assert_eq!(packet.paths.len(), 5);
    assert!(packet.nonblank_lines <= 110 && packet.tokens <= 2_000);
    packet
}

fn episode(variant: Variant, encoding: &str) -> (World, Evidence, Assessment) {
    let world = fixture::world(variant);
    oracle::assert_frozen_packet(&world);
    let mut ledger = CostLedger::new(
        &world.fixture,
        "checkout wire compatibility",
        encoding,
        vec![PhaseBudget {
            label: "pinned checkout review",
            limits: limits(),
            packet: packet(&world, encoding),
        }],
        limits(),
    );
    let mut journey = Journey::bounded(&world.fixture, 6);
    let evidence = driver::prepare(
        &mut journey,
        &mut ledger,
        &Task {
            endpoint: "/checkout",
            base: &world.base,
            head: &world.head,
            encoding,
        },
    );
    ledger.checkpoint();
    let assessment = ledger.finish();
    eprintln!(
        "checkout {variant:?} {encoding}: {}; missing obligations: {:?}",
        serde_json::to_string(&assessment).unwrap(),
        oracle::missing(&world, &evidence)
    );
    oracle::assert_attribution(&world, &evidence);
    (world, evidence, assessment)
}

fn assert_cost(assessment: &Assessment) {
    assert!(
        assessment.accounting_gaps.is_empty() && assessment.excesses.is_empty(),
        "checkout interaction is over budget or unaccounted: {:?}; {:?}",
        assessment.excesses,
        assessment.accounting_gaps
    );
}

fn assert_sufficient(world: &World, evidence: &Evidence) {
    let missing = oracle::missing(world, evidence);
    assert!(
        missing.is_empty(),
        "checkout compatibility evidence remains insufficient: {}",
        missing.join(", ")
    );
}

#[test]
#[ignore = "user efficiency acceptance; run scripts/test-scenarios.sh efficiency"]
fn checkout_wire_review_keeps_irrelevant_pr_churn_out_of_context() {
    let pairs = ["o200k_base", "cl100k_base"].map(|encoding| {
        (
            episode(Variant::Faulty, encoding),
            episode(Variant::Noisy, encoding),
        )
    });
    for ((quiet, quiet_evidence, quiet_cost), (noisy, noisy_evidence, noisy_cost)) in pairs {
        assert_cost(&quiet_cost);
        assert_cost(&noisy_cost);
        assert_sufficient(&quiet, &quiet_evidence);
        assert_sufficient(&noisy, &noisy_evidence);
        assert!(
            noisy_cost.metrics.response_bytes <= quiet_cost.metrics.response_bytes + 4 * 1024,
            "unrelated churn added more than 4 KiB of raw interaction responses"
        );
    }
}

#[test]
#[ignore = "user efficiency acceptance; run scripts/test-scenarios.sh efficiency"]
fn checkout_wire_review_delivers_the_real_repair_from_pinned_revisions() {
    let (world, evidence, cost) = episode(Variant::Repaired, "o200k_base");
    assert_cost(&cost);
    assert_sufficient(&world, &evidence);
}

#[test]
#[ignore = "negative efficiency control; run scripts/test-scenarios.sh efficiency"]
fn calculator_only_check_does_not_complete_checkout_wire_review() {
    let (world, evidence, cost) = episode(Variant::Counterfeit, "o200k_base");
    assert_cost(&cost);
    let counterfeit = fixture::COUNTERFEIT
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert!(
        evidence.contains("head", "checks/test_checkout.py", &counterfeit),
        "negative control must actually deliver the calculator-only check"
    );
    assert_eq!(
        oracle::missing(&world, &evidence),
        ["genuine dispatcher-level request assertions"]
    );
    eprintln!("negative control: a passing calculator check leaves checkout review incomplete");
}
