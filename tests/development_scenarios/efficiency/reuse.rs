//! Frozen case K: current quota evidence with caller-owned source retention.

use super::support::{CostLedger, EvidenceFragment, Limits, PacketCost, PhaseBudget};
use crate::journeys::support::Journey;
use driver::PublicTask;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use world::{QuotaWorld, Variant};

mod controls;
mod driver;
mod evidence;
mod world;

fn sha256(source: &str) -> String {
    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(source.as_bytes()) {
        write!(encoded, "{byte:02x}").unwrap();
    }
    encoded
}

fn packet_cost(encoding: &str, fragments: &[world::RequiredFragment]) -> PacketCost {
    PacketCost::new(
        encoding,
        &fragments
            .iter()
            .map(|part| EvidenceFragment {
                path: part.path,
                source: &part.source,
            })
            .collect::<Vec<_>>(),
    )
}

fn limits(
    calls: usize,
    bytes: usize,
    tokens: usize,
    paths: usize,
    lines: usize,
    bodies: Option<usize>,
) -> Limits {
    Limits {
        calls,
        response_bytes: bytes,
        response_tokens: tokens,
        source_paths: paths,
        source_nonblank_lines: lines,
        source_tokens: None,
        body_nonblank_lines: bodies,
    }
}

fn run(variant: Variant, encoding: &str) {
    let world = QuotaWorld::new(variant, encoding);
    if matches!(variant, Variant::Helper) && encoding == "o200k_base" {
        controls::assert_oracle_sensitivity(&world);
    }
    let initial_packet = packet_cost(encoding, &world.initial_packet);
    let delta_packet = packet_cost(encoding, &world.delta_packet);
    let initial = limits(4, 8 * 1024, initial_packet.tokens + 2000, 3, 64, None);
    let followup = if matches!(variant, Variant::Binding) {
        limits(4, 8 * 1024, delta_packet.tokens + 2400, 2, 32, None)
    } else {
        limits(
            3,
            6 * 1024,
            delta_packet.tokens + 1600,
            1,
            16,
            matches!(
                variant,
                Variant::UnrelatedFile | Variant::UnrelatedDeclaration
            )
            .then_some(0),
        )
    };
    let episode = limits(
        initial.calls + followup.calls,
        initial.response_bytes + followup.response_bytes,
        initial.response_tokens + followup.response_tokens,
        initial.source_paths + followup.source_paths,
        initial.source_nonblank_lines + followup.source_nonblank_lines,
        None,
    );
    let mut costs = CostLedger::new(
        &world.fixture,
        "K-quota-evidence-reuse",
        encoding,
        vec![
            PhaseBudget {
                label: "initial-pinned-HEAD",
                limits: initial,
                packet: initial_packet,
            },
            PhaseBudget {
                label: "current-worktree-followup",
                limits: followup,
                packet: delta_packet,
            },
        ],
        episode,
    );
    let mut journey = Journey::bounded(&world.fixture, episode.calls);
    world.assert_probe(false);
    let task = PublicTask {
        query: "storage status",
        snapshot: "HEAD",
        encoding,
        limits: initial,
    };
    let before = driver::initial(&mut journey, &mut costs, &task);
    let mut failures = evidence::initial_missing(&world, &before);
    let first = costs.checkpoint();
    failures.extend(first.excesses);
    failures.extend(first.accounting_gaps);

    // Only the fixture author mutates input, between the two declared public-CLI phases.
    world.mutate();
    world.assert_probe(true);
    let task = PublicTask {
        query: "storage status",
        snapshot: "worktree",
        encoding,
        limits: followup,
    };
    let after = driver::follow_up(&mut journey, &mut costs, &task, &before);
    failures.extend(evidence::final_missing(&world, &after));
    let second = costs.checkpoint();
    failures.extend(second.excesses);
    failures.extend(second.accounting_gaps);
    let total = costs.finish();
    failures.extend(total.excesses);
    failures.extend(total.accounting_gaps);
    fs::write(
        world.fixture.state_path().join("quota-obligations.json"),
        serde_json::to_vec_pretty(&failures).unwrap(),
    )
    .unwrap();
    assert!(
        failures.is_empty(),
        "K {} / {encoding}: missing or inefficient evidence:\n{}",
        variant.label(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "user efficiency case K; run scripts/test-scenarios.sh efficiency::reuse"]
fn helper_update_rejects_stale_handoff_and_reuses_unchanged_context_o200k() {
    run(Variant::Helper, "o200k_base");
}

#[test]
#[ignore = "same frozen K representative with explicitly selected encoding"]
fn helper_update_rejects_stale_handoff_and_reuses_unchanged_context_cl100k() {
    run(Variant::Helper, "cl100k_base");
}

#[test]
#[ignore = "user efficiency case K; zero new body evidence after an unrelated-file edit"]
fn unrelated_file_edit_requires_no_new_source_bodies() {
    run(Variant::UnrelatedFile, "o200k_base");
}

#[test]
#[ignore = "user efficiency case K; retained source needs public disjoint-change proof"]
fn unrelated_declaration_edit_reuses_source_under_its_original_identity() {
    run(Variant::UnrelatedDeclaration, "o200k_base");
}

#[test]
#[ignore = "user efficiency case K; changed binding has a separately frozen necessary delta"]
fn changed_binding_requires_new_binding_and_helper_without_repeating_policy_body() {
    run(Variant::Binding, "o200k_base");
}
