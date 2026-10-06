use super::fixture::TariffFixture;
use super::{TARIFF, assert_unrelated_debt_remains, has_source, source_packet};
use crate::acceptance::support::Journey;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

fn arguments() -> Vec<String> {
    [
        "dup",
        ".",
        "--no-project-config",
        "--no-cache",
        "-f",
        "json",
        "--quiet",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn save_baseline(journey: &mut Journey<'_>, path: &Path) {
    let mut args = arguments();
    args.extend([
        "--baseline-ready".to_owned(),
        "--output".to_owned(),
        path.to_str().unwrap().to_owned(),
    ]);
    journey.step(
        "record the user's permitted existing maintenance debt",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        0,
    );
}

fn gate(journey: &mut Journey<'_>, path: &Path, exit: i32) -> Value {
    let mut args = arguments();
    args.extend([
        "--baseline".to_owned(),
        path.to_str().unwrap().to_owned(),
        "--fail-on-regression".to_owned(),
    ]);
    journey
        .step(
            "apply the user's no-new-tariff-copy regression gate",
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            exit,
        )
        .stdout_json()
}

fn finding_paths(finding: &Value) -> BTreeSet<String> {
    std::iter::once(&finding["primary_location"])
        .chain(
            finding["related_locations"]
                .as_array()
                .into_iter()
                .flatten(),
        )
        .filter_map(|location| location["path"].as_str().map(str::to_owned))
        .collect()
}

fn read_new_debt(journey: &mut Journey<'_>, comparison: &Value) -> Value {
    let mut targets = BTreeSet::new();
    for change in comparison["baseline"]["finding_changes"]["changes"]
        .as_array()
        .unwrap()
    {
        if !matches!(change["state"].as_str(), Some("new" | "worsened"))
            || change["after"]["kind"] != "duplication"
        {
            continue;
        }
        let finding = &change["after"];
        for location in std::iter::once(&finding["primary_location"]).chain(
            finding["related_locations"]
                .as_array()
                .into_iter()
                .flatten(),
        ) {
            targets.insert((
                location["path"].as_str().unwrap().to_owned(),
                location["start_line"].as_u64().unwrap().to_string(),
            ));
        }
    }
    assert!(
        !targets.is_empty(),
        "no source locations for the new independent tariff obligation"
    );
    let mut args: Vec<String> = [
        "read",
        ".",
        "--budget",
        "32768",
        "--max-output-bytes",
        "262144",
        "--no-project-config",
        "-f",
        "json",
        "--quiet",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (path, line) in targets {
        args.extend(["--line".to_owned(), path, line]);
    }
    journey
        .step(
            "inspect the new maintenance sites named by the comparison",
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            0,
        )
        .stdout_json()
}

fn assert_new_tariff_debt(report: &Value, fixture: &TariffFixture, added: &str) {
    assert_eq!(
        report["baseline"]["finding_changes"]["comparison"], "complete",
        "absolute duplication totals cannot answer the no-new-debt policy"
    );
    assert_eq!(report["baseline"]["regressed"], true);
    let changes = report["baseline"]["finding_changes"]["changes"]
        .as_array()
        .unwrap();
    let new_debt: Vec<_> = changes
        .iter()
        .filter(|change| {
            matches!(change["state"].as_str(), Some("new" | "worsened"))
                && change["after"]["kind"] == "duplication"
        })
        .map(|change| finding_paths(&change["after"]))
        .collect();
    assert!(
        new_debt
            .iter()
            .any(|paths| paths.contains(added) && paths.contains(&fixture.shared)),
        "the regression must identify the new billing copy and the shared tariff it copied"
    );
    for paths in new_debt {
        assert!(
            fixture.unrelated.iter().all(|path| !paths.contains(path)),
            "legacy formatter debt was mislabeled as the new tariff obligation"
        );
    }
}

fn check_new_copy(replace_legacy: bool) {
    let mut fixture = TariffFixture::new();
    fixture.centralize();
    fixture.verify_behavior();
    let baseline = fixture.fixture.state_path().join("permitted-debt.json");
    {
        let mut journey = Journey::bounded(&fixture.fixture, 8);
        save_baseline(&mut journey, &baseline);
        let unchanged = gate(&mut journey, &baseline, 0);
        assert_eq!(unchanged["baseline"]["regressed"], false);
        assert_unrelated_debt_remains(&unchanged, &fixture);
    }
    let added = fixture.copy_into_billing();
    if replace_legacy {
        fixture.resolve_legacy_debt();
    }
    fixture.verify_behavior();
    let mut journey = Journey::bounded(&fixture.fixture, 8);
    let report = gate(&mut journey, &baseline, 2);
    let read = read_new_debt(&mut journey, &report);
    assert_new_tariff_debt(&report, &fixture, &added);
    let packet = source_packet(&read, &fixture);
    for path in [&added, &fixture.shared] {
        assert!(
            has_source(&packet, path, TARIFF),
            "missing real tariff source supporting the new-copy finding: {path}"
        );
    }
    if replace_legacy {
        assert!(
            report["baseline"]["finding_changes"]["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|change| {
                    matches!(change["state"].as_str(), Some("resolved" | "improved"))
                        && fixture
                            .unrelated
                            .iter()
                            .all(|path| finding_paths(&change["before"]).contains(path))
                }),
            "the unrelated old clone removal must remain distinct from the new tariff debt"
        );
    } else {
        assert_unrelated_debt_remains(&report, &fixture);
    }
}

#[test]
#[ignore = "user acceptance case E new tariff debt while legacy debt remains"]
fn a_new_tariff_copy_fails_the_gate_while_existing_debt_remains_allowed() {
    check_new_copy(false);
}

#[test]
#[ignore = "user acceptance case E new tariff debt offsets unrelated debt removal"]
fn removing_old_formatter_duplication_cannot_hide_a_new_tariff_copy() {
    check_new_copy(true);
}

#[test]
#[ignore = "user acceptance case E repair restores the original baseline gate"]
fn removing_the_new_tariff_copy_restores_the_gate_without_removing_legacy_debt() {
    let mut fixture = TariffFixture::new();
    fixture.centralize();
    fixture.verify_behavior();
    let baseline = fixture.fixture.state_path().join("permitted-debt.json");
    {
        let mut journey = Journey::bounded(&fixture.fixture, 8);
        save_baseline(&mut journey, &baseline);
    }
    fixture.copy_into_billing();
    fixture.verify_behavior();
    fixture.centralize();
    fixture.verify_behavior();
    let mut journey = Journey::bounded(&fixture.fixture, 8);
    let report = gate(&mut journey, &baseline, 0);
    assert_eq!(report["baseline"]["regressed"], false);
    assert_eq!(
        report["baseline"]["finding_changes"]["comparison"],
        "complete"
    );
    assert_unrelated_debt_remains(&report, &fixture);
}
