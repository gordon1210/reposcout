//! Review readiness is judged against authored application facts, not report vocabulary.

use super::support::{Fixture, Journey};
use git2::{Oid, Repository};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

mod deletion;
mod refund;

#[derive(Clone)]
struct SourceEvidence {
    tree: String,
    path: String,
    body: String,
    hash: String,
    provenance: String,
}

struct ReviewEvidence {
    report: Value,
    sources: Vec<SourceEvidence>,
}

impl ReviewEvidence {
    fn contains(&self, side: &str, path: &str, fragments: &[&str]) -> bool {
        let tree = self.report["comparison"][format!("{side}_tree")]
            .as_str()
            .unwrap();
        let body = self
            .sources
            .iter()
            .filter(|source| source.tree == tree && source.path == path)
            .map(|source| source.body.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        fragments.iter().all(|fragment| body.contains(fragment))
    }
}

// The driver receives revisions and a CLI handle only. All path choices come from responses.
fn prepare_review(journey: &mut Journey<'_>, base: &str, head: &str) -> ReviewEvidence {
    let report = journey
        .step(
            "request source evidence for the supplied PR revisions",
            &[
                "review-context",
                ".",
                "--base",
                base,
                "--head",
                head,
                "--source",
                "--diff",
                "--context-budget",
                "24000",
                "--context-max-files",
                "40",
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
    let sources = report["context"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| {
            Some(SourceEvidence {
                tree: entry["snapshot"].as_str()?.to_owned(),
                path: entry["path"].as_str()?.to_owned(),
                body: entry["source"].as_str()?.to_owned(),
                hash: entry["sha256"].as_str()?.to_owned(),
                provenance: "whole-file source from review-context".to_owned(),
            })
        })
        .collect();
    ReviewEvidence { report, sources }
}

fn confirm_remaining_sources(journey: &mut Journey<'_>, evidence: &mut ReviewEvidence) {
    let base_tree = evidence.report["comparison"]["base_tree"].as_str().unwrap();
    let head_tree = evidence.report["comparison"]["head_tree"]
        .as_str()
        .unwrap()
        .to_owned();
    let prior_sources: Vec<_> = evidence
        .sources
        .iter()
        .filter(|source| {
            source.tree == base_tree
                && !evidence
                    .sources
                    .iter()
                    .any(|other| other.tree == head_tree && other.path == source.path)
        })
        .cloned()
        .collect();
    let paths: BTreeSet<_> = prior_sources
        .iter()
        .map(|source| source.path.as_str())
        .collect();
    if paths.is_empty() {
        return;
    }
    let mut args = vec![
        "read".to_owned(),
        ".".to_owned(),
        "--snapshot".to_owned(),
        head_tree.clone(),
    ];
    for path in paths {
        args.extend(["--outline".to_owned(), path.to_owned()]);
    }
    args.extend(
        [
            "--budget",
            "8192",
            "--no-project-config",
            "-f",
            "json",
            "--quiet",
        ]
        .map(str::to_owned),
    );
    let report = journey
        .step(
            "check the PR-tip identity of consumers learned from the base revision",
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            0,
        )
        .stdout_json();
    for file in report["files"].as_array().unwrap() {
        if file["snapshot"]["revision"].as_str() != Some(head_tree.as_str()) {
            continue;
        }
        if let Some(prior) = prior_sources.iter().find(|source| {
            file["path"].as_str() == Some(source.path.as_str())
                && file["sha256"].as_str() == Some(source.hash.as_str())
        }) {
            let mut source = prior.clone();
            source.provenance = format!(
                "returned base source at {} plus pinned head outline confirming identical whole-file SHA-256",
                source.tree
            );
            source.tree.clone_from(&head_tree);
            evidence.sources.push(source);
        }
    }
}

fn sha256(bytes: &[u8]) -> String {
    let mut hash = String::new();
    for byte in Sha256::digest(bytes) {
        write!(hash, "{byte:02x}").unwrap();
    }
    hash
}

// This verifier may inspect the authored repository; the driver above cannot.
fn assert_source_attribution(fixture: &Fixture, evidence: &ReviewEvidence, base: &str, head: &str) {
    let repository = Repository::open(fixture.path()).unwrap();
    for (side, revision) in [("base", base), ("head", head)] {
        let commit = repository
            .find_commit(Oid::from_str(revision).unwrap())
            .unwrap();
        assert_eq!(
            evidence.report["comparison"][format!("{side}_commit")],
            revision
        );
        assert_eq!(
            evidence.report["comparison"][format!("{side}_tree")],
            commit.tree_id().to_string(),
            "review source must belong to the requested revision"
        );
    }
    for source in &evidence.sources {
        assert!(
            ["base_tree", "head_tree"].iter().any(|key| {
                evidence.report["comparison"][key].as_str() == Some(source.tree.as_str())
            }),
            "unrequested source revision: {}",
            source.provenance
        );
        let tree = repository
            .find_tree(Oid::from_str(&source.tree).unwrap())
            .unwrap();
        let entry = tree.get_path(Path::new(&source.path)).unwrap();
        let blob = repository.find_blob(entry.id()).unwrap();
        let actual = std::str::from_utf8(blob.content()).unwrap();
        assert_eq!(source.hash, sha256(blob.content()), "{}", source.path);
        assert!(
            actual.contains(&source.body),
            "misleading source for {}: {}",
            source.path,
            source.provenance
        );
    }
}
