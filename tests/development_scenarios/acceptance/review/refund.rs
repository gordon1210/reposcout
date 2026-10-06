use super::super::support::{assert_fixture_bounds, probe};
use super::{Fixture, Journey, ReviewEvidence, assert_source_attribution, prepare_review};
use serde_json::{Value, json};
use std::fmt::Write as _;

#[derive(Clone, Copy)]
enum Variant {
    Ordinary,
    ThirdCaller,
    RenamedWithDecoys,
    CounterfeitTest,
}

struct Caller {
    module: &'static str,
    function: &'static str,
    binding: &'static str,
}

struct RefundWorld {
    fixture: Fixture,
    base: String,
    head: String,
    policy: &'static str,
    test: &'static str,
    callers: Vec<Caller>,
    head_test_targets_policy: bool,
}

fn policy_source(inclusive: bool) -> &'static str {
    if inclusive {
        "def refund_allowed(days):\n    return days <= 14\n"
    } else {
        "def refund_allowed(days):\n    return days < 14\n"
    }
}

fn test_source(policy: &str) -> String {
    format!(
        "from {policy} import refund_allowed\n\ndef test_day_fourteen():\n    assert refund_allowed(14) is True\n"
    )
}

fn caller_source(policy: &str, caller: &Caller) -> String {
    format!(
        "from {policy} import refund_allowed as {}\n\ndef {}(days):\n    return {}(days)\n",
        caller.binding, caller.function, caller.binding
    )
}

fn write_production_flows(world: &RefundWorld) {
    let mut application = String::new();
    for caller in &world.callers {
        world.fixture.write(
            &format!("{}.py", caller.module),
            &caller_source(world.policy, caller),
        );
        writeln!(
            application,
            "from {} import {}",
            caller.module, caller.function
        )
        .unwrap();
    }
    application.push_str("\nPRODUCTION_FLOWS = {\n");
    for caller in &world.callers {
        writeln!(
            application,
            "    '{}': {},",
            caller.function, caller.function
        )
        .unwrap();
    }
    application.push_str("}\n");
    world.fixture.write("application.py", &application);
}

fn assert_domain_truth(world: &RefundWorld, expected: [bool; 3], test_passes: bool) {
    let observed = probe(
        &world.fixture,
        &format!(
            "import json\nimport application\nimport {} as checks\n\
             flows = {{name: [handler(day) for day in [13, 14, 15]] for name, handler in application.PRODUCTION_FLOWS.items()}}\n\
             try:\n    checks.test_day_fourteen()\n    passed = True\n\
             except AssertionError:\n    passed = False\n\
             print(json.dumps({{'flows': flows, 'boundary_test_passes': passed}}))\n",
            world.test
        ),
    );
    let expected_flows: serde_json::Map<String, Value> = world
        .callers
        .iter()
        .map(|caller| (caller.function.to_owned(), json!(expected)))
        .collect();
    assert_eq!(observed["flows"], Value::Object(expected_flows));
    assert_eq!(observed["boundary_test_passes"], test_passes);
}

fn refund_world(variant: Variant) -> RefundWorld {
    let names = if matches!(variant, Variant::RenamedWithDecoys) {
        [
            "eligibility",
            "sales_endpoint",
            "parcel_endpoint",
            "test_window",
        ]
    } else {
        ["refund_policy", "checkout", "returns", "test_refunds"]
    };
    let mut world = RefundWorld {
        fixture: Fixture::new("acceptance-refund-review"),
        base: String::new(),
        head: String::new(),
        policy: names[0],
        test: names[3],
        head_test_targets_policy: !matches!(variant, Variant::CounterfeitTest),
        callers: vec![
            Caller {
                module: names[1],
                function: "refund_order",
                binding: "refund_allowed",
            },
            Caller {
                module: names[2],
                function: "return_delivery",
                binding: "within_window",
            },
        ],
    };
    if matches!(variant, Variant::ThirdCaller) {
        world.callers.push(Caller {
            module: "service_desk",
            function: "refund_at_counter",
            binding: "eligible_return",
        });
    }
    world
        .fixture
        .write(&format!("{}.py", world.policy), policy_source(true));
    world
        .fixture
        .write(&format!("{}.py", world.test), &test_source(world.policy));
    world.fixture.write(
        "training_policy.py",
        "def refund_allowed(days):\n    return days <= 30\n",
    );
    write_production_flows(&world);
    if matches!(variant, Variant::RenamedWithDecoys) {
        world.fixture.write(
            "archive/refund_policy.py",
            "def refund_allowed(days):\n    return False\n",
        );
        world.fixture.write(
            "notes/refund_review.md",
            "refund_allowed checkout refunds return_delivery day 14\n",
        );
    }
    assert_domain_truth(&world, [true, true, false], true);
    world.base = world
        .fixture
        .commit("Refunds are permitted through day fourteen");
    world
        .fixture
        .write(&format!("{}.py", world.policy), policy_source(false));
    if matches!(variant, Variant::CounterfeitTest) {
        world.fixture.write(
            &format!("{}.py", world.test),
            &test_source("training_policy"),
        );
    }
    assert_domain_truth(
        &world,
        [true, false, false],
        matches!(variant, Variant::CounterfeitTest),
    );
    world.head = world.fixture.commit("Change the refund window boundary");
    // The later local repair must not replace the PR tip's evidence.
    world
        .fixture
        .write(&format!("{}.py", world.policy), policy_source(true));
    assert_domain_truth(&world, [true, true, false], true);
    assert_fixture_bounds(&world.fixture);
    world
}

fn missing_obligations(world: &RefundWorld, evidence: &ReviewEvidence) -> Vec<String> {
    let mut missing = Vec::new();
    let policy = format!("{}.py", world.policy);
    for (side, predicate) in [("base", "return days <= 14"), ("head", "return days < 14")] {
        if !evidence.contains(side, &policy, &["def refund_allowed(days):", predicate]) {
            missing.push(format!("{side} refund policy"));
        }
    }
    for caller in &world.callers {
        let import = format!(
            "from {} import refund_allowed as {}",
            world.policy, caller.binding
        );
        let call = format!("return {}(days)", caller.binding);
        let declaration = format!("def {}(days):", caller.function);
        if !evidence.contains(
            "head",
            &format!("{}.py", caller.module),
            &[&import, &declaration, &call],
        ) {
            missing.push(format!(
                "production caller {} with its binding",
                caller.function
            ));
        }
        let activation_import = format!("from {} import {}", caller.module, caller.function);
        let registration = format!("'{}': {}", caller.function, caller.function);
        if !evidence.contains(
            "head",
            "application.py",
            &[&activation_import, &registration],
        ) {
            missing.push(format!(
                "active production registration for {}",
                caller.function
            ));
        }
    }
    let import = format!("from {} import refund_allowed", world.policy);
    if !evidence.contains(
        "head",
        &format!("{}.py", world.test),
        &[&import, "assert refund_allowed(14) is True"],
    ) {
        missing.push("genuine day-14 regression assertion".to_owned());
    }
    for relation in evidence.report["relations"].as_array().unwrap() {
        let source = relation["edge"]["source"].as_str().unwrap();
        let target = relation["edge"]["target"].as_str().unwrap();
        if target == policy {
            let is_caller = world
                .callers
                .iter()
                .any(|caller| source == format!("{}.py", caller.module));
            let is_genuine_test = source == format!("{}.py", world.test)
                && (relation["side"] == "base" || world.head_test_targets_policy);
            assert!(
                is_caller || is_genuine_test,
                "incorrect direct policy-consumer claim: {relation}"
            );
        }
    }
    missing
}

fn check_refund_review(variant: Variant) {
    let world = refund_world(variant);
    let mut journey = Journey::bounded(&world.fixture, 8);
    let evidence = prepare_review(&mut journey, &world.base, &world.head);
    assert_source_attribution(&world.fixture, &evidence, &world.base, &world.head);
    let missing = missing_obligations(&world, &evidence);
    if matches!(variant, Variant::CounterfeitTest) {
        assert!(
            evidence.contains(
                "head",
                &format!("{}.py", world.test),
                &[
                    "from training_policy import refund_allowed",
                    "assert refund_allowed(14) is True",
                ]
            ),
            "the negative control must expose the actual counterfeit assertion"
        );
        assert_eq!(missing, ["genuine day-14 regression assertion"]);
        eprintln!(
            "negative control: the passing counterfeit test does not complete review readiness"
        );
    } else {
        assert!(
            missing.is_empty(),
            "refund review remains insufficient: {}",
            missing.join(", ")
        );
    }
}

#[test]
#[ignore = "user acceptance; run scripts/test-scenarios.sh acceptance"]
fn refund_review_supplies_policy_callers_and_genuine_boundary_check() {
    check_refund_review(Variant::Ordinary);
}

#[test]
#[ignore = "user acceptance; run scripts/test-scenarios.sh acceptance"]
fn refund_review_obligation_grows_with_a_third_active_aliased_caller() {
    check_refund_review(Variant::ThirdCaller);
}

#[test]
#[ignore = "user acceptance; run scripts/test-scenarios.sh acceptance"]
fn refund_review_survives_renamed_paths_and_unused_homonyms() {
    check_refund_review(Variant::RenamedWithDecoys);
}

#[test]
#[ignore = "negative acceptance control; run scripts/test-scenarios.sh acceptance"]
fn counterfeit_refund_test_cannot_satisfy_genuine_regression_evidence() {
    check_refund_review(Variant::CounterfeitTest);
}
