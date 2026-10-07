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
    pub(super) retained: BTreeMap<String, Vec<Retained>>,
    pub(super) reports: Vec<Value>,
}

pub(super) struct Continuation {
    pub(super) retained: BTreeMap<String, Vec<Retained>>,
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
    ranges: Vec<(u64, u64)>,
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
        self.read_selected(path, symbol, &[], hash, snapshot)
    }

    fn read_selected(
        &mut self,
        path: &str,
        symbol: Option<&str>,
        ranges: &[(u64, u64)],
        hash: Option<&str>,
        snapshot: &str,
    ) -> Value {
        let mut args = vec![
            "read".to_owned(),
            ".".to_owned(),
            "--snapshot".to_owned(),
            snapshot.to_owned(),
        ];
        if !ranges.is_empty() {
            for (start, end) in ranges {
                args.extend([
                    "--range".to_owned(),
                    path.to_owned(),
                    start.to_string(),
                    end.to_string(),
                ]);
            }
        } else if let Some(symbol) = symbol {
            args.extend(["--symbol".to_owned(), path.to_owned(), symbol.to_owned()]);
        } else {
            args.extend(["--file".to_owned(), path.to_owned()]);
        }
        if let Some(hash) = hash {
            args.extend(["--expect-hash".to_owned(), path.to_owned(), hash.to_owned()]);
        }
        self.run("read only a publicly discovered source target", args, false)
    }

    fn read_imports(
        &mut self,
        targets: &[(String, String)],
        snapshot: &str,
        definitions: bool,
    ) -> Value {
        let mut args = vec![
            "read".to_owned(),
            ".".to_owned(),
            "--snapshot".to_owned(),
            snapshot.to_owned(),
        ];
        if definitions {
            for (path, symbol) in targets {
                args.extend(["--symbol".to_owned(), path.clone(), symbol.clone()]);
            }
        } else {
            for path in targets
                .iter()
                .map(|(path, _)| path)
                .collect::<BTreeSet<_>>()
            {
                args.extend(["--file".to_owned(), path.clone()]);
            }
        }
        self.run(
            "read targets from all delivered active bindings",
            args,
            false,
        )
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
    let entry_captures = captures(&entry);
    let policies = imported_targets(&entry_captures);
    retain(&mut state.retained, entry_captures);
    state.reports.push(entry);
    if policies.is_empty() {
        return state;
    }
    let report = session.read_imports(&policies, task.snapshot, false);
    let policy_captures = captures(&report);
    let helpers = imported_targets(&policy_captures);
    retain(&mut state.retained, policy_captures);
    state.reports.push(report);
    if helpers.is_empty() {
        return state;
    }
    let report = session.read_imports(&helpers, task.snapshot, true);
    retain(&mut state.retained, captures(&report));
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
            .flatten()
            .any(|retained| retained.capture.file["snapshot"] != report["change"]["base"])
    {
        return result;
    }
    let refresh = retain_unchanged(&mut result, &report);
    for RefreshTarget {
        path,
        previous: old,
        current_file: current,
        ranges,
    } in refresh
    {
        let stale = session.read_selected(
            &path,
            old.symbol.as_deref(),
            &ranges,
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
        let delivered = session.read_selected(
            &path,
            old.symbol.as_deref(),
            &ranges,
            Some(text(&current, "sha256")),
            "worktree",
        );
        let fresh = captures(&delivered);
        let new_imports: Vec<_> = imported_targets(&fresh)
            .into_iter()
            .filter(|(path, symbol)| {
                result.retained.get(path).is_none_or(|fragments| {
                    !fragments
                        .iter()
                        .any(|fragment| fragment.capture.symbol.as_deref() == Some(symbol.as_str()))
                })
            })
            .collect();
        retain(&mut result.retained, fresh);
        result.deliveries.push(delivered);
        if !new_imports.is_empty() {
            let delivered = session.read_imports(&new_imports, "worktree", true);
            retain(&mut result.retained, captures(&delivered));
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
    for (path, fragments) in &mut result.retained {
        let Some(current) = changed.get(path) else {
            for retained in fragments {
                retained.current_proof = Some(report.clone());
            }
            continue;
        };
        for mut retained in std::mem::take(fragments) {
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
                fragments.push(retained);
                continue;
            }
            let quotes = if same_base && retained.capture.symbol.is_none() {
                unchanged_quotes(&retained.capture, report, path)
            } else {
                Vec::new()
            };
            let ranges = if quotes.is_empty() {
                Vec::new()
            } else {
                changed_ranges(report, path, "current")
                    .iter()
                    .filter_map(|range| range["start"].as_u64().zip(range["end"].as_u64()))
                    .filter(|(start, end)| *start > 0 && end >= start)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            };
            for quote in quotes {
                result.reused_pieces.push(Retained {
                    capture: quote,
                    current_proof: Some(report.clone()),
                });
            }
            refresh.push(RefreshTarget {
                path: path.clone(),
                previous: retained.capture,
                current_file: current.clone(),
                ranges,
            });
        }
    }
    refresh
}

fn retain(retained: &mut BTreeMap<String, Vec<Retained>>, values: Vec<Capture>) {
    for capture in values {
        let fragments = retained
            .entry(text(&capture.file, "path").to_owned())
            .or_default();
        if !fragments.iter().any(|previous| {
            previous.capture.file == capture.file && previous.capture.source == capture.source
        }) {
            fragments.push(Retained {
                capture,
                current_proof: None,
            });
        }
    }
}

fn imported_targets(captured: &[Capture]) -> Vec<(String, String)> {
    captured
        .iter()
        .flat_map(|captured| text(&captured.source, "content").lines())
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() != 4 || fields[0] != "from" || fields[2] != "import" {
                return None;
            }
            Some((
                format!("{}.py", fields[1].replace('.', "/")),
                fields[3].to_owned(),
            ))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn captures(report: &Value) -> Vec<Capture> {
    let mut seen = BTreeSet::new();
    report["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|result| result["status"] == "complete")
        .filter_map(|result| {
            let file = report["files"]
                .as_array()?
                .iter()
                .find(|file| file["id"] == result["file"])?;
            let source = report["sources"]
                .as_array()?
                .iter()
                .find(|source| source["id"] == result["source"] && source["file"] == file["id"])?;
            seen.insert((file["id"].as_u64()?, source["id"].as_u64()?))
                .then(|| Capture {
                    file: file.clone(),
                    source: source.clone(),
                    symbol: result["definition"]["name"].as_str().map(str::to_owned),
                    quote_origin: None,
                })
        })
        .collect()
}

fn unchanged_quotes(captured: &Capture, report: &Value, path: &str) -> Vec<Capture> {
    let mut changes = Vec::new();
    for side in ["base", "current"] {
        let ranges = changed_ranges(report, path, side);
        if ranges.is_empty()
            || ranges
                .iter()
                .any(|range| range["start"].as_u64().is_none() || range["end"].as_u64().is_none())
        {
            return Vec::new();
        }
        changes.extend(ranges);
    }
    let content = text(&captured.source, "content");
    let first = captured.source["span"]["start_line"].as_u64().unwrap();
    let mut offset = 0;
    let mut start = None;
    let mut quotes = Vec::new();
    for (index, line) in content.split_inclusive('\n').enumerate() {
        let number = first + index as u64;
        let intersects = changes.iter().any(|range| {
            range["start"].as_u64().unwrap() <= number && range["end"].as_u64().unwrap() >= number
        });
        if intersects {
            if let Some((begin, begin_line)) = start.take() {
                quote(captured, begin, offset, begin_line, number - 1, &mut quotes);
            }
        } else if start.is_none() {
            start = Some((offset, number));
        }
        offset += line.len();
    }
    if let Some((begin, begin_line)) = start {
        let last = captured.source["span"]["end_line"].as_u64().unwrap();
        quote(captured, begin, offset, begin_line, last, &mut quotes);
    }
    quotes
}

fn quote(
    captured: &Capture,
    start: usize,
    end: usize,
    first_line: u64,
    last_line: u64,
    quotes: &mut Vec<Capture>,
) {
    let content = text(&captured.source, "content");
    if content[start..end].trim().is_empty() {
        return;
    }
    let base = captured.source["span"]["start_byte"].as_u64().unwrap();
    let mut quote = captured.clone();
    quote.quote_origin = Some(captured.source.clone());
    quote.source["content"] = Value::String(content[start..end].to_owned());
    quote.source["span"]["start_byte"] = Value::from(base + start as u64);
    quote.source["span"]["end_byte"] = Value::from(base + end as u64);
    quote.source["span"]["start_line"] = Value::from(first_line);
    quote.source["span"]["end_line"] = Value::from(last_line);
    quotes.push(quote);
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
