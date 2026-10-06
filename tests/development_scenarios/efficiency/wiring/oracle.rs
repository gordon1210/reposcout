use super::driver::Evidence;
use crate::support::Fixture;
use git2::{Oid, Repository};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub(super) type Packet<'a> = Vec<(&'a str, &'a str, &'a str)>;

pub(super) struct Input<'a> {
    pub(super) fixture: &'a Fixture,
    pub(super) base: &'a str,
    pub(super) head: &'a str,
    pub(super) before: &'a BTreeMap<String, String>,
    pub(super) after: &'a BTreeMap<String, String>,
    pub(super) packet: &'a Packet<'a>,
    pub(super) test_path: &'a str,
    pub(super) genuine_test: &'a str,
    pub(super) selected_policy_path: &'a str,
    pub(super) selected_policy: &'a str,
}

#[derive(Clone)]
struct Fragment {
    tree: String,
    path: String,
    hash: String,
    start: usize,
    end: usize,
    content: String,
}

pub(super) struct Assessment {
    pub(super) missing: Vec<String>,
    pub(super) wrong_identity: Vec<String>,
    pub(super) false_changes: Vec<String>,
    pub(super) coverage_gaps: Vec<String>,
    pub(super) context_diagnostics: Vec<String>,
}

pub(super) fn hash(source: &str) -> String {
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(source.as_bytes()) {
        write!(digest, "{byte:02x}").unwrap();
    }
    digest
}

fn identity(repository: &Repository, revision: &str) -> String {
    repository
        .find_commit(Oid::from_str(revision).unwrap())
        .unwrap()
        .tree_id()
        .to_string()
}

fn capture_sources(
    evidence: &Evidence,
    expected: &BTreeMap<(String, String), &str>,
    errors: &mut Vec<String>,
) -> Vec<Fragment> {
    let mut fragments = Vec::new();
    for report in &evidence.reads {
        for chunk in report["sources"].as_array().into_iter().flatten() {
            let file = report["files"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|file| file["id"] == chunk["file"]);
            let Some(file) = file else {
                errors.push("source chunk has no file identity".to_owned());
                continue;
            };
            if !report["results"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|result| {
                    result["source"] == chunk["id"]
                        && result["file"] == file["id"]
                        && result["status"] == "complete"
                })
            {
                errors.push("source chunk lacks a complete attributed target result".to_owned());
                continue;
            }
            let path = file["path"].as_str().unwrap_or("");
            let tree = file["snapshot"]["revision"].as_str().unwrap_or("");
            let Some(whole) = expected.get(&(tree.to_owned(), path.to_owned())) else {
                errors.push(format!("unrequested revision/path {tree}:{path}"));
                continue;
            };
            let start = chunk["span"]["start_byte"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok());
            let end = chunk["span"]["end_byte"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok());
            let content = chunk["content"].as_str().unwrap_or("");
            let correct = file["snapshot"]["kind"] == "tree"
                && file["sha256"].as_str() == Some(hash(whole).as_str())
                && start
                    .zip(end)
                    .is_some_and(|(start, end)| whole.get(start..end) == Some(content));
            if !correct {
                errors.push(format!("wrong hash/snapshot/span/bytes for {tree}:{path}"));
                continue;
            }
            let (start, end) = start.zip(end).unwrap();
            let correct_lines = chunk["span"]["start_line"].as_u64()
                == Some(
                    1 + u64::try_from(whole[..start].bytes().filter(|b| *b == b'\n').count())
                        .unwrap(),
                )
                && chunk["span"]["end_line"].as_u64()
                    == Some(u64::try_from(whole[..end].lines().count().max(1)).unwrap());
            if !correct_lines {
                errors.push(format!("wrong line span for {tree}:{path}"));
                continue;
            }
            fragments.push(Fragment {
                tree: tree.to_owned(),
                path: path.to_owned(),
                hash: hash(whole),
                start,
                end,
                content: content.to_owned(),
            });
        }
    }
    fragments
}

fn retain_equal_identities(
    evidence: &Evidence,
    fragments: &mut Vec<Fragment>,
    expected: &BTreeMap<(String, String), &str>,
) {
    let delivered = fragments.clone();
    for candidate in evidence.review["context"].as_array().into_iter().flatten() {
        let (Some(tree), Some(path), Some(candidate_hash)) = (
            candidate["snapshot"].as_str(),
            candidate["path"].as_str(),
            candidate["sha256"].as_str(),
        ) else {
            continue;
        };
        let Some(whole) = expected.get(&(tree.to_owned(), path.to_owned())) else {
            continue;
        };
        if candidate_hash != hash(whole) {
            continue;
        }
        for prior in delivered
            .iter()
            .filter(|prior| prior.path == path && prior.hash == candidate_hash)
        {
            let mut retained = prior.clone();
            tree.clone_into(&mut retained.tree);
            fragments.push(retained);
        }
    }
}

fn supplied(fragments: &[Fragment], tree: &str, path: &str, source: &str) -> bool {
    // Adjacent attributed import/registration/declaration chunks may collectively supply a
    // packet. Whitespace between them is not a business or binding obligation.
    source
        .bytes()
        .enumerate()
        .filter(|(_, byte)| !byte.is_ascii_whitespace())
        .all(|(offset, byte)| {
            fragments.iter().any(|fragment| {
                fragment.tree == tree
                    && fragment.path == path
                    && fragment.start <= offset
                    && offset < fragment.end
                    && fragment.content.as_bytes().get(offset - fragment.start) == Some(&byte)
            })
        })
}

fn coverage(review: &Value) -> (Vec<String>, Vec<String>) {
    let mut gaps = Vec::new();
    let mut diagnostics = Vec::new();
    for field in [
        "changes_not_analyzed",
        "changes_without_hunks",
        "changes_omitted",
    ] {
        if review["totals"][field].as_u64() != Some(0) {
            gaps.push(format!("{field} is missing or nonzero"));
        }
    }
    match review["totals"]["candidates_omitted"].as_u64() {
        Some(0) => {}
        Some(count) => diagnostics.push(format!("{count} context candidates omitted")),
        None => diagnostics.push("context candidate omission count unavailable".to_owned()),
    }
    for side in review["coverage"].as_array().into_iter().flatten() {
        if side["inventory_truncated"].as_bool() != Some(false) {
            gaps.push("revision inventory incomplete".to_owned());
        }
        for field in ["parse_errors", "config_errors"] {
            if side[field].as_u64() != Some(0) {
                gaps.push(format!("{field} is missing or nonzero"));
            }
        }
    }
    (gaps, diagnostics)
}

pub(super) fn assess(input: &Input<'_>, evidence: &Evidence) -> Assessment {
    let repository = Repository::open(input.fixture.path()).unwrap();
    let trees = BTreeMap::from([
        ("base", identity(&repository, input.base)),
        ("head", identity(&repository, input.head)),
    ]);
    let expected: BTreeMap<_, _> = [("base", input.before), ("head", input.after)]
        .into_iter()
        .flat_map(|(side, files)| {
            files
                .iter()
                .map(|(path, source)| ((trees[side].clone(), path.clone()), source.as_str()))
                .collect::<Vec<_>>()
        })
        .collect();
    for ((tree, path), source) in &expected {
        let tree = repository.find_tree(Oid::from_str(tree).unwrap()).unwrap();
        let entry = tree.get_path(Path::new(path)).unwrap();
        let blob = repository.find_blob(entry.id()).unwrap();
        assert_eq!(
            blob.content(),
            source.as_bytes(),
            "frozen bytes must be in the requested Git tree: {path}"
        );
    }
    let mut wrong_identity = Vec::new();
    for (side, revision) in [("base", input.base), ("head", input.head)] {
        if evidence.review["comparison"][format!("{side}_commit")] != revision
            || evidence.review["comparison"][format!("{side}_tree")] != trees[side]
        {
            wrong_identity.push(format!(
                "{side} comparison is not the requested commit/tree"
            ));
        }
    }
    let mut fragments = capture_sources(evidence, &expected, &mut wrong_identity);
    retain_equal_identities(evidence, &mut fragments, &expected);
    let mut missing: Vec<_> = input
        .packet
        .iter()
        .filter(|(side, path, source)| !supplied(&fragments, &trees[*side], path, source))
        .map(|(side, path, _)| format!("{side} necessary evidence {path}"))
        .collect();
    if !supplied(
        &fragments,
        &trees["head"],
        input.selected_policy_path,
        input.selected_policy,
    ) {
        missing.push("head actively selected tariff implementation".to_owned());
    }
    if !supplied(
        &fragments,
        &trees["head"],
        input.test_path,
        input.genuine_test,
    ) {
        missing.push("genuine dispatcher request/assertion binding".to_owned());
    }
    let false_changes = unchanged_declaration_claims(&evidence.review, input.before, input.after);
    let (coverage_gaps, context_diagnostics) = coverage(&evidence.review);
    fs::write(input.fixture.state_path().join("wiring-obligations.json"), serde_json::to_vec_pretty(&json!({"missing": missing, "wrong_identity": wrong_identity, "false_changes": false_changes, "coverage_gaps": coverage_gaps, "context_diagnostics": context_diagnostics})).unwrap()).unwrap();
    Assessment {
        missing,
        wrong_identity,
        false_changes,
        coverage_gaps,
        context_diagnostics,
    }
}

fn unchanged_declaration_claims(
    review: &Value,
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut wrong = Vec::new();
    for change in review["changes"].as_array().into_iter().flatten() {
        for side in ["base", "head"] {
            let file = &change[side];
            let path = file["path"].as_str().unwrap_or("");
            for definition in file["definitions"].as_array().into_iter().flatten() {
                let identical_file = before
                    .get(path)
                    .is_some_and(|source| after.get(path) == Some(source));
                // The independently frozen Python fixture edits imports only in this module.
                let unchanged_handler =
                    path == "shipping/quote.py" && definition["symbol"]["name"] == "handle_quote";
                if identical_file || unchanged_handler {
                    wrong.push(format!(
                        "unchanged declaration claimed edited: {side}:{path}:{}",
                        definition["symbol"]["name"]
                    ));
                }
            }
        }
    }
    wrong
}

fn authored_evidence(input: &Input<'_>) -> Evidence {
    let repository = Repository::open(input.fixture.path()).unwrap();
    let trees = BTreeMap::from([
        ("base", identity(&repository, input.base)),
        ("head", identity(&repository, input.head)),
    ]);
    let context: Vec<_> = [("base", input.before), ("head", input.after)].into_iter().flat_map(|(side, files)| files.iter().map(|(path, source)| json!({"snapshot": trees[side], "path": path, "sha256": hash(source)})).collect::<Vec<_>>()).collect();
    let reads = input.packet.iter().map(|(side, path, source)| {
        let files = if *side == "base" { input.before } else { input.after };
        assert_eq!(files[*path], *source);
        json!({"kind": "source_query", "mode": "source", "files": [{"id": 1, "path": path, "sha256": hash(source), "snapshot": {"kind": "tree", "revision": trees[*side]}}], "results": [{"target": 1, "status": "complete", "selection": "file", "file": 1, "source": 1}], "sources": [{"id": 1, "file": 1, "content": source, "span": {"start_byte": 0, "end_byte": source.len(), "start_line": 1, "end_line": source.lines().count().max(1)}}]})
    }).collect();
    Evidence {
        review: json!({"comparison": {"base_commit": input.base, "head_commit": input.head, "base_tree": trees["base"], "head_tree": trees["head"]}, "context": context, "changes": [], "totals": {"changes_not_analyzed": 0, "changes_without_hunks": 0, "changes_omitted": 0, "candidates_omitted": 0}, "coverage": [{"inventory_truncated": false, "parse_errors": 0, "config_errors": 0}, {"inventory_truncated": false, "parse_errors": 0, "config_errors": 0}]}),
        reads,
    }
}

pub(super) fn assert_packet_sensitivity(input: &Input<'_>, counterfeit: bool) {
    let authored = authored_evidence(input);
    let full = assess(input, &authored);
    assert!(
        full.wrong_identity.is_empty()
            && full.false_changes.is_empty()
            && full.coverage_gaps.is_empty()
    );
    assert_eq!(
        full.missing.len(),
        usize::from(counterfeit),
        "the independent packet must satisfy the same evidence evaluator"
    );
    for removed in 0..authored.reads.len() {
        let altered = Evidence {
            review: authored.review.clone(),
            reads: authored
                .reads
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != removed)
                .map(|(_, value)| value.clone())
                .collect(),
        };
        let result = assess(input, &altered);
        assert!(
            result.missing.len() > full.missing.len(),
            "removing indispensable evidence must fail the real evaluator"
        );
    }
    let mut wrong_identity = Evidence {
        review: authored.review.clone(),
        reads: authored.reads.clone(),
    };
    wrong_identity.reads[0]["files"][0]["sha256"] = json!("0".repeat(64));
    assert!(
        !assess(input, &wrong_identity).wrong_identity.is_empty(),
        "right text under a wrong hash must fail the real evaluator"
    );
}
