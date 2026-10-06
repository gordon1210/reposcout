use crate::acceptance::support::{assert_fixture_bounds, probe};
use crate::support::Fixture;
use serde_json::{Value, json};

pub(super) const ROUTES: &str = r#"from shop.handler import checkout as submit_checkout

ROUTES = {"POST /checkout": submit_checkout}


def dispatch(method, path, payload):
    return ROUTES[f"{method} {path}"](payload)
"#;

pub(super) const HANDLER: &str = r#"from shop.amounts import order_total as calculate_total
from shop.presentation import checkout_response as render_checkout


def checkout(payload):
    amount_cents = calculate_total(payload["items"])
    return render_checkout(amount_cents)
"#;

pub(super) const CALCULATOR: &str = r#"def order_total(items):
    return sum(
        item["unit_price_cents"] * item["quantity"]
        for item in items
    )
"#;

pub(super) const CORRECT: &str = r#"def checkout_response(amount_cents):
    return {"amount_cents": amount_cents, "currency": "EUR"}
"#;

pub(super) const FAULTY: &str = r#"def checkout_response(amount_cents):
    return {"amount_cents": amount_cents // 100, "currency": "EUR"}
"#;

pub(super) const CONTRACT: &str = r#"import unittest
from shop.routes import dispatch as send_request


class CheckoutContract(unittest.TestCase):
    def test_checkout_wire_amounts(self):
        cases = [
            ({"items": [{"unit_price_cents": 1250, "quantity": 2}]},
             {"amount_cents": 2500, "currency": "EUR"}),
            ({"items": [{"unit_price_cents": 99, "quantity": 1}]},
             {"amount_cents": 99, "currency": "EUR"}),
        ]
        for request, expected in cases:
            with self.subTest(request=request):
                self.assertEqual(send_request("POST", "/checkout", request), expected)
"#;

pub(super) const COUNTERFEIT: &str = r#"import unittest
from shop.amounts import order_total


class CheckoutContract(unittest.TestCase):
    def test_checkout_wire_amounts(self):
        self.assertEqual(order_total([{"unit_price_cents": 1250, "quantity": 2}]), 2500)
        self.assertEqual(order_total([{"unit_price_cents": 99, "quantity": 1}]), 99)
"#;

pub(super) const REPORT_BASE: &str = r#"import csv
import io

def summarize(events):
    totals = {}
    for event in events:
        route = event["route"]
        record = totals.setdefault(route, {"count": 0, "bytes": 0, "errors": 0})
        record["count"] += 1
        record["bytes"] += event["response_bytes"]
        record["errors"] += int(event["status"] >= 500)
    return totals

def render_report(events):
    output = io.StringIO()
    writer = csv.writer(output, lineterminator="\n")
    writer.writerow(["route", "requests", "bytes", "errors"])
    for route, record in sorted(summarize(events).items()):
        writer.writerow([route, record["count"], record["bytes"], record["errors"]])
    return output.getvalue()
"#;

pub(super) const REPORT_HEAD: &str = r#"import csv
import io


def summarize(events):
    totals = {}
    for event in events:
        route = event["route"]
        record = totals.setdefault(
            route,
            {"count": 0, "bytes": 0, "errors": 0},
        )
        record["count"] += 1
        record["bytes"] += event["response_bytes"]
        record["errors"] += int(event["status"] >= 500)
    return totals


def render_report(events):
    output = io.StringIO()
    writer = csv.writer(output, lineterminator="\n")
    writer.writerow(["route", "requests", "bytes", "errors"])
    for route, record in sorted(summarize(events).items()):
        writer.writerow(
            [route, record["count"], record["bytes"], record["errors"]]
        )
    return output.getvalue()
"#;

pub(super) const PROBE: &str = r#"import importlib.util
import io
import json
import unittest
from pathlib import Path
from shop.routes import dispatch
from shop.amounts import order_total

requests = [
    {"items": [{"unit_price_cents": 1250, "quantity": 2}]},
    {"items": [{"unit_price_cents": 99, "quantity": 1}]},
]
spec = importlib.util.spec_from_file_location("checkout_checks", "checks/test_checkout.py")
checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checks)
result = unittest.TextTestRunner(stream=io.StringIO()).run(
    unittest.defaultTestLoader.loadTestsFromModule(checks)
)
evidence = {
    "responses": [dispatch("POST", "/checkout", request) for request in requests],
    "calculator": [order_total(request["items"]) for request in requests],
    "checks_pass": result.wasSuccessful(),
}
if Path("telemetry/reporting.py").exists():
    from telemetry.reporting import render_report
    evidence["telemetry"] = render_report([
        {"route": "/health", "response_bytes": 40, "status": 200},
        {"route": "/health", "response_bytes": 12, "status": 503},
    ])
    evidence["dashboard"] = json.loads(Path("telemetry/dashboard.json").read_text())
print(json.dumps(evidence, sort_keys=True))
"#;

#[derive(Clone, Copy, Debug)]
pub(super) enum Variant {
    Faulty,
    Noisy,
    Repaired,
    Counterfeit,
}

pub(super) struct World {
    pub fixture: Fixture,
    pub base: String,
    pub head: String,
    pub base_serializer: &'static str,
    pub head_serializer: &'static str,
    pub head_checks: &'static str,
}

fn dashboard() -> Value {
    json!({"routes": (0..180).map(|index| json!({
        "route": format!("/metric/{index}"),
        "requests": index + 1,
        "response_bytes": index * 128,
        "errors": index % 7,
    })).collect::<Vec<_>>()})
}

fn assert_truth(fixture: &Fixture, variant: Variant, base: bool) {
    let observed = probe(fixture, PROBE);
    let repaired = base || matches!(variant, Variant::Repaired);
    let expected = if repaired {
        json!([
            {"amount_cents": 2500, "currency": "EUR"},
            {"amount_cents": 99, "currency": "EUR"},
        ])
    } else {
        json!([
            {"amount_cents": 25, "currency": "EUR"},
            {"amount_cents": 0, "currency": "EUR"},
        ])
    };
    assert_eq!(observed["responses"], expected);
    assert_eq!(observed["calculator"], json!([2500, 99]));
    assert_eq!(
        observed["checks_pass"],
        repaired || matches!(variant, Variant::Counterfeit)
    );
    if matches!(variant, Variant::Noisy) {
        assert_eq!(
            observed["telemetry"],
            "route,requests,bytes,errors\n/health,2,52,1\n"
        );
        assert_eq!(observed["dashboard"], dashboard());
    }
}

pub(super) fn world(variant: Variant) -> World {
    let fixture = Fixture::new("efficiency-checkout-wire-compatibility");
    for (path, source) in [
        ("shop/__init__.py", ""),
        ("shop/routes.py", ROUTES),
        ("shop/handler.py", HANDLER),
        ("shop/amounts.py", CALCULATOR),
        ("shop/presentation.py", CORRECT),
        ("checks/test_checkout.py", CONTRACT),
    ] {
        fixture.write(path, source);
    }
    if matches!(variant, Variant::Noisy) {
        fixture.write("telemetry/__init__.py", "");
        fixture.write("telemetry/reporting.py", REPORT_BASE);
        fixture.write(
            "telemetry/dashboard.json",
            &serde_json::to_string(&dashboard()).unwrap(),
        );
    }
    assert_truth(&fixture, variant, true);
    let initial = fixture.commit("Preserve checkout integer cents on the wire");
    let (base, base_serializer) = if matches!(variant, Variant::Repaired) {
        fixture.write("shop/presentation.py", FAULTY);
        assert_truth(&fixture, Variant::Faulty, false);
        (
            fixture.commit("Introduce the checkout wire regression before repair"),
            FAULTY,
        )
    } else {
        (initial, CORRECT)
    };
    let head_serializer = if matches!(variant, Variant::Repaired) {
        CORRECT
    } else {
        FAULTY
    };
    fixture.write("shop/presentation.py", head_serializer);
    let head_checks = if matches!(variant, Variant::Counterfeit) {
        COUNTERFEIT
    } else {
        CONTRACT
    };
    fixture.write("checks/test_checkout.py", head_checks);
    if matches!(variant, Variant::Noisy) {
        fixture.write("telemetry/reporting.py", REPORT_HEAD);
        fixture.write(
            "telemetry/dashboard.json",
            &serde_json::to_string_pretty(&dashboard()).unwrap(),
        );
    }
    assert_truth(&fixture, variant, false);
    let head = fixture.commit("Review checkout presentation and unrelated maintenance");
    assert_fixture_bounds(&fixture);
    World {
        fixture,
        base,
        head,
        base_serializer,
        head_serializer,
        head_checks,
    }
}
