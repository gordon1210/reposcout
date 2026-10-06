use super::super::support::{CacheState, CostLedger};
use crate::journeys::support::Journey;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Evidence {
    pub(super) review: Value,
    pub(super) reads: Vec<Value>,
}

struct Discovery {
    review: Value,
    head_targets: BTreeMap<String, String>,
    base_changed: BTreeMap<String, String>,
}

fn items<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value[field].as_array().map_or(&[], Vec::as_slice)
}

fn invoke(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    label: &str,
    args: &[String],
    state: CacheState,
) -> Value {
    let borrowed: Vec<_> = args.iter().map(String::as_str).collect();
    let step = ledger.step(journey, label, &borrowed, state);
    step.assert_exit(0);
    step.stdout_json()
}

fn read_args(
    snapshot: &str,
    targets: &BTreeMap<String, String>,
    encoding: &str,
    tokens: &str,
    bytes: &str,
) -> Vec<String> {
    let mut args: Vec<_> = [
        "read",
        ".",
        "--snapshot",
        snapshot,
        "--budget",
        tokens,
        "--max-output-bytes",
        bytes,
        "--encoding",
        encoding,
    ]
    .map(str::to_owned)
    .to_vec();
    for (path, hash) in targets {
        args.extend(["--file".to_owned(), path.clone()]);
        if !hash.is_empty() {
            args.extend(["--expect-hash".to_owned(), path.clone(), hash.clone()]);
        }
    }
    args.extend(["--no-project-config", "-f", "json", "--quiet"].map(str::to_owned));
    args
}

// These are the complete driver inputs. Application names and the discount rule come from the user;
// every file selector/hash below comes from a public response rather than the authored fixture.
fn discover(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    base: &str,
    head: &str,
    applications: [&str; 2],
    encoding: &str,
) -> Discovery {
    let review_args = [
        "review-context",
        ".",
        "--base",
        base,
        "--head",
        head,
        "--budget",
        "2000",
        "--max-output-bytes",
        "7168",
        "--encoding",
        encoding,
        "--no-project-config",
        "-f",
        "json",
        "--quiet",
    ]
    .map(str::to_owned);
    let review = invoke(
        journey,
        ledger,
        "discover the supplied discount comparison",
        &review_args,
        CacheState::Cold,
    );
    let mut head_targets = BTreeMap::new();
    let mut base_changed = BTreeMap::new();
    for change in items(&review, "changes") {
        for (side, targets) in [("head", &mut head_targets), ("base", &mut base_changed)] {
            let file = &change[side];
            if let (Some(path), Some(hash)) = (file["path"].as_str(), file["sha256"].as_str()) {
                targets.insert(path.to_owned(), hash.to_owned());
            }
        }
    }
    for application in applications {
        let query = format!("{application} discount");
        let args = [
            "find",
            &query,
            ".",
            "--match",
            "all",
            "--limit",
            "12",
            "--budget",
            "900",
            "--max-output-bytes",
            "3072",
            "--encoding",
            encoding,
            "--no-project-config",
            "-f",
            "json",
            "--quiet",
        ]
        .map(str::to_owned);
        let found = invoke(
            journey,
            ledger,
            "discover active application request candidates",
            &args,
            CacheState::Warm,
        );
        for hit in items(&found, "hits") {
            if let (Some(path), Some(hash)) = (
                hit["read"]["path"].as_str(),
                hit["read"]["expected_hash"].as_str(),
            ) {
                head_targets.insert(path.to_owned(), hash.to_owned());
            }
        }
    }
    Discovery {
        review,
        head_targets,
        base_changed,
    }
}

pub(super) fn prepare(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    base: &str,
    head: &str,
    applications: [&str; 2],
    encoding: &str,
) -> Evidence {
    let Discovery {
        review,
        head_targets,
        base_changed,
    } = discover(journey, ledger, base, head, applications, encoding);
    let mut reads = Vec::new();
    let Some(head_tree) = review["comparison"]["head_tree"].as_str() else {
        return Evidence { review, reads };
    };
    if !head_targets.is_empty() {
        let args = read_args(head_tree, &head_targets, encoding, "2900", "9216");
        reads.push(invoke(
            journey,
            ledger,
            "read pinned active application evidence",
            &args,
            CacheState::Warm,
        ));
    }
    let missing_imports = imports(&reads, &head_targets);
    if !missing_imports.is_empty() {
        // Reserve the final historical read inside the frozen complete episode envelope.
        let tokens = 7_500usize
            .saturating_sub(ledger.totals().response_tokens + 1_800)
            .min(1_500);
        let bytes = (24 * 1024usize)
            .saturating_sub(ledger.totals().response_bytes + 6_144)
            .min(4_096);
        if tokens >= 256 && bytes >= 1_024 {
            let args = read_args(
                head_tree,
                &missing_imports,
                encoding,
                &tokens.to_string(),
                &bytes.to_string(),
            );
            reads.push(invoke(
                journey,
                ledger,
                "follow exact imports supplied by active API source",
                &args,
                CacheState::Warm,
            ));
        }
    }
    // Complete comparison inventory plus the pinned head identity permits retention of unchanged
    // production fragments. Only changed historical bytes need another source delivery.
    let mut base_targets = base_changed;
    // A changed request check already supplied at head is not needed again at base.
    base_targets.retain(|path, _| {
        !reads.iter().any(|report| {
            items(report, "sources").iter().any(|chunk| {
                let file = items(report, "files")
                    .iter()
                    .find(|file| file["id"] == chunk["file"]);
                file.is_some_and(|file| file["path"].as_str() == Some(path))
                    && chunk["content"]
                        .as_str()
                        .is_some_and(|content| content.contains("\ndef test_"))
            })
        })
    });
    if let Some(base_tree) = review["comparison"]["base_tree"].as_str()
        && !base_targets.is_empty()
    {
        let args = read_args(base_tree, &base_targets, encoding, "1800", "6144");
        reads.push(invoke(
            journey,
            ledger,
            "confirm the original production bindings and policies",
            &args,
            CacheState::Warm,
        ));
    }
    Evidence { review, reads }
}

fn imports(reads: &[Value], known: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut missing_imports = BTreeMap::new();
    let delivered = source_paths(reads);
    for report in reads {
        for chunk in items(report, "sources") {
            let Some(content) = chunk["content"].as_str() else {
                continue;
            };
            let Some(file) = items(report, "files")
                .iter()
                .find(|file| file["id"] == chunk["file"])
            else {
                continue;
            };
            let Some(path) = file["path"].as_str() else {
                continue;
            };
            for module in content.lines().filter_map(|line| {
                line.strip_prefix("from ")?
                    .split_once(" import ")
                    .map(|(module, _)| module)
            }) {
                let Some(imported) = local_module(path, module) else {
                    continue;
                };
                if !delivered.contains(&imported) {
                    missing_imports.insert(
                        imported.clone(),
                        known.get(&imported).cloned().unwrap_or_default(),
                    );
                }
            }
        }
    }
    missing_imports
}

fn source_paths(reads: &[Value]) -> BTreeSet<String> {
    let mut delivered = BTreeSet::new();
    for report in reads {
        for chunk in items(report, "sources") {
            let file = items(report, "files")
                .iter()
                .find(|file| file["id"] == chunk["file"]);
            if let Some(path) = file.and_then(|file| file["path"].as_str()) {
                delivered.insert(path.to_owned());
            }
        }
    }
    delivered
}

fn local_module(importer: &str, module: &str) -> Option<String> {
    let relative = module.bytes().take_while(|byte| *byte == b'.').count();
    let mut parts: Vec<_> = if relative == 0 {
        Vec::new()
    } else {
        let mut parts: Vec<_> = importer.split('/').collect();
        for _ in 0..relative {
            parts.pop()?;
        }
        parts
    };
    for part in module[relative..].split('.') {
        if part.is_empty()
            || !part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return None;
        }
        parts.push(part);
    }
    Some(format!("{}.py", parts.join("/")))
}
