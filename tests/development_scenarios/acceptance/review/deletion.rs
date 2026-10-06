use super::super::support::{assert_fixture_bounds, probe};
use super::{
    Fixture, Journey, ReviewEvidence, assert_source_attribution, confirm_remaining_sources,
    prepare_review,
};
use serde_json::json;

const OLD_HELPER: &str = "def round_cents(mills):\n    return (mills + 5) // 10\n";
const REPLACEMENT: &str = "def quantize_cents(mills):\n    whole, remainder = divmod(mills, 10)\n    return whole + int(remainder >= 5)\n";
const OLD_INVOICE: &str =
    "from money import round_cents\n\ndef invoice_total(mills):\n    return round_cents(mills)\n";
const NEW_INVOICE: &str = "from precise_money import quantize_cents\n\ndef invoice_total(mills):\n    return quantize_cents(mills)\n";
const FORGOTTEN_REFUND: &str = "from money import round_cents as settled_cents\n\ndef refund_total(mills):\n    return settled_cents(mills)\n";
const REPAIRED_REFUND: &str = "from precise_money import quantize_cents as settled_cents\n\ndef refund_total(mills):\n    return settled_cents(mills)\n";
const EXTERNAL: &str = "from builtins import round as round_cents\n\ndef preview_amount(value):\n    return round_cents(value)\n";

struct MigrationWorld {
    fixture: Fixture,
    base: String,
    head: String,
    repaired: bool,
}

fn assert_domain_truth(fixture: &Fixture, missing_local_target: bool) {
    let observed = probe(
        fixture,
        r"import importlib
import json
import external_rounding
results = {}
for module, function in [('invoice', 'invoice_total'), ('refund', 'refund_total')]:
    try:
        handler = getattr(importlib.import_module(module), function)
        results[module] = [handler(mills) for mills in [1000, 1004, 1005]]
    except ModuleNotFoundError as error:
        results[module] = {'missing_module': error.name}
results['external'] = external_rounding.preview_amount(100.25)
print(json.dumps(results))",
    );
    assert_eq!(observed["invoice"], json!([100, 100, 101]));
    assert_eq!(
        observed["external"], 100,
        "the external distractor remains independently runnable"
    );
    if missing_local_target {
        assert_eq!(observed["refund"], json!({"missing_module": "money"}));
    } else {
        assert_eq!(observed["refund"], json!([100, 100, 101]));
    }
}

fn migration_world(repaired: bool) -> MigrationWorld {
    let fixture = Fixture::new("acceptance-helper-migration");
    fixture.write("money.py", OLD_HELPER);
    fixture.write("invoice.py", OLD_INVOICE);
    fixture.write("refund.py", FORGOTTEN_REFUND);
    fixture.write("external_rounding.py", EXTERNAL);
    assert_domain_truth(&fixture, false);
    let base = fixture.commit("Invoice and refund share the local rounding policy");
    fixture.remove("money.py");
    fixture.write("precise_money.py", REPLACEMENT);
    fixture.write("invoice.py", NEW_INVOICE);
    if repaired {
        fixture.write("refund.py", REPAIRED_REFUND);
    }
    assert_domain_truth(&fixture, !repaired);
    let head = fixture.commit("Migrate the rounding policy and delete the old helper");
    assert_fixture_bounds(&fixture);
    MigrationWorld {
        fixture,
        base,
        head,
        repaired,
    }
}

fn assert_migration_evidence(world: &MigrationWorld, evidence: &ReviewEvidence) {
    let mut missing = Vec::new();
    for (label, side, path, fragments) in [
        (
            "old local rounding implementation",
            "base",
            "money.py",
            vec!["def round_cents(mills):", "return (mills + 5) // 10"],
        ),
        (
            "migrated invoice binding and call",
            "head",
            "invoice.py",
            vec![
                "from precise_money import quantize_cents",
                "return quantize_cents(mills)",
            ],
        ),
    ] {
        if !evidence.contains(side, path, &fragments) {
            missing.push(label);
        }
    }
    let binding = if world.repaired {
        "from precise_money import quantize_cents as settled_cents"
    } else {
        "from money import round_cents as settled_cents"
    };
    if !evidence.contains(
        "head",
        "refund.py",
        &[binding, "return settled_cents(mills)"],
    ) {
        missing.push(if world.repaired {
            "repaired refund binding and call"
        } else {
            "remaining refund import/call to the deleted local target"
        });
    }
    let deletion = evidence.report["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|change| {
            change["base"]["path"] == "money.py"
                && change["head"].is_null()
                && change["status"] == "deleted"
        });
    if !deletion {
        missing.push("absence of the old local module at the PR tip");
    }
    for relation in evidence.report["relations"].as_array().unwrap() {
        if relation["edge"]["target"] == "money.py" {
            assert!(
                relation["side"] == "base"
                    && ["invoice.py", "refund.py"]
                        .contains(&relation["edge"]["source"].as_str().unwrap()),
                "an absent local helper or similarly named external binding cannot be a resolved head target: {relation}"
            );
        }
    }
    if world.repaired {
        assert!(
            !evidence.contains("head", "refund.py", &["from money import round_cents"]),
            "a repaired binding removes the unresolved-consumer obligation; stale source is misleading"
        );
    }
    assert!(
        missing.is_empty(),
        "rounding migration review remains insufficient: {}",
        missing.join(", ")
    );
}

fn check_migration(repaired: bool) {
    let world = migration_world(repaired);
    let mut journey = Journey::bounded(&world.fixture, 8);
    let mut evidence = prepare_review(&mut journey, &world.base, &world.head);
    confirm_remaining_sources(&mut journey, &mut evidence);
    assert_source_attribution(&world.fixture, &evidence, &world.base, &world.head);
    assert_migration_evidence(&world, &evidence);
}

#[test]
#[ignore = "user acceptance; run scripts/test-scenarios.sh acceptance"]
fn deleted_helper_review_retains_the_forgotten_local_consumer() {
    check_migration(false);
}

#[test]
#[ignore = "user acceptance; run scripts/test-scenarios.sh acceptance"]
fn repairing_the_refund_binding_removes_the_unresolved_consumer_obligation() {
    check_migration(true);
}
