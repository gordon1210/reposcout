//! The adapter receives user words, numeric limits and public responses, never the oracle.

use super::super::support::{CacheState, CostLedger, Limits};
use crate::journeys::support::Journey;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(super) struct Capture {
    pub(super) file: Value,
    pub(super) source: Value,
    pub(super) symbol: Option<String>,
    /// A caller-retained quote is traceable to its earlier complete public chunk.
    pub(super) quote_origin: Option<Value>,
}

#[derive(Clone, Debug)]
pub(super) struct Retained {
    pub(super) capture: Capture,
    /// Keep original source identity. A public change record separately justifies reuse.
    pub(super) current_proof: Option<Value>,
}

#[derive(Default)]
pub(super) struct Investigation {
    pub(super) retained: BTreeMap<String, Retained>,
    pub(super) reports: Vec<Value>,
}

pub(super) struct Continuation {
    pub(super) retained: BTreeMap<String, Retained>,
    pub(super) change: Value,
    pub(super) deliveries: Vec<Value>,
    pub(super) stale: Vec<Value>,
    pub(super) reused_pieces: Vec<Retained>,
}

pub(super) struct PublicTask<'a> {
    pub(super) query: &'a str,
    pub(super) snapshot: &'a str,
    pub(super) encoding: &'a str,
    pub(super) limits: Limits,
}

struct Session<'a, 'fixture> {
    journey: &'a mut Journey<'fixture>,
    costs: &'a mut CostLedger,
    encoding: &'a str,
    limits: Limits,
    first_tokens: usize,
    first_bytes: usize,
}

struct RefreshTarget {
    path: String,
    previous: Capture,
    current_file: Value,
}

impl Session<'_, '_> {
    fn run(&mut self, label: &str, mut args: Vec<String>, cold: bool) -> Value {
        let consumed = self.costs.totals();
        let tokens = self
            .limits
            .response_tokens
            .saturating_sub(consumed.response_tokens - self.first_tokens);
        let bytes = self
            .limits
            .response_bytes
            .saturating_sub(consumed.response_bytes - self.first_bytes);
        assert!(
            tokens >= 256 && bytes >= 1024,
            "unmet next evidence obligation {label:?}: remaining phase budget ({tokens} tokens, {bytes} bytes) cannot admit another legal public query; previous responses remain accounted"
        );
        args.extend([
            "--encoding".to_owned(),
            self.encoding.to_owned(),
            "--budget".to_owned(),
            tokens.to_string(),
            "--max-output-bytes".to_owned(),
            bytes.to_string(),
            "--no-project-config".to_owned(),
            "-f".to_owned(),
            "json".to_owned(),
            "--quiet".to_owned(),
        ]);
        let step = self.costs.step(
            self.journey,
            label,
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            if cold {
                CacheState::Cold
            } else {
                CacheState::Warm
            },
        );
        step.assert_exit(0);
        step.stdout_json()
    }

    fn read(
        &mut self,
        path: &str,
        symbol: Option<&str>,
        hash: Option<&str>,
        snapshot: &str,
    ) -> Value {
        let mut args = vec![
            "read".to_owned(),
            ".".to_owned(),
            "--snapshot".to_owned(),
            snapshot.to_owned(),
        ];
        if let Some(symbol) = symbol {
            args.extend(["--symbol".to_owned(), path.to_owned(), symbol.to_owned()]);
        } else {
            args.extend(["--file".to_owned(), path.to_owned()]);
        }
        if let Some(hash) = hash {
            args.extend(["--expect-hash".to_owned(), path.to_owned(), hash.to_owned()]);
        }
        self.run("read only a publicly discovered source target", args, false)
    }
}

pub(super) fn initial(
    journey: &mut Journey<'_>,
    costs: &mut CostLedger,
    task: &PublicTask<'_>,
) -> Investigation {
    let first_tokens = costs.totals().response_tokens;
    let first_bytes = costs.totals().response_bytes;
    let mut session = Session {
        journey,
        costs,
        encoding: task.encoding,
        limits: task.limits,
        first_tokens,
        first_bytes,
    };
    let search = session.run(
        "find the user-described storage entrypoint",
        vec![
            "find".to_owned(),
            task.query.to_owned(),
            ".".to_owned(),
            "--limit".to_owned(),
            "3".to_owned(),
        ],
        true,
    );
    let mut state = Investigation::default();
    let Some(hit) = search["hits"].as_array().and_then(|hits| hits.first()) else {
        state.reports.push(search);
        return state;
    };
    let path = text(&hit["read"], "path").to_owned();
    let hash = text(&hit["read"], "expected_hash").to_owned();
    state.reports.push(search);
    let entry = session.read(&path, None, Some(&hash), task.snapshot);
    let entry_capture = capture(&entry);
    let policy = entry_capture.as_ref().and_then(import);
    retain(&mut state.retained, entry_capture);
    state.reports.push(entry);
    let Some((path, _)) = policy else {
        return state;
    };
    let report = session.read(&path, None, None, task.snapshot);
    let policy_capture = capture(&report);
    let helper = policy_capture.as_ref().and_then(import);
    retain(&mut state.retained, policy_capture);
    state.reports.push(report);
    let Some((path, symbol)) = helper else {
        return state;
    };
    let report = session.read(&path, Some(&symbol), None, task.snapshot);
    retain(&mut state.retained, capture(&report));
    state.reports.push(report);
    state
}

pub(super) fn follow_up(
    journey: &mut Journey<'_>,
    costs: &mut CostLedger,
    task: &PublicTask<'_>,
    previous: &Investigation,
) -> Continuation {
    let first_tokens = costs.totals().response_tokens;
    let first_bytes = costs.totals().response_bytes;
    let mut session = Session {
        journey,
        costs,
        encoding: task.encoding,
        limits: task.limits,
        first_tokens,
        first_bytes,
    };
    let report = session.run(
        "justify retained source through complete public working changes",
        vec!["changes".to_owned(), ".".to_owned(), "--working".to_owned()],
        false,
    );
    let mut result = Continuation {
        retained: previous.retained.clone(),
        change: report.clone(),
        deliveries: Vec::new(),
        stale: Vec::new(),
        reused_pieces: Vec::new(),
    };
    if !complete_changes(&report)
        || previous.retained.is_empty()
        || previous
            .retained
            .values()
            .any(|retained| retained.capture.file["snapshot"] != report["change"]["base"])
    {
        return result;
    }
    let refresh = retain_unchanged(&mut result, &report);
    for RefreshTarget {
        path,
        previous: old,
        current_file: current,
    } in refresh
    {
        result.retained.remove(&path);
        let stale = session.read(
            &path,
            old.symbol.as_deref(),
            Some(text(&old.file, "sha256")),
            "worktree",
        );
        assert!(
            stale["sources"].as_array().is_none_or(Vec::is_empty),
            "stale source must never be delivered"
        );
        assert!(
            stale["results"]
                .as_array()
                .is_some_and(|results| !results.is_empty()
                    && results.iter().all(|item| item["status"] == "stale")),
            "public stale-hash rejection missing: {stale}"
        );
        result.stale.push(stale);
        let delivered = session.read(
            &path,
            old.symbol.as_deref(),
            Some(text(&current, "sha256")),
            "worktree",
        );
        let fresh = capture(&delivered);
        let new_import = fresh
            .as_ref()
            .and_then(import)
            .filter(|(target, _)| !result.retained.contains_key(target));
        retain(&mut result.retained, fresh);
        result.deliveries.push(delivered);
        if let Some((path, symbol)) = new_import {
            let delivered = session.read(&path, Some(&symbol), None, "worktree");
            retain(&mut result.retained, capture(&delivered));
            result.deliveries.push(delivered);
        }
    }
    result
}

fn retain_unchanged(result: &mut Continuation, report: &Value) -> Vec<RefreshTarget> {
    let changed: BTreeMap<String, Value> = report["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|file| file["snapshot"]["kind"] == "worktree")
        .map(|file| (text(file, "path").to_owned(), file.clone()))
        .collect();
    let mut refresh = Vec::new();
    for (path, retained) in &mut result.retained {
        let Some(current) = changed.get(path) else {
            retained.current_proof = Some(report.clone());
            continue;
        };
        let same_base = report["files"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|file| {
                file["path"] == *path
                    && file["snapshot"] == report["change"]["base"]
                    && file["sha256"] == retained.capture.file["sha256"]
            });
        let unchanged_fragment = same_base
            && ["base", "current"].into_iter().all(|side| {
                let ranges = changed_ranges(report, path, side);
                !ranges.is_empty()
                    && ranges
                        .iter()
                        .all(|range| disjoint(range, &retained.capture.source["span"]))
            });
        if unchanged_fragment {
            retained.current_proof = Some(report.clone());
        } else {
            if let Some(quote) = after_binding_quote(&retained.capture) {
                let quote_unchanged = same_base
                    && ["base", "current"].into_iter().all(|side| {
                        let ranges = changed_ranges(report, path, side);
                        !ranges.is_empty()
                            && ranges
                                .iter()
                                .all(|range| disjoint(range, &quote.source["span"]))
                    });
                if quote_unchanged {
                    result.reused_pieces.push(Retained {
                        capture: quote,
                        current_proof: Some(report.clone()),
                    });
                }
            }
            refresh.push(RefreshTarget {
                path: path.clone(),
                previous: retained.capture.clone(),
                current_file: current.clone(),
            });
        }
    }
    refresh
}

fn retain(retained: &mut BTreeMap<String, Retained>, value: Option<Capture>) {
    if let Some(capture) = value {
        retained.insert(
            text(&capture.file, "path").to_owned(),
            Retained {
                capture,
                current_proof: None,
            },
        );
    }
}

fn import(captured: &Capture) -> Option<(String, String)> {
    text(&captured.source, "content").lines().find_map(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 4 || fields[0] != "from" || fields[2] != "import" {
            return None;
        }
        Some((
            format!("{}.py", fields[1].replace('.', "/")),
            fields[3].to_owned(),
        ))
    })
}

fn capture(report: &Value) -> Option<Capture> {
    let result = report["results"]
        .as_array()?
        .iter()
        .find(|result| result["status"] == "complete")?;
    let file = report["files"]
        .as_array()?
        .iter()
        .find(|file| file["id"] == result["file"])?;
    let source = report["sources"]
        .as_array()?
        .iter()
        .find(|source| source["id"] == result["source"] && source["file"] == file["id"])?;
    Some(Capture {
        file: file.clone(),
        source: source.clone(),
        symbol: result["definition"]["name"].as_str().map(str::to_owned),
        quote_origin: None,
    })
}

fn after_binding_quote(captured: &Capture) -> Option<Capture> {
    let content = captured.source["content"].as_str()?;
    let prefix_end = content.find("\n\n")? + 2;
    if !content[..prefix_end]
        .lines()
        .all(|line| line.is_empty() || line.starts_with("from "))
    {
        return None;
    }
    let mut quote = captured.clone();
    quote.quote_origin = Some(captured.source.clone());
    quote.source["content"] = Value::String(content[prefix_end..].to_owned());
    quote.source["span"]["start_byte"] =
        Value::from(captured.source["span"]["start_byte"].as_u64()? + prefix_end as u64);
    quote.source["span"]["start_line"] = Value::from(
        captured.source["span"]["start_line"].as_u64()?
            + content[..prefix_end]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count() as u64,
    );
    Some(quote)
}

pub(super) fn complete_changes(report: &Value) -> bool {
    let change = &report["change"];
    if report["kind"] != "change_query"
        || change["scope"] != "working"
        || change["current"]["kind"] != "worktree"
        || report["omitted_targets"] != 0
        || change["total_files"] != change["processed_files"]
    {
        return false;
    }
    for key in [
        "omitted_files",
        "unavailable_sides",
        "omitted_hunks",
        "unprocessed_ranges",
    ] {
        if change[key] != 0 {
            return false;
        }
    }
    let paths: BTreeSet<_> = report["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|file| file["snapshot"]["kind"] == "worktree")
        .filter_map(|file| file["path"].as_str())
        .collect();
    if change["processed_files"].as_u64() != Some(paths.len() as u64) {
        return false;
    }
    report["results"].as_array().is_some_and(|results| {
        results.iter().all(|item| {
            matches!(item["status"].as_str(), Some("changed" | "unmapped"))
                && item["change"]["ambiguous"] != true
        })
    })
}

pub(super) fn changed_ranges(report: &Value, path: &str, side: &str) -> Vec<Value> {
    let files = report["files"].as_array().map_or(&[][..], Vec::as_slice);
    report["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|result| {
            result["change"]["side"] == side
                && files
                    .iter()
                    .any(|file| file["id"] == result["file"] && file["path"] == path)
        })
        .flat_map(|result| {
            result["change"]["ranges"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(
                    result["change"]["wrapper_ranges"]
                        .as_array()
                        .into_iter()
                        .flatten(),
                )
        })
        .cloned()
        .collect()
}

pub(super) fn disjoint(range: &Value, span: &Value) -> bool {
    match (
        range["start"].as_u64(),
        range["end"].as_u64(),
        span["start_line"].as_u64(),
        span["end_line"].as_u64(),
    ) {
        (Some(start), Some(end), Some(first), Some(last)) => end < first || start > last,
        _ => false,
    }
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("missing public string {key}: {value}"))
}
