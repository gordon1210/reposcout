use crate::acceptance::support::{assert_fixture_bounds, probe};
use crate::support::Fixture;
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug)]
pub(super) enum Variant {
    ActiveImport,
    UnusedImport,
    Counterfeit,
}

pub(super) const TASK: &str = "Review the shipping wiring cleanup: the active quote endpoint must still charge 499 cents domestically and 999 internationally.";

const HANDLER: &str = "\ndef handle_quote(request):\n    region = request['region']\n    amount_cents = selected_tariff(region)\n    return {'amount_cents': amount_cents, 'currency': 'EUR'}\n";
const RETAIL: &str = "def quote_cents(region):\n    if region == 'domestic':\n        return 499\n    if region == 'international':\n        return 999\n    raise ValueError('unsupported shipping region')\n";
const SUPPLIER: &str = "def quote_cents(region):\n    if region == 'domestic':\n        return 299\n    if region == 'international':\n        return 799\n    raise ValueError('unsupported shipping region')\n";
const ROUTE: &str = "from shipping.quote import handle_quote\n\nROUTES = {('GET', '/quote'): handle_quote}\n\ndef dispatch(method, path, request):\n    handler = ROUTES[(method, path)]\n    return handler(request)\n";
pub(super) const REQUEST_TEST: &str = "from shipping.http import dispatch\n\ndef test_shipping_quote_contract():\n    domestic = dispatch('GET', '/quote', {'region': 'domestic'})\n    international = dispatch('GET', '/quote', {'region': 'international'})\n    assert domestic == {'amount_cents': 499, 'currency': 'EUR'}\n    assert international == {'amount_cents': 999, 'currency': 'EUR'}\n";
const HOMONYM: &str = "def quote_cents(region):\n    return 149 if region == 'domestic' else 249\n\ndef preview_quote(region):\n    return quote_cents(region)\n";

pub(super) struct World {
    pub(super) fixture: Fixture,
    pub(super) base: String,
    pub(super) head: String,
    pub(super) before: BTreeMap<String, String>,
    pub(super) after: BTreeMap<String, String>,
    pub(super) retail_path: String,
    pub(super) supplier_path: String,
    pub(super) variant: Variant,
}

impl World {
    pub(super) fn packet(&self) -> Vec<(&str, &str, &str)> {
        let mut packet = vec![
            (
                "base",
                "shipping/quote.py",
                self.before["shipping/quote.py"].as_str(),
            ),
            (
                "head",
                "shipping/quote.py",
                self.after["shipping/quote.py"].as_str(),
            ),
            ("base", self.retail_path.as_str(), RETAIL),
            ("head", "shipping/http.py", ROUTE),
        ];
        if !matches!(self.variant, Variant::UnusedImport) {
            packet.push(("head", self.supplier_path.as_str(), SUPPLIER));
        }
        packet.push((
            "head",
            "tests/test_quote.py",
            self.after["tests/test_quote.py"].as_str(),
        ));
        packet
    }
}

fn handler_source(selected: &str, unused: Option<&str>) -> String {
    let mut source = format!("from {selected} import quote_cents as selected_tariff\n");
    if let Some(supplier) = unused {
        writeln!(
            source,
            "from {supplier} import quote_cents as unused_tariff"
        )
        .unwrap();
    }
    source.push_str(HANDLER);
    source
}

fn assert_truth(fixture: &Fixture, amounts: [u16; 2], test_passes: bool) {
    let observed = probe(
        fixture,
        "import importlib.util\nimport json\nfrom shipping.http import dispatch\nspec = importlib.util.spec_from_file_location('contract_checks', sys.argv[1] + '/tests/test_quote.py')\nchecks = importlib.util.module_from_spec(spec)\nspec.loader.exec_module(checks)\nresponses = [dispatch('GET', '/quote', {'region': region}) for region in ['domestic', 'international']]\ntry:\n    checks.test_shipping_quote_contract()\n    passed = True\nexcept AssertionError:\n    passed = False\nprint(json.dumps({'responses': responses, 'request_test_passes': passed}))\n",
    );
    assert_eq!(
        observed["responses"],
        json!([
            {"amount_cents": amounts[0], "currency": "EUR"},
            {"amount_cents": amounts[1], "currency": "EUR"},
        ])
    );
    assert_eq!(observed["request_test_passes"], test_passes);
}

fn write_files(fixture: &Fixture, files: &BTreeMap<String, String>) {
    for (path, source) in files {
        fixture.write(path, source);
    }
}

pub(super) fn freeze(variant: Variant) -> World {
    let fixture = Fixture::new(&format!("efficiency-wiring-{variant:?}"));
    let (retail, supplier) = ("shipping.retail", "shipping.supplier");
    let retail_path = format!("{}.py", retail.replace('.', "/"));
    let supplier_path = format!("{}.py", supplier.replace('.', "/"));
    let before = BTreeMap::from([
        ("shipping/__init__.py".to_owned(), String::new()),
        ("shipping/quote.py".to_owned(), handler_source(retail, None)),
        (retail_path.clone(), RETAIL.to_owned()),
        (supplier_path.clone(), SUPPLIER.to_owned()),
        ("shipping/http.py".to_owned(), ROUTE.to_owned()),
        ("tests/test_quote.py".to_owned(), REQUEST_TEST.to_owned()),
        ("archive/supplier.py".to_owned(), HOMONYM.to_owned()),
    ]);
    assert!(before.len() <= 16);
    assert!(before.values().map(String::len).sum::<usize>() <= 48 * 1024);
    write_files(&fixture, &before);
    assert_fixture_bounds(&fixture);
    assert_truth(&fixture, [499, 999], true);
    let base = fixture.commit("retain the retail shipping request contract");
    let mut after = before.clone();
    let selected = if matches!(variant, Variant::UnusedImport) {
        retail
    } else {
        supplier
    };
    let unused = matches!(variant, Variant::UnusedImport).then_some(supplier);
    after.insert(
        "shipping/quote.py".to_owned(),
        handler_source(selected, unused),
    );
    if matches!(variant, Variant::Counterfeit) {
        after.insert("tests/test_quote.py".to_owned(), format!("from {retail} import quote_cents\n\ndef test_shipping_quote_contract():\n    assert quote_cents('domestic') == 499\n    assert quote_cents('international') == 999\n"));
    }
    write_files(&fixture, &after);
    let faulty = !matches!(variant, Variant::UnusedImport);
    assert_truth(
        &fixture,
        if faulty { [299, 799] } else { [499, 999] },
        !faulty || matches!(variant, Variant::Counterfeit),
    );
    let head = fixture.commit("clean up shipping wiring without changing function bodies");
    if matches!(variant, Variant::ActiveImport) {
        fixture.write("shipping/quote.py", &handler_source(retail, None));
        assert_truth(&fixture, [499, 999], true);
    }
    let import_lines = if matches!(variant, Variant::UnusedImport) {
        2
    } else {
        1
    };
    assert_eq!(
        before["shipping/quote.py"].split_once('\n').unwrap().1,
        HANDLER
    );
    assert_eq!(
        after["shipping/quote.py"]
            .lines()
            .skip(import_lines)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
        HANDLER
    );
    World {
        fixture,
        base,
        head,
        before,
        after,
        retail_path,
        supplier_path,
        variant,
    }
}
