use super::{
    Journey, OutputBudget, ReadEvidence, ReviewDiscovery, ReviewTask, array,
    assert_complete_definition, assert_complete_review, capture_review, hash, number,
    read_discovery, text, tree_for,
};
use crate::support::Fixture;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;

const CHECKOUT: &str =
    "import { price as quote } from '@pricing';\nexport function checkout() { return quote(2); }\n";
const DIRECT: &str = "import { price } from './pricing/standard';\nexport function legacyCheckout() { return price(3); }\n";
const RETIRED: &str = "export function legacyBanner() { return 'legacy customer pricing'; }\n";
const ADDED: &str =
    "export function migrationLabel(account: string) { return `Pricing migrated: ${account}`; }\n";
const CURRENCY: &str = "export function currencyLabel() { return 'EUR'; }\n";

fn alias_config(target: &str) -> String {
    format!(r#"{{"compilerOptions":{{"baseUrl":".","paths":{{"@pricing":["{target}"]}}}}}}"#)
}

fn pricing_source(multiplier: u32) -> String {
    format!("export function price(quantity: number) {{ return quantity * {multiplier}; }}\n")
}

fn migration_fixture() -> (Fixture, String, String) {
    let fixture = Fixture::new("journey-review-pricing-migration");
    fixture.write("tsconfig.json", &alias_config("src/pricing/standard.ts"));
    fixture.write("src/pricing/standard.ts", &pricing_source(100));
    fixture.write("src/pricing/discount.ts", &pricing_source(90));
    fixture.write("src/checkout.ts", CHECKOUT);
    fixture.write("src/direct.ts", DIRECT);
    fixture.write("src/legacy_banner.ts", RETIRED);
    fixture.write("src/currency.ts", CURRENCY);
    fixture.write("src/invoice.ts", "import { currencyLabel } from './currency';\nexport function invoiceCurrency() { return currencyLabel(); }\n");
    fixture.write("src/shadow.ts", "import { price } from '@pricing';\nexport function injected(price: (quantity: number) => number) { return price(4); }\n");
    fixture.write("src/training.ts", "function price(quantity: number) { return quantity; }\nexport function example() { return price(5); }\n");
    let base = fixture.commit("standard pricing alias and migration-era helpers");

    fixture.write("tsconfig.json", &alias_config("src/pricing/discount.ts"));
    fixture.write("src/pricing/standard.ts", &pricing_source(105));
    fixture.write("src/pricing/discount.ts", &pricing_source(120));
    fixture.remove("src/legacy_banner.ts");
    fixture.write("src/migration.ts", ADDED);
    fs::rename(
        fixture.path().join("src/currency.ts"),
        fixture.path().join("src/money.ts"),
    )
    .unwrap();
    fixture.write("src/invoice.ts", "import { currencyLabel } from './money';\nexport function invoiceCurrency() { return currencyLabel(); }\n");
    let head = fixture.commit("activate new pricing and retire migration helpers");
    (fixture, base, head)
}

fn side_path<'a>(change: &'a Value, side: &str) -> Option<&'a str> {
    let file = change.get(side).expect("each change names both sides");
    if file.is_null() {
        None
    } else {
        Some(text(file, "path"))
    }
}

fn assert_lifecycle_inventory(report: &Value) {
    let changes = array(report, "changes");
    let identities: BTreeSet<_> = changes
        .iter()
        .map(|change| {
            (
                text(change, "status"),
                side_path(change, "base"),
                side_path(change, "head"),
            )
        })
        .collect();
    assert_eq!(
        identities.len(),
        changes.len(),
        "change identities are unique"
    );
    assert_eq!(
        identities,
        BTreeSet::from([
            ("modified", Some("tsconfig.json"), Some("tsconfig.json")),
            (
                "modified",
                Some("src/pricing/standard.ts"),
                Some("src/pricing/standard.ts")
            ),
            (
                "modified",
                Some("src/pricing/discount.ts"),
                Some("src/pricing/discount.ts")
            ),
            ("modified", Some("src/invoice.ts"), Some("src/invoice.ts")),
            ("renamed", Some("src/currency.ts"), Some("src/money.ts")),
            ("deleted", Some("src/legacy_banner.ts"), None),
            ("added", None, Some("src/migration.ts")),
        ])
    );
    assert_eq!(number(&report["totals"], "changes"), 7);
    assert_eq!(number(&report["totals"], "definitions"), 6);
    for change in changes {
        if text(change, "status") == "renamed" {
            assert_eq!(number(change, "hunks"), 0);
            assert_eq!(
                text(&change["base"], "sha256"),
                text(&change["head"], "sha256")
            );
            assert_eq!(text(&change["base"], "sha256"), hash(CURRENCY));
            for side in ["base", "head"] {
                assert!(array(&change[side], "definitions").is_empty());
                assert!(array(&change[side], "ranges").is_empty());
            }
        } else if side_path(change, "base") == Some("src/invoice.ts") {
            // Only the import path changes. Do not manufacture a changed function.
            assert!(number(change, "hunks") > 0);
            for side in ["base", "head"] {
                assert_eq!(text(&change[side], "mapping_status"), "available");
                assert!(array(&change[side], "definitions").is_empty());
            }
        }
    }
}

fn assert_revision_bindings(report: &Value) {
    for (side, pricing_path) in [
        ("base", "src/pricing/standard.ts"),
        ("head", "src/pricing/discount.ts"),
    ] {
        let references: Vec<_> = array(report, "relations")
            .iter()
            .filter(|relation| {
                text(relation, "side") == side && text(relation, "kind") == "symbol-reference"
            })
            .collect();
        let identities: BTreeSet<_> = references
            .iter()
            .map(|relation| {
                assert_eq!(text(relation, "change_basis"), "changed-definition");
                let reference = &relation["symbol"];
                assert_eq!(text(reference, "kind"), "call");
                assert_eq!(number(&reference["site"], "start_line"), 2);
                let source = &reference["source"];
                let target = &reference["target"];
                (
                    text(source, "path"),
                    text(source, "name"),
                    text(target, "path"),
                    text(target, "name"),
                )
            })
            .collect();
        assert_eq!(identities.len(), references.len());
        assert_eq!(
            identities,
            BTreeSet::from([
                ("src/checkout.ts", "checkout", pricing_path, "price"),
                (
                    "src/direct.ts",
                    "legacyCheckout",
                    "src/pricing/standard.ts",
                    "price"
                ),
            ])
        );
        for importer in ["src/checkout.ts", "src/shadow.ts"] {
            let edges: Vec<_> = array(report, "relations")
                .iter()
                .filter(|relation| {
                    text(relation, "side") == side
                        && text(relation, "kind") == "import"
                        && text(&relation["edge"], "source") == importer
                })
                .collect();
            assert_eq!(
                edges.len(),
                1,
                "one revision-local alias edge for {side}:{importer}"
            );
            assert_eq!(text(&edges[0]["edge"], "target"), pricing_path);
            assert_eq!(text(&edges[0]["edge"], "resolver"), "tsconfig-paths");
        }
    }
    for coverage in array(report, "coverage") {
        assert!(
            number(&coverage["call_resolution"], "unresolved") > 0,
            "the injected parameter call remains explicitly unresolved"
        );
    }
}

fn assert_migration_discovery(discovery: &ReviewDiscovery) {
    assert_complete_review(&discovery.report);
    assert_lifecycle_inventory(&discovery.report);
    assert_revision_bindings(&discovery.report);
    assert_eq!(
        discovery.targets.len(),
        10,
        "four pricing sides, four caller sides, one retired and one added declaration"
    );
}

fn assert_migration_source(evidence: &ReadEvidence, base_tree: &str, head_tree: &str) {
    assert_eq!(evidence.omitted, 0);
    let mut observed = BTreeSet::new();
    for outcome in &evidence.outcomes {
        assert!(observed.insert(outcome.target.key()));
        assert!([base_tree, head_tree].contains(&outcome.target.tree.as_str()));
        let is_base = outcome.target.tree == base_tree;
        let source = match (outcome.target.path.as_str(), outcome.target.symbol.as_str()) {
            ("src/pricing/standard.ts", "price") => pricing_source(if is_base { 100 } else { 105 }),
            ("src/pricing/discount.ts", "price") => pricing_source(if is_base { 90 } else { 120 }),
            ("src/checkout.ts", "checkout") => CHECKOUT.to_owned(),
            ("src/direct.ts", "legacyCheckout") => DIRECT.to_owned(),
            ("src/legacy_banner.ts", "legacyBanner") => {
                assert!(is_base, "deleted declaration has no head counterpart");
                RETIRED.to_owned()
            }
            ("src/migration.ts", "migrationLabel") => {
                assert!(!is_base, "added declaration has no base counterpart");
                ADDED.to_owned()
            }
            _ => panic!("unexpected migration read target: {:?}", outcome.target),
        };
        let definition = source.lines().last().unwrap();
        assert_complete_definition(outcome, &source, definition);
    }
    let mut expected: BTreeSet<_> = [base_tree, head_tree]
        .into_iter()
        .flat_map(|tree| {
            [
                ("src/pricing/standard.ts", "price"),
                ("src/pricing/discount.ts", "price"),
                ("src/checkout.ts", "checkout"),
                ("src/direct.ts", "legacyCheckout"),
            ]
            .map(|(path, symbol)| (tree.to_owned(), path.to_owned(), symbol.to_owned()))
        })
        .collect();
    expected.insert((
        base_tree.to_owned(),
        "src/legacy_banner.ts".to_owned(),
        "legacyBanner".to_owned(),
    ));
    expected.insert((
        head_tree.to_owned(),
        "src/migration.ts".to_owned(),
        "migrationLabel".to_owned(),
    ));
    assert_eq!(
        observed, expected,
        "all discovered revision-local definitions are delivered"
    );
}

#[test]
#[ignore = "development scenario; run scripts/test-scenarios.sh"]
fn pr_module_migration_reads_reported_alias_targets_and_respects_file_lifecycle() {
    let (fixture, base, head) = migration_fixture();
    let mut journey = Journey::new(&fixture);
    let task = ReviewTask {
        base: &base,
        head: &head,
        budget: OutputBudget {
            tokens: 32_768,
            bytes: 262_144,
        },
    };
    let discovery = capture_review(&mut journey, &task);
    assert_migration_discovery(&discovery);

    // Current resolver contents are deliberately unusable. The captured comparison
    // and its source handoff still refer to their original, reported tree identities.
    fixture.write(
        "tsconfig.json",
        "{ deliberately invalid live resolver configuration\n",
    );
    fixture.write(
        "src/pricing/discount.ts",
        "export function price() { return 'LIVE_POISON'; }\n",
    );
    let evidence = read_discovery(&mut journey, &discovery);
    assert_migration_source(
        &evidence,
        tree_for(&discovery.report, "base"),
        tree_for(&discovery.report, "head"),
    );
    let repeated = capture_review(&mut journey, &task);
    assert_eq!(repeated.report, discovery.report);
}
