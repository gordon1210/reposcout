//! Only public task inputs and CLI responses choose source targets.

use super::super::support::{CacheState, CostLedger};
use crate::journeys::support::Journey;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(super) struct Task<'a> {
    pub endpoint: &'a str,
    pub base: &'a str,
    pub head: &'a str,
    pub encoding: &'a str,
}

pub(super) struct Source {
    pub tree: String,
    pub path: String,
    pub hash: String,
    pub body: String,
    pub span: Value,
    pub snapshot_kind: String,
}

pub(super) struct Evidence {
    pub review: Value,
    pub sources: Vec<Source>,
}

impl Evidence {
    pub(super) fn contains(&self, side: &str, path: &str, fragments: &[&str]) -> bool {
        let tree = self.review["comparison"][format!("{side}_tree")].as_str();
        let mut sources = self
            .sources
            .iter()
            .filter(|source| Some(source.tree.as_str()) == tree && source.path == path)
            .collect::<Vec<_>>();
        sources.sort_by_key(|source| source.span["start_byte"].as_u64().unwrap());
        let mut body = String::new();
        let mut previous_end = None;
        for source in sources {
            let start = usize::try_from(source.span["start_byte"].as_u64().unwrap()).unwrap();
            let end = usize::try_from(source.span["end_byte"].as_u64().unwrap()).unwrap();
            let overlap = previous_end.map_or(0, |prior: usize| prior.saturating_sub(start));
            if previous_end.is_some_and(|prior| prior < start) {
                body.push('\n');
            }
            if overlap < source.body.len() {
                body.push_str(&source.body[overlap..]);
            }
            previous_end = Some(previous_end.map_or(end, |prior| prior.max(end)));
        }
        fragments.iter().all(|fragment| body.contains(fragment))
    }
}

fn invoke(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    label: &str,
    arguments: &[String],
) -> Value {
    let args = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    let state = if ledger.totals().calls == 0 {
        CacheState::Cold
    } else {
        CacheState::Warm
    };
    let step = ledger.step(journey, label, &args, state);
    step.assert_exit(0);
    step.stdout_json()
}

fn options(encoding: &str) -> Vec<String> {
    [
        "--encoding",
        encoding,
        "--budget",
        "4096",
        "--max-output-bytes",
        "16384",
        "--no-project-config",
        "-f",
        "json",
        "--quiet",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn collect_sources(report: &Value, evidence: &mut Evidence) {
    for chunk in report["sources"].as_array().into_iter().flatten() {
        let file = report["files"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|file| file["id"] == chunk["file"])
            .expect("public source chunk refers to a returned file");
        evidence.sources.push(Source {
            tree: file["snapshot"]["revision"].as_str().unwrap().to_owned(),
            path: file["path"].as_str().unwrap().to_owned(),
            hash: file["sha256"].as_str().unwrap().to_owned(),
            body: chunk["content"].as_str().unwrap().to_owned(),
            span: chunk["span"].clone(),
            snapshot_kind: file["snapshot"]["kind"].as_str().unwrap().to_owned(),
        });
    }
}

fn read_files(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    task: &Task<'_>,
    tree: &str,
    targets: &BTreeMap<String, Option<String>>,
    evidence: &mut Evidence,
) {
    if targets.is_empty() {
        return;
    }
    let mut arguments = ["read", ".", "--snapshot", tree]
        .map(str::to_owned)
        .to_vec();
    for (path, hash) in targets {
        arguments.extend(["--file".to_owned(), path.clone()]);
        if let Some(hash) = hash {
            arguments.extend(["--expect-hash".to_owned(), path.clone(), hash.clone()]);
        }
    }
    arguments.extend(options(task.encoding));
    let report = invoke(
        journey,
        ledger,
        "read discovered pinned checkout context",
        &arguments,
    );
    collect_sources(&report, evidence);
}

fn add_importers(review: &Value, paths: &mut BTreeMap<String, Option<String>>) {
    loop {
        let previous = paths.len();
        for relation in review["relations"].as_array().into_iter().flatten() {
            if relation["side"] != "head" {
                continue;
            }
            let edge = &relation["edge"];
            if let (Some(source), Some(target)) = (edge["source"].as_str(), edge["target"].as_str())
                && paths.contains_key(target)
            {
                paths.entry(source.to_owned()).or_insert(None);
            }
        }
        if paths.len() == previous {
            break;
        }
    }
}

fn missing_imports(evidence: &Evidence, tree: &str) -> BTreeMap<String, Option<String>> {
    let captured = evidence
        .sources
        .iter()
        .filter(|source| source.tree == tree)
        .map(|source| source.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut targets = BTreeMap::new();
    for source in evidence.sources.iter().filter(|source| source.tree == tree) {
        for line in source.body.lines() {
            let words = line.split_whitespace().collect::<Vec<_>>();
            if words.len() >= 4
                && words[0] == "from"
                && words[2] == "import"
                && words[1].split('.').all(|part| {
                    !part.is_empty()
                        && part
                            .chars()
                            .all(|letter| letter.is_alphanumeric() || letter == '_')
                })
            {
                let path = format!("{}.py", words[1].replace('.', "/"));
                if !captured.contains(path.as_str()) {
                    targets.insert(path, None);
                }
            }
        }
    }
    targets
}

fn scope(found: &Value) -> &str {
    found["hits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|hit| hit["read"]["path"].as_str())
        .find(|path| {
            !Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("test_") || name.ends_with("_test.py"))
        })
        .and_then(|path| Path::new(path).parent())
        .and_then(Path::to_str)
        .filter(|parent| !parent.is_empty())
        .unwrap_or(".")
}

fn old_targets(
    review: &Value,
    paths: &BTreeMap<String, Option<String>>,
) -> BTreeMap<String, Option<String>> {
    let mut targets = BTreeMap::new();
    for change in review["changes"].as_array().into_iter().flatten() {
        if let (Some(old), Some(new)) = (
            change["base"]["path"].as_str(),
            change["head"]["path"].as_str(),
        ) && paths.contains_key(new)
        {
            targets.insert(
                old.to_owned(),
                change["base"]["sha256"].as_str().map(str::to_owned),
            );
        }
    }
    targets
}

pub(super) fn prepare(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    task: &Task<'_>,
) -> Evidence {
    let query = task.endpoint.trim_matches('/');
    let mut arguments = ["find", query, ".", "--limit", "8"]
        .map(str::to_owned)
        .to_vec();
    arguments.extend(options(task.encoding));
    let found = invoke(
        journey,
        ledger,
        "discover the public checkout endpoint",
        &arguments,
    );
    let mut paths = BTreeMap::new();
    for hit in found["hits"].as_array().into_iter().flatten() {
        if let Some(path) = hit["read"]["path"].as_str() {
            paths.insert(
                path.to_owned(),
                hit["read"]["expected_hash"].as_str().map(str::to_owned),
            );
        }
    }
    let mut arguments = [
        "review-context",
        scope(&found),
        "--base",
        task.base,
        "--head",
        task.head,
    ]
    .map(str::to_owned)
    .to_vec();
    arguments.extend(options(task.encoding));
    let review = invoke(
        journey,
        ledger,
        "pin the endpoint's PR comparison",
        &arguments,
    );
    add_importers(&review, &mut paths);
    let base_tree = review["comparison"]["base_tree"]
        .as_str()
        .unwrap()
        .to_owned();
    let head_tree = review["comparison"]["head_tree"]
        .as_str()
        .unwrap()
        .to_owned();
    let old_paths = old_targets(&review, &paths);
    let mut evidence = Evidence {
        review,
        sources: Vec::new(),
    };
    read_files(journey, ledger, task, &head_tree, &paths, &mut evidence);
    let imports = missing_imports(&evidence, &head_tree);
    read_files(journey, ledger, task, &head_tree, &imports, &mut evidence);
    read_files(journey, ledger, task, &base_tree, &old_paths, &mut evidence);
    evidence
}
