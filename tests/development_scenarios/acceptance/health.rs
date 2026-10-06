//! Frozen user cases D/E: tariff-maintenance evidence, independent of clone scores and totals.

mod debt;
mod fixture;

use super::support::Journey;
use fixture::{TARIFF, TariffFixture};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

struct TariffEvidence {
    duplication: Value,
    read: Value,
}

fn investigate_tariff(journey: &mut Journey<'_>) -> TariffEvidence {
    let duplication = journey
        .step(
            "locate independently maintained source logic in the owned tree",
            &[
                "dup",
                ".",
                "--no-project-config",
                "--no-cache",
                "-f",
                "json",
                "--quiet",
            ],
            0,
        )
        .stdout_json();
    let candidates = journey
        .step(
            "find production roles and regression checks using the user's shipping domain",
            &[
                "find",
                "shipping tariff",
                ".",
                "--match",
                "any",
                "--limit",
                "32",
                "--budget",
                "32768",
                "--max-output-bytes",
                "262144",
                "--no-project-config",
                "-f",
                "json",
                "--quiet",
            ],
            0,
        )
        .stdout_json();
    let mut arguments: Vec<String> = [
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
    let mut hashes = BTreeMap::new();
    for hit in candidates["hits"].as_array().unwrap() {
        let target = &hit["read"];
        let path = target["path"].as_str().unwrap();
        hashes.insert(
            path.to_owned(),
            target["expected_hash"].as_str().unwrap().to_owned(),
        );
    }
    assert!(
        !hashes.is_empty(),
        "shipping task returned no source candidates"
    );
    for (path, hash) in hashes {
        arguments.extend([
            "--file".to_owned(),
            path.clone(),
            "--expect-hash".to_owned(),
            path,
            hash,
        ]);
    }
    let read = journey
        .step(
            "read the discovered tariff responsibilities and actual regression assertions",
            &arguments.iter().map(String::as_str).collect::<Vec<_>>(),
            0,
        )
        .stdout_json();
    TariffEvidence { duplication, read }
}

fn source_packet(report: &Value, fixture: &TariffFixture) -> BTreeMap<String, Vec<String>> {
    let files: BTreeMap<_, _> = report["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| (file["id"].as_u64().unwrap(), file["path"].as_str().unwrap()))
        .collect();
    let mut packet = BTreeMap::<String, Vec<String>>::new();
    for source in report["sources"].as_array().into_iter().flatten() {
        let path = files[&source["file"].as_u64().unwrap()];
        let content = source["content"].as_str().unwrap();
        let start = usize::try_from(source["span"]["start_byte"].as_u64().unwrap()).unwrap();
        let end = usize::try_from(source["span"]["end_byte"].as_u64().unwrap()).unwrap();
        let authored = fixture
            .sources
            .get(path)
            .expect("source belongs to authored fixture");
        assert_eq!(
            authored.get(start..end),
            Some(content),
            "misattributed source: {path}"
        );
        packet
            .entry(path.to_owned())
            .or_default()
            .push(content.to_owned());
    }
    packet
}

fn has_source(packet: &BTreeMap<String, Vec<String>>, path: &str, expected: &str) -> bool {
    packet.get(path).is_some_and(|chunks| {
        chunks
            .iter()
            .any(|content| content.contains(expected.trim_end()))
    })
}

fn duplicate_families(report: &Value) -> impl Iterator<Item = &Value> {
    ["exact", "near"]
        .into_iter()
        .flat_map(|kind| report["duplicates"][kind].as_array().unwrap())
}

fn assert_unrelated_debt_remains(report: &Value, fixture: &TariffFixture) {
    assert!(
        duplicate_families(report).any(|family| {
            let locations: BTreeSet<_> = family["instances"]
                .as_array()
                .unwrap()
                .iter()
                .map(|instance| instance["path"].as_str().unwrap())
                .collect();
            fixture
                .unrelated
                .iter()
                .all(|path| locations.contains(path.as_str()))
        }),
        "centralizing tariffs must leave the independently authored legacy duplication visible"
    );
}

fn cleanup_gaps(evidence: &TariffEvidence, fixture: &TariffFixture) -> Vec<String> {
    let packet = source_packet(&evidence.read, fixture);
    let mut missing = Vec::new();
    for path in &fixture.rule_sites {
        if !has_source(&packet, path, TARIFF) {
            missing.push(format!("maintained tariff source: {path}"));
        }
    }
    for entry in &fixture.entries {
        if !has_source(&packet, &entry.path, &entry.wrapper()) {
            missing.push(format!("production role: {}", entry.path));
        }
        if let Some(binding) = &entry.binding
            && !has_source(&packet, &entry.path, binding)
        {
            missing.push(format!("production binding: {}: {binding}", entry.path));
        }
    }
    for (path, test) in &fixture.tests {
        if !has_source(&packet, path, test) {
            missing.push(format!("tariff assertion: {path}"));
        }
    }
    for entry in &fixture.entries {
        if !has_source(&packet, &fixture.test_path, &entry.test_binding()) {
            missing.push(format!("genuine test binding to {}", entry.path));
        }
    }
    assert_current_maintenance_locations(&evidence.duplication, fixture);
    assert_unrelated_debt_remains(&evidence.duplication, fixture);
    missing
}

fn assert_current_maintenance_locations(report: &Value, fixture: &TariffFixture) {
    for family in duplicate_families(report) {
        for instance in family["instances"].as_array().unwrap() {
            let path = instance["path"].as_str().unwrap();
            let source = fixture
                .sources
                .get(path)
                .expect("clone location belongs to fixture");
            let start = instance["start_line"].as_u64().unwrap();
            let end = instance["end_line"].as_u64().unwrap();
            assert!(
                start > 0 && end >= start && end <= u64::try_from(source.lines().count()).unwrap(),
                "stale or invalid maintenance location: {path}:{start}-{end}"
            );
        }
    }
    for finding in report["finding_catalog"]["findings"].as_array().unwrap() {
        if finding["kind"] != "duplication" {
            continue;
        }
        let locations = std::iter::once(&finding["primary_location"]).chain(
            finding["related_locations"]
                .as_array()
                .into_iter()
                .flatten(),
        );
        for location in locations {
            let path = location["path"].as_str().unwrap();
            assert!(
                !path.starts_with("src/assets/") && !path.starts_with("src/data/"),
                "generated/data repetition was offered as an actionable maintenance finding: {path}"
            );
        }
    }
}

#[test]
#[ignore = "user acceptance case D; run scripts/test-scenarios.sh acceptance::health"]
fn tariff_cleanup_supplies_all_maintenance_sites_and_real_regression_source() {
    let mut fixture = TariffFixture::new();
    fixture.verify_behavior();
    let before = {
        let mut journey = Journey::bounded(&fixture.fixture, 8);
        investigate_tariff(&mut journey)
    };
    let before_gaps = cleanup_gaps(&before, &fixture);
    fixture.centralize();
    fixture.verify_behavior();
    let after = {
        let mut journey = Journey::bounded(&fixture.fixture, 8);
        investigate_tariff(&mut journey)
    };
    let after_gaps = cleanup_gaps(&after, &fixture);
    assert!(
        before_gaps.is_empty() && after_gaps.is_empty(),
        "tariff cleanup is incomplete; before: {before_gaps:?}; after centralization: {after_gaps:?}"
    );
}

fn assert_variant_complete(fixture: &TariffFixture) {
    fixture.verify_behavior();
    let mut journey = Journey::bounded(&fixture.fixture, 8);
    let evidence = investigate_tariff(&mut journey);
    let missing = cleanup_gaps(&evidence, fixture);
    assert!(
        missing.is_empty(),
        "tariff maintenance evidence is incomplete: {missing:?}"
    );
}

#[test]
#[ignore = "user acceptance case D third independent implementation"]
fn a_third_tariff_implementation_expands_the_maintenance_obligation() {
    let mut fixture = TariffFixture::new();
    fixture.add_copy();
    assert_eq!(fixture.rule_sites.len(), 3);
    assert_variant_complete(&fixture);
}

#[test]
#[ignore = "user acceptance case D delegated caller counterexample"]
fn a_third_delegated_caller_preserves_two_independent_tariff_rules() {
    let mut fixture = TariffFixture::new();
    fixture.add_delegate();
    assert_eq!(fixture.rule_sites.len(), 2);
    assert_variant_complete(&fixture);
}

#[test]
#[ignore = "user acceptance case D renamed paths and irrelevant ballast"]
fn renamed_tariff_paths_and_generated_ballast_preserve_the_required_evidence() {
    assert_variant_complete(&TariffFixture::renamed());
}

#[test]
#[ignore = "user acceptance case D counterfeit-test negative control"]
fn counterfeit_test_bodies_cannot_establish_genuine_tariff_regression_evidence() {
    let mut fixture = TariffFixture::new();
    fixture.retarget_tests();
    fixture.verify_behavior();
    let mut journey = Journey::bounded(&fixture.fixture, 8);
    let evidence = investigate_tariff(&mut journey);
    let packet = source_packet(&evidence.read, &fixture);
    for (path, body) in &fixture.tests {
        assert!(
            has_source(&packet, path, body),
            "negative control must actually retrieve the counterfeit assertion body"
        );
    }
    for entry in &fixture.entries {
        let counterfeit_binding = format!("from archive.shadow import {}", entry.name);
        assert!(
            has_source(&packet, &fixture.test_path, &counterfeit_binding),
            "counterfeit identity is unavailable; missing actual retargeted test binding: {counterfeit_binding}"
        );
        assert!(
            !has_source(&packet, &fixture.test_path, &entry.test_binding()),
            "an unrelated implementation was presented as the genuine tariff test binding"
        );
    }
    assert!(
        !cleanup_gaps(&evidence, &fixture).is_empty(),
        "counterfeit assertions cannot complete the positive maintenance task"
    );
}
