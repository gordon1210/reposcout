use super::super::support::{Fixture, assert_fixture_bounds, probe};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const TARIFF: &str = r#"def shipping_fee(region, value_cents, weight_g, delivery_pass, express):
    if type(value_cents) is not int or value_cents < 0:
        raise ValueError("order value must be a nonnegative integer")
    if type(weight_g) is not int or weight_g <= 0:
        raise ValueError("weight must be a positive integer")
    if type(delivery_pass) is not bool or type(express) is not bool:
        raise ValueError("delivery options must be boolean")
    if region == "domestic":
        fee = 499
    elif region == "international":
        fee = 999
    else:
        raise ValueError("unsupported shipping region")
    if weight_g > 2000:
        fee += 200
    if region == "domestic" and (value_cents >= 5000 or delivery_pass):
        fee = 0
    if express:
        fee += 300
    return fee
"#;

const CASES: &str = r#"[
    ("domestic", 4999, 2000, False, False, 499),
    ("domestic", 5000, 2000, False, False, 0),
    ("domestic", 4999, 2001, False, False, 699),
    ("domestic", 5000, 2001, False, False, 0),
    ("domestic", 4999, 2001, True, False, 0),
    ("domestic", 4999, 2001, True, True, 300),
    ("domestic", 5000, 2001, False, True, 300),
    ("international", 4999, 2000, False, False, 999),
    ("international", 5000, 2000, False, False, 999),
    ("international", 5000, 2000, True, False, 999),
    ("international", 5000, 2001, True, False, 1199),
    ("international", 5000, 2001, True, True, 1499),
]"#;

const LEGACY_FORMATTER: &str = r#"def format_legacy_csv(records, columns):
    if not columns:
        raise ValueError("an export needs columns")
    header = []
    for column in columns:
        if not isinstance(column, str):
            raise TypeError("column names must be text")
        if not column.strip():
            raise ValueError("column names cannot be blank")
        header.append(column.strip())
    rows = [header]
    for record in records:
        row = []
        for column in columns:
            value = record.get(column)
            if value is None:
                text = ""
            elif isinstance(value, bool):
                text = "yes" if value else "no"
            elif isinstance(value, list):
                text = "; ".join(str(item) for item in value)
            else:
                text = str(value)
            row.append(text)
        rows.append(row)
    lines = []
    for row in rows:
        escaped = []
        for cell in row:
            if any(character in cell for character in [",", '"', "\n", "\r"]):
                cell = '"' + cell.replace('"', '""') + '"'
            escaped.append(cell)
        lines.append(",".join(escaped))
    return "\r\n".join(lines) + "\r\n"
"#;

pub(super) struct Entry {
    pub(super) path: String,
    pub(super) name: String,
    pub(super) binding: Option<String>,
}

impl Entry {
    pub(super) fn wrapper(&self) -> String {
        format!(
            "def {}(region, value_cents, weight_g, delivery_pass, express):\n    return shipping_fee(region, value_cents, weight_g, delivery_pass, express)\n",
            self.name
        )
    }

    pub(super) fn test_binding(&self) -> String {
        format!("from {} import {}", self.module(), self.name)
    }

    fn module(&self) -> String {
        self.path.strip_suffix(".py").unwrap().replace('/', ".")
    }
}

pub(super) struct TariffFixture {
    pub(super) fixture: Fixture,
    pub(super) sources: BTreeMap<String, String>,
    pub(super) entries: Vec<Entry>,
    pub(super) tests: Vec<(String, String)>,
    pub(super) unrelated: [String; 2],
    pub(super) shared: String,
    pub(super) test_path: String,
    pub(super) rule_sites: BTreeSet<String>,
}

impl TariffFixture {
    pub(super) fn new() -> Self {
        Self::with_paths(
            ["src/checkout.py", "src/invoicing.py"],
            "tests/test_shipping.py",
            "src/tariff.py",
        )
    }

    pub(super) fn renamed() -> Self {
        let mut scenario = Self::with_paths(
            ["src/component_7.py", "src/component_2.py"],
            "checks/regression_9.py",
            "src/component_5.py",
        );
        scenario.write(
            "src/assets/extra_shipping.bundle.js",
            &format!(
                "var generatedShipping={:?};\n",
                "shipping tariff data;".repeat(250)
            ),
        );
        assert_fixture_bounds(&scenario.fixture);
        scenario
    }

    fn with_paths(paths: [&str; 2], test_path: &str, shared: &str) -> Self {
        let fixture = Fixture::new("acceptance-tariff-cleanup");
        let entries = vec![
            Entry {
                path: paths[0].to_owned(),
                name: "checkout_shipping_quote".to_owned(),
                binding: None,
            },
            Entry {
                path: paths[1].to_owned(),
                name: "batch_shipping_invoice".to_owned(),
                binding: None,
            },
        ];
        let mut scenario = Self {
            fixture,
            sources: BTreeMap::new(),
            entries,
            tests: Vec::new(),
            unrelated: [
                "src/export_old.py".to_owned(),
                "src/archive_old.py".to_owned(),
            ],
            shared: shared.to_owned(),
            test_path: test_path.to_owned(),
            rule_sites: paths.into_iter().map(str::to_owned).collect(),
        };
        for index in 0..scenario.entries.len() {
            let entry = &scenario.entries[index];
            let source = format!("{TARIFF}\n\n{}", entry.wrapper());
            scenario.write(&scenario.entries[index].path.clone(), &source);
        }
        for path in scenario.unrelated.clone() {
            scenario.write(&path, LEGACY_FORMATTER);
        }
        scenario.write_tests();
        for path in [
            "src/assets/shipping.bundle.js",
            "src/assets/shipping.min.js",
        ] {
            let payload = "shipping table generated asset;".repeat(140);
            scenario.write(
                path,
                &format!("/* generated build output */\nvar shippingPayload={payload:?};\n"),
            );
        }
        for path in ["src/data/shipping_en.json", "src/data/shipping_de.json"] {
            scenario.write(
                path,
                &json!({"shipping": vec!["delivery"; 160]}).to_string(),
            );
        }
        assert_fixture_bounds(&scenario.fixture);
        scenario
    }

    fn write_tests(&mut self) {
        let path = self.test_path.clone();
        self.tests = self
            .entries
            .iter()
            .map(|entry| {
                let source = format!(
                    "def test_{}():\n    cases = {}\n    for region, value, weight, delivery_pass, express, expected in cases:\n        assert {}(region, value, weight, delivery_pass, express) == expected\n",
                    entry.name,
                    CASES.replace('\n', "\n    "),
                    entry.name,
                );
                (path.clone(), source)
            })
            .collect();
        let checks = self
            .tests
            .iter()
            .map(|(_, source)| source.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let bindings = self
            .entries
            .iter()
            .map(Entry::test_binding)
            .collect::<Vec<_>>()
            .join("\n");
        let source = format!("{bindings}\n\n\n{checks}");
        self.write(&path, &source);
    }

    pub(super) fn write(&mut self, path: &str, source: &str) {
        self.fixture.write(path, source);
        self.sources.insert(path.to_owned(), source.to_owned());
    }

    pub(super) fn centralize(&mut self) {
        self.write(&self.shared.clone(), TARIFF);
        self.rule_sites = BTreeSet::from([self.shared.clone()]);
        let binding = self.shared_binding();
        for index in 0..self.entries.len() {
            self.entries[index].binding = Some(binding.clone());
            let entry = &self.entries[index];
            let source = format!("{binding}\n\n\n{}", entry.wrapper());
            self.write(&self.entries[index].path.clone(), &source);
        }
        assert_fixture_bounds(&self.fixture);
    }

    pub(super) fn shared_binding(&self) -> String {
        let module = self.shared.strip_suffix(".py").unwrap().replace('/', ".");
        format!("from {module} import shipping_fee")
    }

    pub(super) fn add_copy(&mut self) {
        let entry = Entry {
            path: "src/reseller.py".to_owned(),
            name: "reseller_shipping_quote".to_owned(),
            binding: None,
        };
        self.write(&entry.path, &format!("{TARIFF}\n\n{}", entry.wrapper()));
        self.rule_sites.insert(entry.path.clone());
        self.entries.push(entry);
        self.write_tests();
        assert_fixture_bounds(&self.fixture);
    }

    pub(super) fn add_delegate(&mut self) {
        let owner = self.entries[0].module();
        let binding = format!("from {owner} import shipping_fee");
        let entry = Entry {
            path: "src/reseller.py".to_owned(),
            name: "reseller_shipping_quote".to_owned(),
            binding: Some(binding.clone()),
        };
        self.write(&entry.path, &format!("{binding}\n\n\n{}", entry.wrapper()));
        self.entries.push(entry);
        self.write_tests();
        assert_fixture_bounds(&self.fixture);
    }

    pub(super) fn retarget_tests(&mut self) {
        let wrappers = self
            .entries
            .iter()
            .map(Entry::wrapper)
            .collect::<Vec<_>>()
            .join("\n\n");
        self.write("archive/shadow.py", &format!("{TARIFF}\n\n{wrappers}"));
        let mut source = self.sources[&self.test_path].clone();
        for entry in &self.entries {
            source = source.replace(
                &entry.test_binding(),
                &format!("from archive.shadow import {}", entry.name),
            );
        }
        self.write(&self.test_path.clone(), &source);
        assert_fixture_bounds(&self.fixture);
    }

    pub(super) fn copy_into_billing(&mut self) -> String {
        let entry = &mut self.entries[1];
        entry.binding = None;
        let path = entry.path.clone();
        let source = format!("{TARIFF}\n\n{}", entry.wrapper());
        self.rule_sites.insert(path.clone());
        self.write(&path, &source);
        assert_fixture_bounds(&self.fixture);
        path
    }

    pub(super) fn resolve_legacy_debt(&mut self) {
        let module = self.unrelated[0]
            .strip_suffix(".py")
            .unwrap()
            .replace('/', ".");
        self.write(
            &self.unrelated[1].clone(),
            &format!("from {module} import format_legacy_csv\n"),
        );
        assert!(
            LEGACY_FORMATTER.lines().count() > TARIFF.lines().count(),
            "the removed independent rule is larger than the newly copied tariff"
        );
    }

    pub(super) fn verify_behavior(&self) {
        let functions = self
            .entries
            .iter()
            .map(|entry| {
                format!(
                    "getattr(importlib.import_module({:?}), {:?})",
                    entry.module(),
                    entry.name
                )
            })
            .collect::<Vec<_>>()
            .join(",\n");
        let script = format!(
            "import importlib, json, runpy\ncases = {CASES}\nchecks = runpy.run_path({:?})\nfor name, check in checks.items():\n    if name.startswith('test_'):\n        check()\nfunctions = [{functions}]\namounts = [[function(*case[:5]) for case in cases] for function in functions]\ninvalid = [('domestic', -1, 2000, False, False), ('domestic', 1.5, 2000, False, False), ('domestic', True, 2000, False, False), ('domestic', 0, 0, False, False), ('domestic', 0, 1.5, False, False), ('domestic', 0, True, False, False), ('unknown', 0, 2000, False, False), ('domestic', 0, 2000, 1, False), ('domestic', 0, 2000, False, 1)]\nrejected = []\nfor function in functions:\n    outcomes = []\n    for case in invalid:\n        try:\n            function(*case)\n        except ValueError:\n            outcomes.append(True)\n        else:\n            outcomes.append(False)\n    rejected.append(outcomes)\nprint(json.dumps({{'amounts': amounts, 'invalid_rejected': rejected}}))\n",
            self.test_path
        );
        let expected: Vec<Value> = self
            .entries
            .iter()
            .map(|_| json!([499, 0, 699, 0, 0, 300, 300, 999, 999, 999, 1199, 1499]))
            .collect();
        assert_eq!(
            probe(&self.fixture, &script),
            json!({"amounts": expected, "invalid_rejected": vec![vec![true; 9]; self.entries.len()]})
        );
    }
}
