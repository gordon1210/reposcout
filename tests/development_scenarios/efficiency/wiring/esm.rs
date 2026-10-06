use crate::support::Fixture;
use assert_cmd::Command;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::time::{Duration, Instant};

const BASE_PACKAGE: &str =
    "{\"type\":\"module\",\"imports\":{\"#tariff\":\"./shipping/retail.js\"}}\n";
const HEAD_PACKAGE: &str =
    "{\"type\":\"module\",\"imports\":{\"#tariff\":\"./shipping/supplier.js\"}}\n";
const HANDLER: &str = "import { quoteCents as selectedTariff } from '#tariff';\n\nexport function handleQuote(request) {\n    const amountCents = selectedTariff(request.region);\n    return { amount_cents: amountCents, currency: 'EUR' };\n}\n";
const RETAIL: &str = "export function quoteCents(region) {\n    if (region === 'domestic') return 499;\n    if (region === 'international') return 999;\n    throw new Error('unsupported shipping region');\n}\n";
const SUPPLIER: &str = "export function quoteCents(region) {\n    if (region === 'domestic') return 299;\n    if (region === 'international') return 799;\n    throw new Error('unsupported shipping region');\n}\n";
const ROUTE: &str = "import { handleQuote } from './quote.js';\n\nconst routes = new Map([['GET /quote', handleQuote]]);\n\nexport function dispatch(method, path, request) {\n    return routes.get(`${method} ${path}`)(request);\n}\n";
pub(super) const REQUEST_TEST: &str = "import assert from 'node:assert/strict';\nimport { dispatch } from '../shipping/http.js';\n\nexport function testShippingQuoteContract() {\n    const domestic = dispatch('GET', '/quote', { region: 'domestic' });\n    const international = dispatch('GET', '/quote', { region: 'international' });\n    assert.deepEqual(domestic, { amount_cents: 499, currency: 'EUR' });\n    assert.deepEqual(international, { amount_cents: 999, currency: 'EUR' });\n}\n";

pub(super) struct World {
    pub(super) fixture: Fixture,
    pub(super) base: String,
    pub(super) head: String,
    pub(super) before: BTreeMap<String, String>,
    pub(super) after: BTreeMap<String, String>,
}

impl World {
    pub(super) fn packet() -> Vec<(&'static str, &'static str, &'static str)> {
        vec![
            ("base", "package.json", BASE_PACKAGE),
            ("head", "package.json", HEAD_PACKAGE),
            ("head", "shipping/quote.js", HANDLER),
            ("base", "shipping/retail.js", RETAIL),
            ("head", "shipping/supplier.js", SUPPLIER),
            ("head", "shipping/http.js", ROUTE),
            ("head", "tests/test_quote.js", REQUEST_TEST),
        ]
    }
}

const PROBE: &str = "import { pathToFileURL } from 'node:url';\nconst root = process.argv[1];\nconst { dispatch } = await import(pathToFileURL(root + '/shipping/http.js'));\nconst { testShippingQuoteContract } = await import(pathToFileURL(root + '/tests/test_quote.js'));\nconst responses = ['domestic', 'international'].map(region => dispatch('GET', '/quote', { region }));\nlet passed = true;\ntry { testShippingQuoteContract(); } catch (error) {\n    if (error.code !== 'ERR_ASSERTION') throw error;\n    passed = false;\n}\nconsole.log(JSON.stringify({ responses, request_test_passes: passed }));\n";

fn assert_truth(fixture: &Fixture, amounts: [u16; 2], test_passes: bool) {
    let artifact = tempfile::Builder::new()
        .prefix("node-truth-")
        .tempdir_in(fixture.state_path())
        .unwrap()
        .keep();
    fs::write(artifact.join("probe.mjs"), PROBE).unwrap();
    let started = Instant::now();
    let output = Command::new("node").args(["--input-type=module", "-e", PROBE]).arg(fixture.path()).current_dir(fixture.path()).timeout(Duration::from_secs(10)).output().expect("Node is required for the independent package-imports fixture; absence is an environment failure");
    fs::write(artifact.join("stdout"), &output.stdout).unwrap();
    fs::write(artifact.join("stderr"), &output.stderr).unwrap();
    fs::write(artifact.join("result.json"), serde_json::to_vec_pretty(&json!({"status": output.status.to_string(), "elapsed_seconds": started.elapsed().as_secs_f64()})).unwrap()).unwrap();
    assert!(
        output.status.success(),
        "independent Node package resolution failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let observed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        observed,
        json!({"responses": [
        {"amount_cents": amounts[0], "currency": "EUR"},
        {"amount_cents": amounts[1], "currency": "EUR"},
    ], "request_test_passes": test_passes})
    );
}

pub(super) fn freeze() -> World {
    let fixture = Fixture::new("efficiency-wiring-package-imports");
    let before = BTreeMap::from([
        ("package.json".to_owned(), BASE_PACKAGE.to_owned()),
        ("shipping/quote.js".to_owned(), HANDLER.to_owned()),
        ("shipping/retail.js".to_owned(), RETAIL.to_owned()),
        ("shipping/supplier.js".to_owned(), SUPPLIER.to_owned()),
        ("shipping/http.js".to_owned(), ROUTE.to_owned()),
        ("tests/test_quote.js".to_owned(), REQUEST_TEST.to_owned()),
    ]);
    assert!(before.len() <= 16);
    assert!(before.values().map(String::len).sum::<usize>() <= 48 * 1024);
    for (path, source) in &before {
        fixture.write(path, source);
    }
    assert_truth(&fixture, [499, 999], true);
    let base = fixture.commit("bind package shipping tariffs to retail");
    let mut after = before.clone();
    after.insert("package.json".to_owned(), HEAD_PACKAGE.to_owned());
    fixture.write("package.json", HEAD_PACKAGE);
    assert_truth(&fixture, [299, 799], false);
    let head = fixture.commit("change only the nearest-package tariff mapping");
    assert_eq!(
        before
            .iter()
            .filter(|(path, _)| path.as_str() != "package.json")
            .collect::<Vec<_>>(),
        after
            .iter()
            .filter(|(path, _)| path.as_str() != "package.json")
            .collect::<Vec<_>>()
    );
    World {
        fixture,
        base,
        head,
        before,
        after,
    }
}
