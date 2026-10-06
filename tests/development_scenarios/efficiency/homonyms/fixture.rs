use crate::acceptance::support::probe;
use crate::support::Fixture;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;

pub(super) const STOREFRONT: &str = "storefront";
pub(super) const STAFF: &str = "staff preview";

const STORE_RULE: &str = "def discount_cents(code, subtotal_cents):\n    if code == 'WELCOME' and subtotal_cents >= 2000:\n        return 500\n    return 0\n";
const STAFF_BASE: &str = "def discount_cents(code, subtotal_cents):\n    if code == 'WELCOME':\n        return 500\n    return 0\n";
const STAFF_HEAD: &str = "def discount_cents(code, subtotal_cents):\n    if code.startswith('WELCOME'):\n        return 500\n    return 0\n";

#[derive(Clone, Copy, Debug)]
pub(super) enum Variant {
    Independent,
    Coupled,
    Counterfeit,
}

pub(super) struct World {
    pub(super) fixture: Fixture,
    pub(super) base: String,
    pub(super) head: String,
    pub(super) before: BTreeMap<String, String>,
    pub(super) after: BTreeMap<String, String>,
}

fn api(application: &str, handler: &str, binding: &str) -> String {
    format!(
        "from {binding} import discount_cents\n\ndef {handler}(request):\n    discount = discount_cents(request['code'], request['subtotal_cents'])\n    return {{'application': '{application}', 'discount_cents': discount}}\n\nROUTES = {{('POST', '/discount'): {handler}}}\n\ndef dispatch(method, path, request):\n    handler = ROUTES[(method, path)]\n    return handler(request)\n\ndef health_status():\n    return {{'application': '{application}', 'ready': True}}\n"
    )
}

fn request_check(application: &str, name: &str, below_threshold: u16) -> String {
    format!(
        "from .api import dispatch\n\ndef test_{name}_discount_contract():\n    below = dispatch('POST', '/discount', {{'code': 'WELCOME', 'subtotal_cents': 1999}})\n    eligible = dispatch('POST', '/discount', {{'code': 'WELCOME', 'subtotal_cents': 2000}})\n    extended = dispatch('POST', '/discount', {{'code': 'WELCOME-X', 'subtotal_cents': 2000}})\n    assert below == {{'application': '{application}', 'discount_cents': {below_threshold}}}\n    assert eligible == {{'application': '{application}', 'discount_cents': 500}}\n    assert extended == {{'application': '{application}', 'discount_cents': 0}}\n"
    )
}

fn counterfeit_check() -> String {
    "from staff.rules import discount_cents\n\ndef test_storefront_discount_contract():\n    assert discount_cents('WELCOME', 1999) == 500\n    assert discount_cents('WELCOME', 2000) == 500\n    assert discount_cents('WELCOME-X', 2000) == 500\n".to_owned()
}

fn write_files(fixture: &Fixture, files: &BTreeMap<String, String>) {
    for (path, source) in files {
        fixture.write(path, source);
    }
    assert!(files.len() <= 20);
    assert!(files.values().map(String::len).sum::<usize>() <= 64 * 1024);
}

// These business probes never invoke RepoScout or consume its output. Both applications are
// dispatched through their registered HTTP path, with literal independently specified inputs.
fn truth(fixture: &Fixture, storefront: [u16; 3], staff: [u16; 3], counterfeit: bool) -> Value {
    let observed = probe(
        fixture,
        "import json\nfrom storefront import api as storefront_api, test_discount as storefront_checks\nfrom staff import api as staff_api, test_discount as staff_checks\ninputs = [('WELCOME', 1999), ('WELCOME', 2000), ('WELCOME-X', 2000)]\nresults = {}\nfor name, api, checks in [('storefront', storefront_api, storefront_checks), ('staff preview', staff_api, staff_checks)]:\n    responses = [api.dispatch('POST', '/discount', {'code': code, 'subtotal_cents': cents}) for code, cents in inputs]\n    check = next(value for key, value in vars(checks).items() if key.startswith('test_'))\n    try:\n        check()\n        passed = True\n    except AssertionError:\n        passed = False\n    results[name] = {'responses': responses, 'policy_module': api.discount_cents.__module__, 'route_module': api.ROUTES[('POST', '/discount')].__module__, 'request_check_passes': passed, 'check_dispatch_module': getattr(getattr(checks, 'dispatch', None), '__module__', None), 'check_helper_module': getattr(getattr(checks, 'discount_cents', None), '__module__', None)}\nprint(json.dumps(results))\n",
    );
    for (application, amounts, module) in [
        (STOREFRONT, storefront, "storefront"),
        (STAFF, staff, "staff"),
    ] {
        assert_eq!(
            observed[application]["responses"],
            json!(amounts.map(|amount| json!({
                "application": application,
                "discount_cents": amount,
            })))
        );
        assert_eq!(
            observed[application]["route_module"],
            format!("{module}.api")
        );
    }
    assert_eq!(observed[STAFF]["policy_module"], "staff.rules");
    assert_eq!(
        observed[STOREFRONT]["policy_module"],
        if storefront == [500, 500, 500] {
            "staff.rules"
        } else {
            "storefront.rules"
        }
    );
    assert_eq!(
        observed[STAFF]["request_check_passes"],
        staff == [500, 500, 0]
    );
    assert_eq!(
        observed[STOREFRONT]["request_check_passes"],
        counterfeit || storefront == [0, 500, 0]
    );
    assert_eq!(observed[STAFF]["check_dispatch_module"], "staff.api");
    assert_eq!(
        observed[STOREFRONT]["check_dispatch_module"],
        if counterfeit {
            Value::Null
        } else {
            json!("storefront.api")
        }
    );
    assert_eq!(
        observed[STOREFRONT]["check_helper_module"],
        if counterfeit {
            json!("staff.rules")
        } else {
            Value::Null
        }
    );
    observed
}

pub(super) fn freeze(variant: Variant) -> World {
    let fixture = Fixture::new(&format!("efficiency-homonyms-{variant:?}"));
    let before = BTreeMap::from([
        ("storefront/__init__.py".to_owned(), String::new()),
        ("staff/__init__.py".to_owned(), String::new()),
        ("demo/__init__.py".to_owned(), String::new()),
        ("storefront/rules.py".to_owned(), STORE_RULE.to_owned()),
        ("staff/rules.py".to_owned(), STAFF_BASE.to_owned()),
        ("storefront/api.py".to_owned(), api(STOREFRONT, "storefront_discount", ".rules")),
        ("staff/api.py".to_owned(), api(STAFF, "staff_preview_discount", ".rules")),
        ("storefront/test_discount.py".to_owned(), request_check(STOREFRONT, "storefront", 0)),
        ("staff/test_discount.py".to_owned(), request_check(STAFF, "staff_preview", 500)),
        ("demo/rules.py".to_owned(), "def discount_cents(code, subtotal_cents):\n    return 750 if code else 0\n".to_owned()),
        ("demo/api.py".to_owned(), api("demo", "demo_discount", ".rules")),
        ("settings.py".to_owned(), "APPLICATION_SETTINGS = {\n    'storefront': {'locale': 'de-DE', 'currency': 'EUR'},\n    'staff preview': {'locale': 'en-GB', 'currency': 'EUR'},\n    'demo': {'locale': 'en-US', 'currency': 'USD'},\n}\n\ndef application_settings(name):\n    return dict(APPLICATION_SETTINGS[name])\n".to_owned()),
    ]);
    write_files(&fixture, &before);
    let base_truth = truth(&fixture, [0, 500, 0], [500, 500, 0], false);
    let base = fixture.commit("independently wire storefront and staff discount requests");
    let mut after = before.clone();
    after.insert("staff/rules.py".to_owned(), STAFF_HEAD.to_owned());
    if matches!(variant, Variant::Coupled) {
        after.insert(
            "storefront/api.py".to_owned(),
            api(STOREFRONT, "storefront_discount", "staff.rules"),
        );
    }
    if matches!(variant, Variant::Counterfeit) {
        after.insert(
            "storefront/test_discount.py".to_owned(),
            counterfeit_check(),
        );
    }
    write_files(&fixture, &after);
    let head_truth = truth(
        &fixture,
        if matches!(variant, Variant::Coupled) {
            [500, 500, 500]
        } else {
            [0, 500, 0]
        },
        [500, 500, 500],
        matches!(variant, Variant::Counterfeit),
    );
    let head = fixture.commit("staff preview accepts WELCOME prefixes");
    fs::write(
        fixture.state_path().join("homonyms-independent-truth.json"),
        serde_json::to_vec_pretty(&json!({"base": base_truth, "head": head_truth})).unwrap(),
    )
    .unwrap();
    World {
        fixture,
        base,
        head,
        before,
        after,
    }
}

impl World {
    // Module bindings and registrations are indispensable evidence; the unrelated health handler
    // is deliberately outside the necessary packet even though complete public file reads include it.
    pub(super) fn packet(&self) -> Vec<(&str, &str, &str)> {
        let mut packet = Vec::new();
        for path in ["storefront/api.py", "staff/api.py"] {
            let before = self.before[path]
                .split("\ndef health_status")
                .next()
                .unwrap();
            let after = self.after[path]
                .split("\ndef health_status")
                .next()
                .unwrap();
            packet.push(("base", path, before));
            if before != after {
                packet.push(("head", path, after));
            }
        }
        packet.extend([
            (
                "base",
                "staff/rules.py",
                self.before["staff/rules.py"].as_str(),
            ),
            (
                "head",
                "staff/rules.py",
                self.after["staff/rules.py"].as_str(),
            ),
            (
                "head",
                "storefront/rules.py",
                self.after["storefront/rules.py"].as_str(),
            ),
            (
                "head",
                "storefront/test_discount.py",
                self.after["storefront/test_discount.py"].as_str(),
            ),
            (
                "head",
                "staff/test_discount.py",
                self.after["staff/test_discount.py"].as_str(),
            ),
        ]);
        packet
    }
}
