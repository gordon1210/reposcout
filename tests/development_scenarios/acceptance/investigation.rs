use super::support::{Fixture, Journey, assert_fixture_bounds, probe};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

struct Ticket {
    method: &'static str,
    endpoint: &'static str,
    request: Value,
    max_files: usize,
    max_nonblank_lines: usize,
}

fn ticket() -> Ticket {
    Ticket {
        method: "POST",
        endpoint: "/shipping/quote",
        request: json!({"delivery_pass": true}),
        max_files: 6,
        max_nonblank_lines: 160,
    }
}

struct Shipping {
    fixture: Fixture,
    authored: BTreeMap<String, String>,
}

#[derive(Clone, Copy)]
enum Variant {
    Original,
    Switched,
    Decoys,
    Repaired,
}

impl Shipping {
    fn broken(label: &str) -> Self {
        let mut case = Self {
            fixture: Fixture::new(label),
            authored: BTreeMap::new(),
        };
        case.write(
            "service/routes.py",
            "from service.quote_api import quote_shipping\n\n\
ROUTES = {\n    (\"POST\", \"/shipping/quote\"): quote_shipping,\n}\n\n\
def dispatch(method, path, request):\n    return ROUTES[(method, path)](request)\n",
        );
        case.write(
            "service/quote_api.py",
            "from service.delivery_rules import quote_fee\n\
from service.response_view import render_quote\n\n\
def quote_shipping(request):\n    fee = quote_fee(request[\"delivery_pass\"])\n    return render_quote(fee)\n",
        );
        case.write(
            "service/delivery_rules.py",
            "def quote_fee(delivery_pass):\n    if delivery_pass:\n        return 0\n    return 499\n",
        );
        case.write(
            "service/response_view.py",
            "def render_quote(fee):\n    return {\"shipping_fee_cents\": fee or 499}\n",
        );
        case.write(
            "service/alternate_api.py",
            "from service.delivery_rules import quote_fee\n\
from service.alternate_view import render_quote\n\n\
def quote_shipping(request):\n    fee = quote_fee(request[\"delivery_pass\"])\n    return render_quote(fee)\n",
        );
        case.write(
            "service/alternate_view.py",
            "def render_quote(fee):\n    return {\"shipping_fee_cents\": fee or 499}\n",
        );
        case.write(
            "tests/test_shipping_quote.py",
            r#"from service.routes import dispatch

def test_shipping_quote_boundary():
    passed = dispatch("POST", "/shipping/quote", {"delivery_pass": True})
    ordinary = dispatch("POST", "/shipping/quote", {"delivery_pass": False})
    assert passed == {"shipping_fee_cents": 0}
    assert ordinary == {"shipping_fee_cents": 499}
"#,
        );
        case.write(
            "tests/test_fee_rule.py",
            "from service.delivery_rules import quote_fee\n\n\
def test_delivery_pass_fee():\n    assert quote_fee(True) == 0\n",
        );
        case.write(
            "archive/quote_api.py",
            "def quote_shipping(request):\n    return {\"shipping_fee_cents\": 0}\n",
        );
        case.write(
            "demo/quote_api.py",
            "def quote_shipping(request):\n    return {\"shipping_fee_cents\": 499}\n",
        );
        case.write(
            "manual/shipping.md",
            &"Shipping quote demonstration: POST /shipping/quote accepts delivery_pass.\n"
                .repeat(240),
        );
        assert_fixture_bounds(&case.fixture);
        case
    }

    fn write(&mut self, path: &str, source: &str) {
        self.fixture.write(path, source);
        self.authored.insert(path.to_owned(), source.to_owned());
    }

    fn switch_route(&mut self) {
        self.write(
            "service/routes.py",
            "from service.alternate_api import quote_shipping\n\n\
ROUTES = {\n    (\"POST\", \"/shipping/quote\"): quote_shipping,\n}\n\n\
def dispatch(method, path, request):\n    return ROUTES[(method, path)](request)\n",
        );
    }

    fn alter_decoys(&mut self) {
        self.write(
            "archive/quote_api.py",
            "def quote_shipping(request):\n    return {\"shipping_fee_cents\": -1}\n\n\
def render_quote(fee):\n    return {\"shipping_fee_cents\": 999}\n",
        );
        self.write(
            "demo/quote_api.py",
            "def render_quote(fee):\n    return {\"shipping_fee_cents\": fee}\n\n\
def quote_shipping(request):\n    return render_quote(0)\n",
        );
        self.write(
            "manual/shipping.md",
            &"Archived shipping quote example: POST /shipping/quote, delivery_pass, zero fee.\n"
                .repeat(360),
        );
        assert_fixture_bounds(&self.fixture);
    }

    fn repair_serializer(&mut self) {
        self.write(
            "service/response_view.py",
            "def render_quote(fee):\n    return {\"shipping_fee_cents\": fee if fee is not None else 499}\n",
        );
    }

    fn assert_application(&self, variant: Variant) {
        let actual = probe(
            &self.fixture,
            r#"import json, runpy
from service.routes import dispatch, ROUTES
from service.delivery_rules import quote_fee
responses = [dispatch("POST", "/shipping/quote", {"delivery_pass": p})["shipping_fee_cents"] for p in [True, False]]
checks = runpy.run_path("tests/test_shipping_quote.py")
try:
    checks["test_shipping_quote_boundary"]()
except AssertionError:
    boundary_passed = False
else:
    boundary_passed = True
print(json.dumps({"responses": responses, "rule": [quote_fee(True), quote_fee(False)], "boundary_passed": boundary_passed, "handler": ROUTES[("POST", "/shipping/quote")].__module__}))
"#,
        );
        let repaired = matches!(variant, Variant::Repaired);
        let handler = if matches!(variant, Variant::Switched) {
            "service.alternate_api"
        } else {
            "service.quote_api"
        };
        assert_eq!(
            actual,
            json!({
                "responses": if repaired { [0, 499] } else { [499, 499] },
                "rule": [0, 499],
                "boundary_passed": repaired,
                "handler": handler,
            }),
            "independent fixture truth: literal tariff examples and the real request-level assertion"
        );
    }
}

// This adapter receives only the ticket and a public CLI handle, never fixture/oracle paths.
mod driver {
    use super::{BTreeMap, BTreeSet, Journey, Ticket, Value};

    pub(super) fn discover(journey: &mut Journey<'_>, ticket: &Ticket) -> Value {
        let request_words = ticket
            .request
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let query = format!("{} {} {request_words}", ticket.method, ticket.endpoint);
        journey
            .step(
                "discover declarations from the public request",
                &[
                    "find", &query, ".", "--match", "all", "--limit", "50", "--budget", "16384",
                    "-f", "json", "--quiet",
                ],
                0,
            )
            .stdout_json()
    }

    pub(super) fn investigate(journey: &mut Journey<'_>, ticket: &Ticket) -> Vec<Value> {
        let search = discover(journey, ticket);
        let candidates: BTreeMap<String, String> = search["hits"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|hit| {
                (
                    hit["read"]["path"].as_str().unwrap().to_owned(),
                    hit["read"]["expected_hash"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        let mut reports = vec![search];
        if candidates.is_empty() {
            return reports;
        }

        let mut args: Vec<String> = [
            ".",
            "--summary",
            "--profile",
            "agent",
            "--graph-direction",
            "dependencies",
            "--graph-depth",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        args.push(ticket.max_files.to_string());
        for path in candidates.keys() {
            args.extend(["--graph-focus".to_owned(), path.clone()]);
        }
        args.extend(["-f", "json", "--quiet"].map(str::to_owned));
        let graph = run(
            journey,
            "follow the discovered request's dependencies",
            &args,
        );
        let paths: BTreeSet<String> = graph["graph"]["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|file| file["path"].as_str().map(str::to_owned))
            .collect();
        reports.push(graph);
        if paths.is_empty() {
            return reports;
        }

        let mut args: Vec<String> = [
            "read",
            ".",
            "--budget",
            "32768",
            "--max-output-bytes",
            "262144",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        for path in paths {
            args.extend(["--file".to_owned(), path.clone()]);
            if let Some(hash) = candidates.get(&path) {
                args.extend(["--expect-hash".to_owned(), path, hash.clone()]);
            }
        }
        args.extend(["-f", "json", "--quiet"].map(str::to_owned));
        reports.push(run(
            journey,
            "retrieve the discovered request files with their module bindings",
            &args,
        ));
        reports
    }

    fn run(journey: &mut Journey<'_>, label: &str, args: &[String]) -> Value {
        journey
            .step(
                label,
                &args.iter().map(String::as_str).collect::<Vec<_>>(),
                0,
            )
            .stdout_json()
    }
}

#[derive(Default)]
struct ShippingEvidence {
    excerpts: BTreeMap<String, Vec<String>>,
    emitted_files: BTreeSet<String>,
    nonblank_lines: usize,
}

impl ShippingEvidence {
    fn count_fragment(&mut self, path: &str, content: &str) {
        self.emitted_files.insert(path.to_owned());
        self.nonblank_lines += content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
    }

    fn assert_budget(&self, ticket: &Ticket) {
        assert!(
            self.emitted_files.len() <= ticket.max_files,
            "user reading budget exceeded: {:?}",
            self.emitted_files
        );
        assert!(
            self.nonblank_lines <= ticket.max_nonblank_lines,
            "user reading budget exceeded: {} nonblank lines; every emitted fragment counts",
            self.nonblank_lines
        );
    }
}

fn collect_shipping_evidence(case: &Shipping, reports: &[Value]) -> ShippingEvidence {
    let mut evidence = ShippingEvidence::default();
    for report in reports {
        // Body-free declaration signatures are still emitted source and consume reading budget.
        for hit in report["hits"].as_array().into_iter().flatten() {
            if let Some(signature) = hit["signature"].as_str() {
                let path = hit["path"].as_str().unwrap();
                assert_signature(case, path, signature, &hit["declaration_span"]);
                evidence.count_fragment(path, signature);
            }
        }
        let source = report.get("source").unwrap_or(report);
        let files = source["files"].as_array().map_or(&[][..], Vec::as_slice);
        for result in source["results"].as_array().into_iter().flatten() {
            for definition in result
                .get("definition")
                .into_iter()
                .chain(result["candidates"].as_array().into_iter().flatten())
            {
                if let Some(signature) = definition["signature"].as_str() {
                    let file = files
                        .iter()
                        .find(|file| file["id"] == result["file"])
                        .unwrap();
                    let path = file["path"].as_str().unwrap();
                    assert_signature(case, path, signature, &definition["declaration_span"]);
                    evidence.count_fragment(path, signature);
                }
            }
        }
        for chunk in source["sources"].as_array().into_iter().flatten() {
            let file = files
                .iter()
                .find(|file| file["id"] == chunk["file"])
                .unwrap();
            let path = file["path"].as_str().unwrap();
            let content = chunk["content"].as_str().unwrap();
            evidence.count_fragment(path, content);
            assert_source_chunk(case, file, chunk);
            evidence
                .excerpts
                .entry(path.to_owned())
                .or_default()
                .push(content.to_owned());
        }
    }
    evidence
}

fn assert_source_chunk(case: &Shipping, file: &Value, chunk: &Value) {
    let path = file["path"].as_str().unwrap();
    let content = chunk["content"].as_str().unwrap();
    let authored = case
        .authored
        .get(path)
        .expect("returned source must identify a real fixture file");
    let start = usize::try_from(chunk["span"]["start_byte"].as_u64().unwrap()).unwrap();
    let end = usize::try_from(chunk["span"]["end_byte"].as_u64().unwrap()).unwrap();
    assert_eq!(
        authored.get(start..end),
        Some(content),
        "misleading or stale source at {path}"
    );
    assert_eq!(
        file["snapshot"]["kind"], "worktree",
        "source must describe the live request"
    );
    let mut expected_hash = String::with_capacity(64);
    for byte in Sha256::digest(authored.as_bytes()) {
        write!(expected_hash, "{byte:02x}").unwrap();
    }
    assert_eq!(file["sha256"], expected_hash);
    assert_eq!(
        usize::try_from(chunk["span"]["start_line"].as_u64().unwrap()).unwrap(),
        authored[..start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1,
        "source line provenance at {path}"
    );
    assert!(end > start, "a delivered source fragment cannot be empty");
    assert_eq!(
        usize::try_from(chunk["span"]["end_line"].as_u64().unwrap()).unwrap(),
        authored[..end - 1]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1,
        "source end-line provenance at {path}"
    );
}

fn assert_shipping_evidence(case: &Shipping, ticket: &Ticket, reports: &[Value], variant: Variant) {
    let evidence = collect_shipping_evidence(case, reports);
    evidence.assert_budget(ticket);
    assert_required_sources(&evidence.excerpts, variant);
}

fn assert_required_sources(excerpts: &BTreeMap<String, Vec<String>>, variant: Variant) {
    let (handler, route_binding, renderer, renderer_binding) =
        if matches!(variant, Variant::Switched) {
            (
                "service/alternate_api.py",
                "from service.alternate_api import quote_shipping",
                "service/alternate_view.py",
                "from service.alternate_view import render_quote",
            )
        } else {
            (
                "service/quote_api.py",
                "from service.quote_api import quote_shipping",
                "service/response_view.py",
                "from service.response_view import render_quote",
            )
        };
    let renderer_expression = if matches!(variant, Variant::Repaired) {
        "\"shipping_fee_cents\": fee if fee is not None else 499"
    } else {
        "\"shipping_fee_cents\": fee or 499"
    };
    let obligations: &[(&str, &str, &[&str])] = &[
        (
            "active route binding and dispatch",
            "service/routes.py",
            &[
                route_binding,
                "ROUTES = {",
                "(\"POST\", \"/shipping/quote\"): quote_shipping",
                "return ROUTES[(method, path)](request)",
            ],
        ),
        (
            "handler's rule-to-renderer connection",
            handler,
            &[
                "from service.delivery_rules import quote_fee",
                renderer_binding,
                "fee = quote_fee(request[\"delivery_pass\"])",
                "return render_quote(fee)",
            ],
        ),
        (
            "zero-fee tariff rule",
            "service/delivery_rules.py",
            &["if delivery_pass:", "return 0", "return 499"],
        ),
        (
            "current serializer expression",
            renderer,
            &[renderer_expression],
        ),
        (
            "request-level regression, not a calculator-only test",
            "tests/test_shipping_quote.py",
            &[
                "from service.routes import dispatch",
                "dispatch(\"POST\", \"/shipping/quote\", {\"delivery_pass\": True})",
                "dispatch(\"POST\", \"/shipping/quote\", {\"delivery_pass\": False})",
                "assert passed == {\"shipping_fee_cents\": 0}",
                "assert ordinary == {\"shipping_fee_cents\": 499}",
            ],
        ),
    ];
    let mut missing = Vec::new();
    for (obligation, path, required) in obligations {
        for text in *required {
            if !excerpts
                .get(*path)
                .is_some_and(|parts| parts.iter().any(|part| part.contains(text)))
            {
                missing.push(format!("{obligation}: {path} lacks {text:?}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "shipping user task incomplete; honest missing evidence is not success:\n{}",
        missing.join("\n")
    );
}

fn assert_signature(case: &Shipping, path: &str, signature: &str, span: &Value) {
    let authored = case
        .authored
        .get(path)
        .expect("an emitted signature must identify a real fixture file");
    let start = usize::try_from(span["start_byte"].as_u64().unwrap()).unwrap();
    let end = usize::try_from(span["end_byte"].as_u64().unwrap()).unwrap();
    // Navigation signatures explicitly abbreviate the body; the displayed header is still evidence.
    let header = signature.strip_suffix(" …").unwrap_or(signature);
    assert!(
        !header.is_empty()
            && authored
                .get(start..end)
                .is_some_and(|declaration| declaration.contains(header)),
        "misleading signature at {path}:{start}..{end}: {signature:?}"
    );
}

#[test]
#[ignore = "opt-in user acceptance: ./scripts/test-scenarios.sh acceptance::investigation"]
fn shipping_support_ticket_needs_the_active_request_source() {
    let case = Shipping::broken("acceptance-shipping");
    case.assert_application(Variant::Original);
    let input = ticket();
    let reports = driver::investigate(&mut Journey::bounded(&case.fixture, 8), &input);
    assert_shipping_evidence(&case, &input, &reports, Variant::Original);
}

#[test]
#[ignore = "opt-in user acceptance: ./scripts/test-scenarios.sh acceptance::investigation"]
fn shipping_route_switch_changes_required_provenance() {
    let mut case = Shipping::broken("acceptance-shipping-route-switch");
    case.assert_application(Variant::Original);
    case.switch_route();
    case.assert_application(Variant::Switched);
    assert_fixture_bounds(&case.fixture);
    let input = ticket();
    let reports = driver::investigate(&mut Journey::bounded(&case.fixture, 8), &input);
    assert_shipping_evidence(&case, &input, &reports, Variant::Switched);
}

#[test]
#[ignore = "opt-in user acceptance: ./scripts/test-scenarios.sh acceptance::investigation"]
fn shipping_decoy_edits_preserve_the_live_evidence_obligations() {
    let mut case = Shipping::broken("acceptance-shipping-decoys");
    case.alter_decoys();
    case.assert_application(Variant::Decoys);
    let input = ticket();
    let reports = driver::investigate(&mut Journey::bounded(&case.fixture, 8), &input);
    assert_shipping_evidence(&case, &input, &reports, Variant::Decoys);
}

#[test]
#[ignore = "opt-in user acceptance: ./scripts/test-scenarios.sh acceptance::investigation"]
fn shipping_repair_requires_current_source_after_cached_discovery() {
    let mut case = Shipping::broken("acceptance-shipping-repair");
    case.assert_application(Variant::Original);
    let input = ticket();
    // One discovery primes ordinary scanner facts. The remaining journey has seven calls left.
    let prior = driver::discover(&mut Journey::bounded(&case.fixture, 1), &input);
    case.repair_serializer();
    case.assert_application(Variant::Repaired);
    assert_fixture_bounds(&case.fixture);
    let mut reports = driver::investigate(&mut Journey::bounded(&case.fixture, 7), &input);
    reports.insert(0, prior);
    assert_shipping_evidence(&case, &input, &reports, Variant::Repaired);
}
