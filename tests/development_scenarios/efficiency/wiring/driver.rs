use super::super::support::{CacheState, CostLedger};
use crate::journeys::support::Journey;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Evidence {
    pub(super) review: Value,
    pub(super) reads: Vec<Value>,
}

struct Target {
    path: String,
    hash: Option<String>,
}

fn items<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value[field].as_array().map_or(&[], Vec::as_slice)
}

fn targets(review: &Value, side: &str) -> Vec<Target> {
    let tree = review["comparison"][format!("{side}_tree")].as_str();
    let mut selected = BTreeMap::new();
    let mut changed = BTreeSet::new();
    for change in items(review, "changes") {
        let file = &change[side];
        if let Some(path) = file["path"].as_str() {
            changed.insert(path.to_owned());
            selected.insert(path.to_owned(), file["sha256"].as_str().map(str::to_owned));
        }
    }
    for relation in items(review, "relations") {
        if relation["side"].as_str() != Some(side) {
            continue;
        }
        let source = relation["edge"]["source"].as_str();
        let target = relation["edge"]["target"].as_str();
        if side == "head" {
            for path in source.into_iter().chain(target) {
                selected.entry(path.to_owned()).or_insert(None);
            }
        } else if source.is_some_and(|path| changed.contains(path))
            && let Some(path) = target
        {
            selected.entry(path.to_owned()).or_insert(None);
        }
    }
    for candidate in items(review, "context") {
        if candidate["snapshot"].as_str() != tree {
            continue;
        }
        let Some(path) = candidate["path"].as_str() else {
            continue;
        };
        if (side == "head" || selected.contains_key(path))
            && let Some(hash) = candidate["sha256"].as_str()
        {
            selected.insert(path.to_owned(), Some(hash.to_owned()));
        }
    }
    selected
        .into_iter()
        .map(|(path, hash)| Target { path, hash })
        .collect()
}

fn read_args(
    tree: &str,
    targets: &[Target],
    encoding: &str,
    tokens: usize,
    bytes: usize,
) -> Vec<String> {
    let tokens = tokens.to_string();
    let bytes = bytes.to_string();
    let mut args: Vec<_> = [
        "read",
        ".",
        "--snapshot",
        tree,
        "--budget",
        &tokens,
        "--max-output-bytes",
        &bytes,
        "--encoding",
        encoding,
    ]
    .map(str::to_owned)
    .to_vec();
    for target in targets {
        args.extend(["--file".to_owned(), target.path.clone()]);
        if let Some(hash) = &target.hash {
            args.extend([
                "--expect-hash".to_owned(),
                target.path.clone(),
                hash.clone(),
            ]);
        }
    }
    args.extend(["--no-project-config", "-f", "json", "--quiet"].map(str::to_owned));
    args
}

fn allowance(
    ledger: &CostLedger,
    reserve_tokens: usize,
    reserve_bytes: usize,
) -> Option<(usize, usize)> {
    let tokens = super::LIMITS
        .response_tokens
        .saturating_sub(ledger.totals().response_tokens + reserve_tokens);
    let bytes = super::LIMITS
        .response_bytes
        .saturating_sub(ledger.totals().response_bytes + reserve_bytes);
    (tokens >= 256 && bytes >= 1024 && ledger.totals().calls < super::LIMITS.calls)
        .then_some((tokens, bytes))
}

fn read(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    evidence: &mut Evidence,
    side: &str,
    selected: &[Target],
    encoding: &str,
    reserve: (usize, usize),
) {
    if selected.is_empty() {
        return;
    }
    let Some((tokens, bytes)) = allowance(ledger, reserve.0, reserve.1) else {
        return;
    };
    let Some(tree) = evidence.review["comparison"][format!("{side}_tree")].as_str() else {
        return;
    };
    let args = read_args(tree, selected, encoding, tokens.min(3000), bytes.min(10240));
    let borrowed: Vec<_> = args.iter().map(String::as_str).collect();
    let step = ledger.step(
        journey,
        "read response-discovered binding and request evidence",
        &borrowed,
        CacheState::Warm,
    );
    step.assert_exit(0);
    evidence.reads.push(step.stdout_json());
}

fn sources(evidence: &Evidence, side: &str) -> Vec<(String, String)> {
    let tree = evidence.review["comparison"][format!("{side}_tree")].as_str();
    evidence
        .reads
        .iter()
        .flat_map(|report| {
            items(report, "sources").iter().filter_map(|chunk| {
                let file = items(report, "files")
                    .iter()
                    .find(|file| file["id"] == chunk["file"])?;
                (file["snapshot"]["revision"].as_str() == tree).then(|| {
                    Some((
                        file["path"].as_str()?.to_owned(),
                        chunk["content"].as_str()?.to_owned(),
                    ))
                })?
            })
        })
        .collect()
}

fn relative(owner: &str, specifier: &str) -> Option<String> {
    let mut parts: Vec<_> = owner.split('/').collect();
    parts.pop();
    for part in specifier.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop()?;
            }
            "" => return None,
            name => parts.push(name),
        }
    }
    Some(parts.join("/"))
}

fn imported_paths(evidence: &Evidence, side: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for (owner, source) in sources(evidence, side) {
        if owner.ends_with("package.json")
            && let Ok(package) = serde_json::from_str::<Value>(&source)
        {
            for mapping in package["imports"]
                .as_object()
                .into_iter()
                .flat_map(|imports| imports.values())
            {
                if let Some(specifier) = mapping.as_str()
                    && let Some(path) = relative(&owner, specifier)
                {
                    paths.insert(path);
                }
            }
        }
        for line in source.lines().map(str::trim) {
            if std::path::Path::new(&owner)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("py"))
                && let Some(module) = line
                    .strip_prefix("from ")
                    .and_then(|rest| rest.split_whitespace().next())
                && !module.starts_with('.')
            {
                paths.insert(format!("{}.py", module.replace('.', "/")));
            }
            if line.starts_with("import ")
                && let Some(specifier) = line
                    .split_once(" from ")
                    .map(|(_, rest)| rest.trim().trim_end_matches(';').trim_matches(['\'', '"']))
                && specifier.starts_with('.')
                && let Some(path) = relative(&owner, specifier)
            {
                paths.insert(path);
            }
        }
    }
    paths
}

fn unread(evidence: &Evidence, side: &str, paths: BTreeSet<String>) -> Vec<Target> {
    let delivered: BTreeSet<_> = sources(evidence, side)
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    let known: BTreeMap<_, _> = targets(&evidence.review, side)
        .into_iter()
        .map(|target| (target.path, target.hash))
        .collect();
    paths
        .into_iter()
        .filter(|path| !delivered.contains(path))
        .map(|path| Target {
            hash: known.get(&path).cloned().flatten(),
            path,
        })
        .collect()
}

fn follow_missing(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    evidence: &mut Evidence,
    task: &str,
    encoding: &str,
) {
    let base_paths = imported_paths(evidence, "base");
    let mut head_paths = imported_paths(evidence, "head");
    // Prior imports are navigation candidates in the new revision, not proof of its binding.
    head_paths.extend(base_paths.iter().cloned());
    let has_request_check = sources(evidence, "head")
        .iter()
        .any(|(path, source)| path.contains("test") && source.contains("assert"));
    if unread(evidence, "base", base_paths.clone()).is_empty()
        && unread(evidence, "head", head_paths.clone()).is_empty()
        && has_request_check
    {
        return;
    }
    let query = task
        .split_ascii_whitespace()
        .filter(|word| matches!(*word, "shipping" | "quote"))
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
        .collect::<Vec<_>>()
        .join(" ");
    if !has_request_check && let Some((tokens, bytes)) = allowance(ledger, 1200, 4096) {
        let tokens = tokens.min(1200).to_string();
        let bytes = bytes.min(4096).to_string();
        let step = ledger.step(
            journey,
            "find missing shipping and quote entrypoints from the user task",
            &[
                "find",
                &query,
                ".",
                "--match",
                "any",
                "--limit",
                "8",
                "--budget",
                &tokens,
                "--max-output-bytes",
                &bytes,
                "--encoding",
                encoding,
                "--no-project-config",
                "-f",
                "json",
                "--quiet",
            ],
            CacheState::Warm,
        );
        step.assert_exit(0);
        for hit in items(&step.stdout_json(), "hits") {
            if let Some(path) = hit["read"]["path"]
                .as_str()
                .or_else(|| hit["path"].as_str())
                && (path.contains("shipping") || path.contains("test"))
            {
                head_paths.insert(path.to_owned());
            }
        }
    }
    let selected = unread(evidence, "base", base_paths);
    read(
        journey,
        ledger,
        evidence,
        "base",
        &selected,
        encoding,
        (900, 3072),
    );
    head_paths.extend(imported_paths(evidence, "base"));
    head_paths.extend(imported_paths(evidence, "head"));
    let selected = unread(evidence, "head", head_paths);
    read(
        journey,
        ledger,
        evidence,
        "head",
        &selected,
        encoding,
        (0, 0),
    );
    let selected = unread(evidence, "head", imported_paths(evidence, "head"));
    read(
        journey,
        ledger,
        evidence,
        "head",
        &selected,
        encoding,
        (0, 0),
    );
}

// Repository/revisions, user task and public handles are the complete driver input.
// The task defines the review scope; file selectors and hashes come only from CLI responses.
pub(super) fn prepare(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    base: &str,
    head: &str,
    task: &str,
    encoding: &str,
) -> Evidence {
    assert!(!task.is_empty());
    let step = ledger.step(
        journey,
        "discover the supplied pinned wiring comparison",
        &[
            "review-context",
            ".",
            "--base",
            base,
            "--head",
            head,
            "--budget",
            "1500",
            "--max-output-bytes",
            "6144",
            "--encoding",
            encoding,
            "--no-project-config",
            "-f",
            "json",
            "--quiet",
        ],
        CacheState::Cold,
    );
    step.assert_exit(0);
    let mut evidence = Evidence {
        review: step.stdout_json(),
        reads: Vec::new(),
    };
    for side in ["base", "head"] {
        let discovered = targets(&evidence.review, side);
        let reserve = if side == "base" {
            (3500, 11264)
        } else {
            (2000, 7168)
        };
        read(
            journey,
            ledger,
            &mut evidence,
            side,
            &discovered,
            encoding,
            reserve,
        );
    }
    follow_missing(journey, ledger, &mut evidence, task, encoding);
    evidence
}
