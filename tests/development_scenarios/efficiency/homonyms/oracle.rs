use super::driver::Evidence;
use super::fixture::World;
use git2::{Oid, Repository};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;

#[derive(Clone)]
struct Fragment {
    tree: String,
    path: String,
    hash: String,
    content: String,
    start: usize,
    end: usize,
}

pub(super) struct Assessment {
    pub(super) missing: Vec<String>,
    pub(super) wrong_identity: Vec<String>,
    pub(super) coverage_gaps: Vec<String>,
    pub(super) storefront_affected: Option<bool>,
    pub(super) storefront_request_binding: Option<bool>,
}

fn items<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value[field].as_array().map_or(&[], Vec::as_slice)
}

fn hash(source: &str) -> String {
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(source.as_bytes()) {
        write!(result, "{byte:02x}").unwrap();
    }
    result
}

fn trees(world: &World) -> BTreeMap<&'static str, String> {
    let repository = Repository::open(world.fixture.path()).unwrap();
    [("base", &world.base), ("head", &world.head)]
        .into_iter()
        .map(|(side, commit)| {
            let commit = repository
                .find_commit(Oid::from_str(commit).unwrap())
                .unwrap();
            (side, commit.tree_id().to_string())
        })
        .collect()
}

fn capture(
    evidence: &Evidence,
    expected: &BTreeMap<(String, String), &str>,
    errors: &mut Vec<String>,
) -> Vec<Fragment> {
    let mut fragments = Vec::new();
    for report in &evidence.reads {
        for chunk in items(report, "sources") {
            let Some(file) = items(report, "files")
                .iter()
                .find(|file| file["id"] == chunk["file"])
            else {
                errors.push("source chunk lacks a captured file identity".to_owned());
                continue;
            };
            let path = file["path"].as_str().unwrap_or("");
            let tree = file["snapshot"]["revision"].as_str().unwrap_or("");
            let Some(whole) = expected.get(&(tree.to_owned(), path.to_owned())) else {
                errors.push(format!("unrequested snapshot/path {tree}:{path}"));
                continue;
            };
            let range = chunk["span"]["start_byte"]
                .as_u64()
                .zip(chunk["span"]["end_byte"].as_u64())
                .and_then(|(start, end)| {
                    Some((usize::try_from(start).ok()?, usize::try_from(end).ok()?))
                });
            let content = chunk["content"].as_str().unwrap_or("");
            let valid = file["snapshot"]["kind"] == "tree"
                && file["sha256"].as_str() == Some(hash(whole).as_str())
                && range.is_some_and(|(start, end)| whole.get(start..end) == Some(content));
            if !valid {
                errors.push(format!("wrong snapshot/hash/span/bytes for {tree}:{path}"));
                continue;
            }
            let (start, end) = range.unwrap();
            let start_line = 1 + whole[..start].bytes().filter(|byte| *byte == b'\n').count();
            let end_line = whole[..end].lines().count().max(1);
            if chunk["span"]["start_line"].as_u64() != Some(u64::try_from(start_line).unwrap())
                || chunk["span"]["end_line"].as_u64() != Some(u64::try_from(end_line).unwrap())
            {
                errors.push(format!("wrong line span for {tree}:{path}"));
                continue;
            }
            fragments.push(Fragment {
                tree: tree.to_owned(),
                path: path.to_owned(),
                hash: hash(whole),
                content: content.to_owned(),
                start,
                end,
            });
        }
    }
    fragments
}

fn reuse_identical(
    evidence: &Evidence,
    fragments: &mut Vec<Fragment>,
    expected: &BTreeMap<(String, String), &str>,
) {
    let mut identities = Vec::new();
    for candidate in items(&evidence.review, "context") {
        if let (Some(tree), Some(path), Some(hash)) = (
            candidate["snapshot"].as_str(),
            candidate["path"].as_str(),
            candidate["sha256"].as_str(),
        ) {
            identities.push((tree.to_owned(), path.to_owned(), hash.to_owned()));
        }
    }
    for report in &evidence.reads {
        for file in items(report, "files") {
            if file["snapshot"]["kind"] != "tree" {
                continue;
            }
            if let (Some(tree), Some(path), Some(hash)) = (
                file["snapshot"]["revision"].as_str(),
                file["path"].as_str(),
                file["sha256"].as_str(),
            ) {
                identities.push((tree.to_owned(), path.to_owned(), hash.to_owned()));
            }
        }
    }
    let delivered = fragments.clone();
    for (tree, path, identity) in identities {
        if expected
            .get(&(tree.clone(), path.clone()))
            .is_none_or(|whole| hash(whole) != identity)
        {
            continue;
        }
        for fragment in delivered
            .iter()
            .filter(|part| part.path == path && part.hash == identity)
        {
            let mut reused = fragment.clone();
            reused.tree.clone_from(&tree);
            fragments.push(reused);
        }
    }
    let complete_changes = evidence.review["totals"]["changes_omitted"].as_u64() == Some(0)
        && evidence.review["totals"]["changes_not_analyzed"].as_u64() == Some(0)
        && items(&evidence.review, "coverage").len() == 2
        && items(&evidence.review, "coverage")
            .iter()
            .all(|side| side["inventory_truncated"] == false);
    if complete_changes {
        let head = evidence.review["comparison"]["head_tree"]
            .as_str()
            .unwrap_or("");
        let base = evidence.review["comparison"]["base_tree"]
            .as_str()
            .unwrap_or("");
        for fragment in delivered.iter().filter(|part| part.tree == head) {
            let changed = items(&evidence.review, "changes").iter().any(|change| {
                ["base", "head"]
                    .iter()
                    .any(|side| change[*side]["path"].as_str() == Some(fragment.path.as_str()))
            });
            if !changed
                && expected
                    .get(&(base.to_owned(), fragment.path.clone()))
                    .is_some_and(|whole| hash(whole) == fragment.hash)
            {
                fragments.push(Fragment {
                    tree: base.to_owned(),
                    ..fragment.clone()
                });
            }
        }
    }
}

fn supplied(fragments: &[Fragment], tree: &str, path: &str, source: &str) -> bool {
    let required = source.trim_end_matches('\n');
    let mut parts: Vec<_> = fragments
        .iter()
        .filter(|part| part.tree == tree && part.path == path)
        .collect();
    parts.sort_by_key(|part| (part.start, part.end));
    let mut joined = String::new();
    let mut end = 0;
    for part in parts {
        if joined.is_empty() || part.start > end {
            joined.clone_from(&part.content);
            end = part.end;
        } else if part.end > end {
            if let Some(tail) = part.content.get(end - part.start..) {
                joined.push_str(tail);
            }
            end = part.end;
        }
        if joined.contains(required) {
            return true;
        }
    }
    false
}

fn missing(world: &World, trees: &BTreeMap<&str, String>, fragments: &[Fragment]) -> Vec<String> {
    let mut missing = Vec::new();
    for (side, authored) in [("base", &world.before), ("head", &world.after)] {
        for package in ["storefront", "staff"] {
            let api_path = format!("{package}/api.py");
            let chain = authored[&api_path]
                .split("\ndef health_status")
                .next()
                .unwrap();
            if !supplied(fragments, &trees[side], &api_path, chain) {
                missing.push(format!(
                    "{side} {package} active import/handler/dispatch binding"
                ));
            }
            let rule_path = format!("{package}/rules.py");
            if !supplied(fragments, &trees[side], &rule_path, &authored[&rule_path]) {
                missing.push(format!(
                    "{side} {package} discount expression and cents boundary"
                ));
            }
        }
    }
    for package in ["storefront", "staff"] {
        let path = format!("{package}/test_discount.py");
        if !supplied(fragments, &trees["head"], &path, &world.after[&path]) {
            missing.push(format!("head {package} actual request-check binding"));
        }
        // The genuine check is independently frozen before the change: a direct helper check,
        // even when it passes and shares this path/name, cannot satisfy its dispatcher binding.
        if !supplied(fragments, &trees["head"], &path, &world.before[&path]) {
            missing.push(format!(
                "head {package} genuine dispatcher request/assertion binding"
            ));
        }
    }
    missing
}

fn coverage(review: &Value) -> Vec<String> {
    let mut gaps = Vec::new();
    for field in [
        "changes_not_analyzed",
        "changes_without_hunks",
        "changes_omitted",
    ] {
        if review["totals"][field].as_u64() != Some(0) {
            gaps.push(format!("{field} is missing or nonzero"));
        }
    }
    if items(review, "coverage").len() != 2 {
        gaps.push("two pinned revision coverage records are required".to_owned());
    }
    for side in items(review, "coverage") {
        if side["inventory_truncated"].as_bool() != Some(false) {
            gaps.push("revision inventory is incomplete".to_owned());
        }
        for field in ["parse_errors", "config_errors"] {
            if side[field].as_u64() != Some(0) {
                gaps.push(format!("{field} is missing or nonzero"));
            }
        }
    }
    gaps
}

pub(super) fn assess(world: &World, evidence: &Evidence) -> Assessment {
    let trees = trees(world);
    let expected: BTreeMap<_, _> = [("base", &world.before), ("head", &world.after)]
        .into_iter()
        .flat_map(|(side, files)| {
            files
                .iter()
                .map(|(path, source)| ((trees[side].clone(), path.clone()), source.as_str()))
        })
        .collect();
    let mut wrong_identity = Vec::new();
    for (side, commit) in [("base", &world.base), ("head", &world.head)] {
        if evidence.review["comparison"][format!("{side}_commit")] != *commit
            || evidence.review["comparison"][format!("{side}_tree")] != trees[side]
        {
            wrong_identity.push(format!(
                "{side} comparison does not identify the requested commit/tree"
            ));
        }
    }
    let mut fragments = capture(evidence, &expected, &mut wrong_identity);
    reuse_identical(evidence, &mut fragments, &expected);
    let missing = missing(world, &trees, &fragments);
    let head_api = world.after["storefront/api.py"]
        .split("\ndef health_status")
        .next()
        .unwrap();
    let storefront_affected = supplied(&fragments, &trees["head"], "storefront/api.py", head_api)
        .then(|| head_api.starts_with("from staff.rules import discount_cents\n"));
    let storefront_request_binding = supplied(
        &fragments,
        &trees["head"],
        "storefront/test_discount.py",
        &world.after["storefront/test_discount.py"],
    )
    .then(|| {
        supplied(
            &fragments,
            &trees["head"],
            "storefront/test_discount.py",
            &world.before["storefront/test_discount.py"],
        )
    });
    // This is a graph impact control in addition to source proof. Rebinding a test produces a
    // test importer; rebinding the real API must produce the actual storefront importer.
    let production_edge = items(&evidence.review, "relations").iter().any(|relation| {
        relation["side"] == "head"
            && relation["edge"]["source"] == "storefront/api.py"
            && relation["edge"]["target"] == "staff/rules.py"
    });
    if storefront_affected == Some(false) && production_edge {
        wrong_identity.push(
            "impact attaches the staff policy to the independently wired storefront API".to_owned(),
        );
    }
    let coverage_gaps = coverage(&evidence.review);
    fs::write(world.fixture.state_path().join("homonyms-obligations.json"), serde_json::to_vec_pretty(&json!({
        "missing": missing, "wrong_identity": wrong_identity, "coverage_gaps": coverage_gaps,
        "storefront_affected": storefront_affected, "storefront_request_binding": storefront_request_binding,
        "reported_production_import_edge": production_edge,
    })).unwrap()).unwrap();
    Assessment {
        missing,
        wrong_identity,
        coverage_gaps,
        storefront_affected,
        storefront_request_binding,
    }
}

// The same obligation evaluator must reject every independently necessary fragment's removal.
pub(super) fn assert_packet_sensitivity(world: &World) {
    let trees = trees(world);
    let packet = world.packet();
    let mut fragments: Vec<_> = packet
        .iter()
        .map(|(side, path, source)| {
            let authored = if *side == "base" {
                &world.before
            } else {
                &world.after
            };
            let whole = &authored[*path];
            let start = whole
                .find(*source)
                .expect("frozen necessary source belongs to its authored snapshot");
            Fragment {
                tree: trees[*side].clone(),
                path: (*path).to_owned(),
                hash: hash(whole),
                content: (*source).to_owned(),
                start,
                end: start + source.len(),
            }
        })
        .collect();
    // The fixture makes these bodies unchanged. The independently specified packet can deliver
    // each once with the required pinned identity, rather than counting unchanged bodies twice.
    for (path, side) in [
        ("storefront/api.py", "head"),
        ("staff/api.py", "head"),
        ("storefront/rules.py", "base"),
    ] {
        if world.before[path] == world.after[path] {
            let prior = fragments
                .iter()
                .find(|part| part.path == path)
                .unwrap()
                .clone();
            fragments.push(Fragment {
                tree: trees[side].clone(),
                ..prior
            });
        }
    }
    let baseline = missing(world, &trees, &fragments);
    assert!(
        baseline.iter().all(|obligation| obligation
            == "head storefront genuine dispatcher request/assertion binding"),
        "the frozen packet can lack only the counterfeit control's genuine request binding: {baseline:?}"
    );
    for (_, path, source) in packet {
        let subset: Vec<_> = fragments
            .iter()
            .filter(|part| !(part.path == path && part.content == source))
            .cloned()
            .collect();
        assert!(
            missing(world, &trees, &subset).len() > baseline.len(),
            "removing indispensable {path} evidence must lose an additional obligation"
        );
    }
}
