use crate::acceptance::support::{assert_fixture_bounds, probe};
use crate::support::Fixture;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;

pub(super) const ENTRY_PATH: &str = "service/storage_api.py";
pub(super) const POLICY_PATH: &str = "service/quota_policy.py";
pub(super) const HELPER_PATH: &str = "service/quota_percent.py";
pub(super) const ALTERNATE_PATH: &str = "service/whole_percent.py";

pub(super) const ENTRY: &str = r#"from service.quota_policy import quota_status

def storage_status(used_bytes, capacity_bytes):
    """Return the storage status for an account's byte usage."""
    return {"status": quota_status(used_bytes, capacity_bytes)}
"#;

pub(super) const POLICY_BODY: &str = r#"def quota_status(used_bytes, capacity_bytes):
    percentage = rounded_usage(used_bytes, capacity_bytes)
    if percentage >= 100:
        return "blocked"
    if percentage >= 90:
        return "warning"
    return "clear"
"#;

pub(super) const OLD_BINDING: &str = "from service.quota_percent import rounded_usage\n";
pub(super) const NEW_BINDING: &str = "from service.whole_percent import rounded_usage\n";

pub(super) const FLOOR: &str = r#"def rounded_usage(used_bytes, capacity_bytes):
    if used_bytes < 0 or capacity_bytes <= 0:
        raise ValueError("usage must be nonnegative and capacity positive")
    return used_bytes * 100 // capacity_bytes
"#;

pub(super) const CEIL: &str = r#"def rounded_usage(used_bytes, capacity_bytes):
    if used_bytes < 0 or capacity_bytes <= 0:
        raise ValueError("usage must be nonnegative and capacity positive")
    return (used_bytes * 100 + capacity_bytes - 1) // capacity_bytes
"#;

const LABEL: &str = r#"def usage_label(account, used_bytes):
    return f"{account}: {used_bytes} bytes"
"#;
const UPDATED_LABEL: &str = r#"def usage_label(account, used_bytes):
    return f"{account}: {used_bytes // 1024} KiB"
"#;

#[derive(Clone, Copy, Debug)]
pub(super) enum Variant {
    Helper,
    UnrelatedFile,
    UnrelatedDeclaration,
    Binding,
}

impl Variant {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Helper => "helper-and-stale-handoff",
            Self::UnrelatedFile => "unrelated-file",
            Self::UnrelatedDeclaration => "unrelated-declaration",
            Self::Binding => "new-binding",
        }
    }

    pub(super) fn expects_warning(self) -> bool {
        matches!(self, Self::Helper | Self::Binding)
    }
}

/// Independently authored source spans, unavailable to the public CLI driver.
pub(super) struct RequiredFragment {
    pub(super) path: &'static str,
    pub(super) source: String,
}

pub(super) struct QuotaWorld {
    pub(super) fixture: Fixture,
    pub(super) base: String,
    pub(super) base_tree: String,
    pub(super) before: BTreeMap<String, String>,
    pub(super) after: BTreeMap<String, String>,
    pub(super) initial_packet: Vec<RequiredFragment>,
    pub(super) delta_packet: Vec<RequiredFragment>,
    pub(super) variant: Variant,
}

impl QuotaWorld {
    pub(super) fn new(variant: Variant, encoding: &str) -> Self {
        let fixture = Fixture::new(&format!("efficiency-quota-{}-{encoding}", variant.label()));
        let policy = format!("{OLD_BINDING}\n{POLICY_BODY}");
        let before = BTreeMap::from([
            (ENTRY_PATH.to_owned(), ENTRY.to_owned()),
            (POLICY_PATH.to_owned(), policy.clone()),
            (HELPER_PATH.to_owned(), format!("{FLOOR}\n{LABEL}")),
            (ALTERNATE_PATH.to_owned(), CEIL.to_owned()),
            (
                "service/account_labels.py".to_owned(),
                "def account_label(account):\n    return account.strip()\n".to_owned(),
            ),
        ]);
        for (path, source) in &before {
            fixture.write(path, source);
        }
        let base = fixture.commit("Storage warnings use completed percentage points");
        let base_tree = git2::Repository::open(fixture.path())
            .unwrap()
            .head()
            .unwrap()
            .peel_to_tree()
            .unwrap()
            .id()
            .to_string();
        let mut after = before.clone();
        match variant {
            Variant::Helper => {
                after.insert(HELPER_PATH.to_owned(), format!("{CEIL}\n{LABEL}"));
            }
            Variant::UnrelatedFile => {
                after.insert(
                    "service/account_labels.py".to_owned(),
                    "def account_label(account):\n    return account.strip().title()\n".to_owned(),
                );
            }
            Variant::UnrelatedDeclaration => {
                after.insert(HELPER_PATH.to_owned(), format!("{FLOOR}\n{UPDATED_LABEL}"));
            }
            Variant::Binding => {
                after.insert(
                    POLICY_PATH.to_owned(),
                    format!("{NEW_BINDING}\n{POLICY_BODY}"),
                );
            }
        }
        let initial_packet = vec![
            fragment(ENTRY_PATH, ENTRY),
            fragment(POLICY_PATH, &policy),
            fragment(HELPER_PATH, FLOOR),
        ];
        let delta_packet = match variant {
            Variant::Helper => vec![fragment(HELPER_PATH, CEIL)],
            Variant::UnrelatedFile | Variant::UnrelatedDeclaration => Vec::new(),
            Variant::Binding => vec![
                fragment(POLICY_PATH, NEW_BINDING),
                fragment(ALTERNATE_PATH, CEIL),
            ],
        };
        let world = Self {
            fixture,
            base,
            base_tree,
            before,
            after,
            initial_packet,
            delta_packet,
            variant,
        };
        world.freeze(encoding);
        assert_fixture_bounds(&world.fixture);
        world
    }

    fn freeze(&self, encoding: &str) {
        let initial = packet_json(&self.initial_packet, &self.before);
        let delta = packet_json(&self.delta_packet, &self.after);
        assert_packet_bounds(&self.initial_packet, 48, 2 * 1024, 3);
        assert_packet_bounds(
            &self.delta_packet,
            if matches!(self.variant, Variant::Binding) {
                24
            } else {
                12
            },
            if matches!(self.variant, Variant::Binding) {
                2 * 1024
            } else {
                1024
            },
            if matches!(self.variant, Variant::Binding) {
                2
            } else {
                1
            },
        );
        assert!(self.before.len() <= 20 && self.after.len() <= 20);
        for source in [&self.before, &self.after] {
            assert!(source.values().map(String::len).sum::<usize>() <= 48 * 1024);
        }
        let frozen = json!({
            "case": "K",
            "variant": self.variant.label(),
            "encoding": encoding,
            "head_commit": self.base,
            "head_tree": self.base_tree,
            "examples": [[0,1000], [890,1000], [895,1000], [900,1000], [1000,1000]],
            "before": ["clear", "clear", "clear", "warning", "blocked"],
            "after": if self.variant.expects_warning() {
                ["clear", "clear", "warning", "warning", "blocked"]
            } else {
                ["clear", "clear", "clear", "warning", "blocked"]
            },
            "initial_packet": initial,
            "delta_packet": delta,
        });
        fs::write(
            self.fixture.state_path().join("frozen-quota-truth.json"),
            serde_json::to_vec_pretty(&frozen).unwrap(),
        )
        .unwrap();
    }

    pub(super) fn mutate(&self) {
        for (path, source) in &self.after {
            if self.before.get(path) != Some(source) {
                self.fixture.write(path, source);
            }
        }
        assert_fixture_bounds(&self.fixture);
    }

    pub(super) fn assert_probe(&self, after: bool) {
        let program = r"import json
from service.storage_api import storage_status
examples = [(0,1000), (890,1000), (895,1000), (900,1000), (1000,1000)]
print(json.dumps([storage_status(used, capacity) for used, capacity in examples]))
";
        let name = if after { "after" } else { "before" };
        let actual = probe(&self.fixture, program);
        fs::write(
            self.fixture
                .state_path()
                .join(format!("quota-probe-{name}.json")),
            serde_json::to_vec_pretty(&actual).unwrap(),
        )
        .unwrap();
        let statuses = if after && self.variant.expects_warning() {
            ["clear", "clear", "warning", "warning", "blocked"]
        } else {
            ["clear", "clear", "clear", "warning", "blocked"]
        };
        let expected: Vec<Value> = statuses
            .into_iter()
            .map(|status| json!({"status": status}))
            .collect();
        assert_eq!(
            actual,
            json!(expected),
            "literal independent quota examples"
        );
    }
}

fn fragment(path: &'static str, source: &str) -> RequiredFragment {
    RequiredFragment {
        path,
        source: source.trim_end_matches('\n').to_owned(),
    }
}

fn packet_json(packet: &[RequiredFragment], authored: &BTreeMap<String, String>) -> Value {
    json!(packet.iter().map(|fragment| {
        let file = &authored[fragment.path];
        let start = file.find(&fragment.source).unwrap();
        let end = start + fragment.source.len();
        json!({
            "path": fragment.path,
            "source": fragment.source,
            "span": {
                "start_byte": start, "end_byte": end,
                "start_line": 1 + file[..start].bytes().filter(|byte| *byte == b'\n').count(),
                "end_line": 1 + file[..end - 1].bytes().filter(|byte| *byte == b'\n').count(),
            },
        })
    }).collect::<Vec<_>>())
}

fn assert_packet_bounds(packet: &[RequiredFragment], lines: usize, bytes: usize, paths: usize) {
    let actual_lines = packet
        .iter()
        .flat_map(|fragment| fragment.source.lines())
        .filter(|line| !line.trim().is_empty())
        .count();
    let escaped_bytes: usize = packet
        .iter()
        .map(|fragment| serde_json::to_string(&fragment.source).unwrap().len())
        .sum();
    let actual_paths: std::collections::BTreeSet<_> =
        packet.iter().map(|fragment| fragment.path).collect();
    assert!(
        actual_lines <= lines,
        "frozen necessary source has {actual_lines} lines"
    );
    assert!(
        escaped_bytes <= bytes,
        "frozen necessary source has {escaped_bytes} escaped bytes"
    );
    assert!(actual_paths.len() <= paths);
}
