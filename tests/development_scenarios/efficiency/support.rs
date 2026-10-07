use crate::journeys::support::{Journey, Step};
use crate::support::Fixture;
use reposcout::metrics::tokens::TokenCounter;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

/// Literal necessary source, authored independently before executing the CLI.
pub(super) struct EvidenceFragment<'source> {
    pub path: &'source str,
    pub source: &'source str,
}

#[derive(Clone, Serialize)]
pub(super) struct PacketCost {
    pub encoding: String,
    pub tokens: usize,
    pub escaped_source_bytes: usize,
    pub nonblank_lines: usize,
    pub paths: BTreeSet<String>,
}

impl PacketCost {
    pub(super) fn new(encoding: &str, fragments: &[EvidenceFragment<'_>]) -> Self {
        let counter = TokenCounter::new(encoding).expect("explicit supported encoding");
        Self {
            encoding: counter.name().to_owned(),
            tokens: fragments
                .iter()
                .map(|part| counter.count(part.source))
                .sum(),
            escaped_source_bytes: fragments
                .iter()
                .map(|part| serde_json::to_vec(part.source).unwrap().len())
                .sum(),
            nonblank_lines: fragments
                .iter()
                .map(|part| nonblank_lines(part.source))
                .sum(),
            paths: fragments.iter().map(|part| part.path.to_owned()).collect(),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
pub(super) struct Limits {
    pub calls: usize,
    pub response_bytes: usize,
    pub response_tokens: usize,
    pub source_paths: usize,
    pub source_nonblank_lines: usize,
    pub source_tokens: Option<usize>,
    pub body_nonblank_lines: Option<usize>,
}

#[derive(Serialize)]
pub(super) struct PhaseBudget {
    pub label: &'static str,
    pub limits: Limits,
    pub packet: PacketCost,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CacheState {
    Cold,
    Warm,
}

#[derive(Clone, Default, Serialize)]
pub(super) struct Metrics {
    pub calls: usize,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub response_bytes: usize,
    pub response_tokens: usize,
    pub argv_bytes: usize,
    pub argv_tokens: usize,
    pub interaction_bytes: usize,
    pub interaction_tokens: usize,
    pub source_paths: BTreeSet<String>,
    pub source_nonblank_lines: usize,
    pub source_tokens: usize,
    pub body_paths: BTreeSet<String>,
    pub body_nonblank_lines: usize,
    pub body_tokens: usize,
    pub wall_seconds: f64,
    /// Measured by the external resource guard when available; never guessed from elapsed time.
    pub process_tree_peak_rss_bytes: Option<u64>,
}

#[derive(Clone, Serialize)]
pub(super) struct Assessment {
    pub label: String,
    pub metrics: Metrics,
    pub excesses: Vec<String>,
    pub accounting_gaps: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum SourceKind {
    Body,
    Patch,
    Signature,
}

#[derive(Serialize)]
struct Occurrence {
    pointer: String,
    paths: BTreeSet<String>,
    kind: SourceKind,
    nonblank_lines: usize,
    tokens: usize,
}

#[derive(Serialize)]
struct CommandCost {
    label: String,
    phase: usize,
    argv: Vec<String>,
    cache_state: CacheState,
    exit: Option<i32>,
    completed: bool,
    transcript: Option<PathBuf>,
    metrics: Metrics,
    occurrences: Vec<Occurrence>,
    accounting_gaps: Vec<String>,
}

impl CommandCost {
    fn pending(
        counter: &TokenCounter,
        label: &str,
        phase: usize,
        args: &[&str],
        cache_state: CacheState,
    ) -> Self {
        let serialized = serde_json::to_string(args).unwrap();
        Self {
            label: label.to_owned(),
            phase,
            argv: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cache_state,
            exit: None,
            completed: false,
            transcript: None,
            metrics: Metrics {
                calls: 1,
                argv_bytes: serialized.len(),
                argv_tokens: counter.count(&serialized),
                ..Metrics::default()
            },
            occurrences: Vec::new(),
            accounting_gaps: vec!["command has not returned a process outcome".to_owned()],
        }
    }

    fn capture(
        &mut self,
        counter: &TokenCounter,
        stdout: &[u8],
        stderr: &[u8],
        exit: Option<i32>,
        elapsed_seconds: f64,
    ) {
        self.exit = exit;
        self.completed = true;
        self.accounting_gaps.clear();
        self.metrics.stdout_bytes = stdout.len();
        self.metrics.stderr_bytes = stderr.len();
        self.metrics.response_bytes = stdout.len() + stderr.len();
        self.metrics.response_tokens = counter.count(&String::from_utf8_lossy(stdout))
            + counter.count(&String::from_utf8_lossy(stderr));
        self.metrics.wall_seconds = elapsed_seconds;
        collect_stream(counter, self, "stdout", stdout);
        collect_stream(counter, self, "stderr", stderr);
        self.metrics.interaction_bytes = self.metrics.response_bytes + self.metrics.argv_bytes;
        self.metrics.interaction_tokens = self.metrics.response_tokens + self.metrics.argv_tokens;
    }
}

#[derive(Serialize)]
struct LedgerReport {
    label: String,
    encoding: String,
    phases: Vec<PhaseBudget>,
    episode_limits: Limits,
    commands: Vec<CommandCost>,
    checkpoints: Vec<Assessment>,
    metrics: Metrics,
    episode: Option<Assessment>,
}

/// Accounting around the existing public CLI journey, with no fixture or source oracle access.
pub(super) struct CostLedger {
    counter: TokenCounter,
    report: LedgerReport,
    phase: usize,
    artifact: PathBuf,
}

impl CostLedger {
    pub(super) fn new(
        fixture: &Fixture,
        label: &str,
        encoding: &str,
        phases: Vec<PhaseBudget>,
        episode_limits: Limits,
    ) -> Self {
        assert!(
            !phases.is_empty(),
            "an episode has at least one frozen phase"
        );
        let counter = TokenCounter::new(encoding).expect("explicit supported encoding");
        assert!(
            phases
                .iter()
                .all(|phase| phase.packet.encoding == counter.name())
        );
        let directory = tempfile::Builder::new()
            .prefix("efficiency-ledger-")
            .tempdir_in(fixture.state_path())
            .expect("private cost artifact directory")
            .keep();
        let report = LedgerReport {
            label: label.to_owned(),
            encoding: counter.name().to_owned(),
            phases,
            episode_limits,
            commands: Vec::new(),
            checkpoints: Vec::new(),
            metrics: Metrics::default(),
            episode: None,
        };
        fs::write(
            directory.join("frozen-budgets.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .expect("freeze packet cost metadata and limits before the first CLI invocation");
        let ledger = Self {
            counter,
            report,
            phase: 0,
            artifact: directory.join("ledger.json"),
        };
        ledger.persist();
        eprintln!("[efficiency] cost ledger: {}", ledger.artifact.display());
        ledger
    }

    pub(super) fn step(
        &mut self,
        journey: &mut Journey<'_>,
        label: &str,
        args: &[&str],
        cache_state: CacheState,
    ) -> Step {
        assert!(
            self.phase < self.report.phases.len(),
            "no frozen phase remains"
        );
        // A process-launch failure must still leave its invocation/argv in the artifact.
        self.report.commands.push(CommandCost::pending(
            &self.counter,
            label,
            self.phase,
            args,
            cache_state,
        ));
        self.refresh_totals();
        self.persist();
        let step = journey.observe(label, args);
        let command = self.report.commands.last_mut().unwrap();
        command.transcript = Some(step.transcript_prefix().to_path_buf());
        command.capture(
            &self.counter,
            step.stdout_bytes(),
            step.stderr_bytes(),
            step.exit_code(),
            step.elapsed_seconds(),
        );
        self.refresh_totals();
        self.persist();
        step
    }

    pub(super) fn checkpoint(&mut self) -> Assessment {
        let budget = &self.report.phases[self.phase];
        let assessment = assess(
            budget.label,
            budget.limits,
            self.report
                .commands
                .iter()
                .filter(|command| command.phase == self.phase),
        );
        self.report.checkpoints.push(assessment.clone());
        self.phase += 1;
        self.persist();
        eprintln!(
            "[efficiency checkpoint] {}",
            serde_json::to_string(&assessment).unwrap()
        );
        assessment
    }

    pub(super) fn finish(&mut self) -> Assessment {
        let assessment = assess(
            &self.report.label,
            self.report.episode_limits,
            self.report.commands.iter(),
        );
        self.report.episode = Some(assessment.clone());
        self.persist();
        eprintln!(
            "[efficiency result] {}",
            serde_json::to_string(&assessment).unwrap()
        );
        assessment
    }

    pub(super) fn totals(&self) -> &Metrics {
        &self.report.metrics
    }

    fn refresh_totals(&mut self) {
        self.report.metrics = sum_metrics(self.report.commands.iter());
    }

    fn persist(&self) {
        fs::write(
            &self.artifact,
            serde_json::to_vec_pretty(&self.report).unwrap(),
        )
        .expect("retain complete cost metrics before driver parsing or criterion assertions");
    }
}

#[test]
#[ignore = "opt-in efficiency accounting contract"]
fn raw_error_retry_and_repeated_source_all_consume_cost() {
    let counter = TokenCounter::new("o200k_base").unwrap();
    let args = ["read", ".", "--encoding", "o200k_base", "-f", "json"];
    let source = "def payable():\n\n    return 904\n";
    let stdout = serde_json::to_vec(&serde_json::json!({
        "files": [{"id": 1, "path": "invoices.py"}],
        "results": [
            {"file": 1, "definition": {"signature": "def payable(): …"}, "source": 1},
            {"file": 1, "definition": {"signature": "def payable(): …"}, "source": 1}
        ],
        "sources": [{"id": 1, "file": 1, "span": {}, "content": source}],
        "duplicates": [{"fragment_a": {
            "path": "copied.py", "start_line": 1, "end_line": 3,
            "start_byte": 0, "end_byte": source.len(), "snippet": source
        }}],
        "metadata": {"path": "ignored.py", "snippet": "display hint"}
    }))
    .unwrap();
    let stderr = b"{\"error\":\"temporary failure\"}\n";
    let mut failed = CommandCost::pending(&counter, "failed", 0, &args, CacheState::Cold);
    failed.capture(&counter, &stdout, stderr, Some(2), 0.5);
    let mut retry = CommandCost::pending(&counter, "retry", 0, &args, CacheState::Warm);
    retry.capture(&counter, &stdout, b"", Some(0), 0.25);
    let total = sum_metrics([&failed, &retry].into_iter());
    let serialized = serde_json::to_string(&args).unwrap();
    assert_eq!(total.calls, 2);
    assert_eq!(total.response_bytes, stdout.len() * 2 + stderr.len());
    assert_eq!(
        total.response_tokens,
        counter.count(std::str::from_utf8(&stdout).unwrap()) * 2
            + counter.count(std::str::from_utf8(stderr).unwrap())
    );
    assert_eq!(total.argv_bytes, serialized.len() * 2);
    assert_eq!(total.argv_tokens, counter.count(&serialized) * 2);
    assert_eq!(
        total.interaction_bytes,
        total.response_bytes + total.argv_bytes
    );
    assert_eq!(total.source_nonblank_lines, 12);
    assert_eq!(total.body_nonblank_lines, 8);
    assert_eq!(
        total.source_tokens,
        2 * (2 * counter.count(source) + 2 * counter.count("def payable(): …"))
    );
    assert_eq!(
        failed.occurrences.len(),
        4,
        "shared body once, each emitted signature and duplicate snippet separately"
    );
    assert_eq!(
        total.source_paths,
        BTreeSet::from(["invoices.py".to_owned(), "copied.py".to_owned()])
    );
    assert_eq!(failed.accounting_gaps, Vec::<String>::new());
}

#[test]
#[ignore = "opt-in efficiency accounting contract"]
fn public_review_diffs_count_once_and_retain_both_rename_paths() {
    let counter = TokenCounter::new("o200k_base").unwrap();
    let diffs = [
        "--- a/policy.py\n+++ b/policy.py\n@@ -1 +1 @@\n-return 499\n+return 299\n",
        "--- /dev/null\n+++ b/new.py\n@@ -0,0 +1 @@\n+return 499\n",
        "--- a/deleted.py\n+++ /dev/null\n@@ -1 +0,0 @@\n-return 499\n",
        "--- a/old.py\n+++ b/renamed.py\n@@ -1 +1 @@\n-return 499\n+return 299\n",
    ];
    let stdout = serde_json::to_vec(&serde_json::json!({
        "kind": "review-context",
        "comparison": {"base_commit": "abc", "head_commit": "def", "diff": "abc..def"},
        "changes": [
            {"status": "modified", "base": {"path": "policy.py", "definitions": [{"symbol": {"signature": "def quote(): …"}}]}, "head": {"path": "policy.py", "definitions": [{"symbol": {"signature": "def quote(): …"}}]}, "diff_tokens": 30, "diff": diffs[0]},
            {"status": "added", "base": null, "head": {"path": "new.py"}, "diff_tokens": 20, "diff": diffs[1]},
            {"status": "deleted", "base": {"path": "deleted.py"}, "head": null, "diff_tokens": 20, "diff": diffs[2]},
            {"status": "renamed", "base": {"path": "old.py"}, "head": {"path": "renamed.py"}, "diff_tokens": 30, "diff": diffs[3]}
        ]
    })).unwrap();
    let stderr = b"warning: metadata {\"diff\":\"abc..def\"}\n";
    let mut command = CommandCost::pending(
        &counter,
        "review diffs",
        0,
        &["review-context", "--diff"],
        CacheState::Cold,
    );
    command.capture(&counter, &stdout, stderr, Some(0), 0.0);
    assert_eq!(command.accounting_gaps, Vec::<String>::new());
    assert_eq!(command.occurrences.len(), 6);
    assert_eq!(
        command.metrics.body_nonblank_lines,
        diffs.iter().map(|text| nonblank_lines(text)).sum::<usize>()
    );
    assert_eq!(
        command.metrics.body_tokens,
        diffs.iter().map(|text| counter.count(text)).sum::<usize>()
    );
    assert_eq!(
        command.metrics.source_nonblank_lines,
        command.metrics.body_nonblank_lines + 2
    );
    assert_eq!(
        command.metrics.source_paths,
        BTreeSet::from(
            ["policy.py", "new.py", "deleted.py", "old.py", "renamed.py"].map(str::to_owned)
        )
    );
    let renamed = command
        .occurrences
        .iter()
        .find(|occurrence| occurrence.pointer.ends_with("/changes/3/diff"))
        .unwrap();
    assert_eq!(
        renamed.paths,
        BTreeSet::from(["old.py".to_owned(), "renamed.py".to_owned()])
    );
    assert_eq!(command.metrics.stderr_bytes, stderr.len());
    assert_eq!(command.metrics.response_bytes, stdout.len() + stderr.len());
}

#[test]
#[ignore = "opt-in efficiency accounting contract"]
fn plan_source_uses_its_own_file_ids_and_never_inherits_missing_id_paths() {
    let counter = TokenCounter::new("cl100k_base").unwrap();
    let source = "def quote():\n    return 499\n";
    let nested = |files| {
        serde_json::json!({
            "kind": "definition-plan", "files": [{"id": 1, "path": "outer.py"}],
            "selected": [{"file": 1, "name": "quote"}],
            "source": {
                "kind": "source-query", "files": files,
                "results": [{"file": 1, "definition": {"signature": "def quote(): …"}, "source": 1}],
                "sources": [{"id": 1, "file": 1, "span": {}, "content": source}]
            }
        })
    };
    let mut valid = CommandCost::pending(
        &counter,
        "plan source",
        0,
        &["plan", "--source"],
        CacheState::Cold,
    );
    valid.capture(
        &counter,
        &serde_json::to_vec(&nested(serde_json::json!([{"id": 1, "path": "inner.py"}]))).unwrap(),
        b"",
        Some(0),
        0.0,
    );
    assert_eq!(valid.accounting_gaps, Vec::<String>::new());
    assert_eq!(
        valid.metrics.source_paths,
        BTreeSet::from(["inner.py".to_owned()])
    );
    assert_eq!(valid.metrics.source_nonblank_lines, 3);
    assert_eq!(valid.metrics.body_nonblank_lines, 2);
    let mut missing = CommandCost::pending(
        &counter,
        "missing nested file identity",
        0,
        &["plan", "--source"],
        CacheState::Cold,
    );
    missing.capture(
        &counter,
        &serde_json::to_vec(&nested(serde_json::json!([]))).unwrap(),
        b"",
        Some(0),
        0.0,
    );
    assert_eq!(missing.accounting_gaps.len(), 2);
    assert!(missing.metrics.source_paths.is_empty());
    assert_eq!(
        missing.metrics.source_nonblank_lines,
        valid.metrics.source_nonblank_lines
    );
    assert_eq!(missing.metrics.source_tokens, valid.metrics.source_tokens);
}

#[test]
#[ignore = "opt-in efficiency accounting contract"]
fn failed_phase_checkpoints_preserve_episode_and_serialized_metrics() {
    let fixture = Fixture::new("efficiency-ledger-checkpoints");
    let limits = Limits {
        calls: 1,
        response_bytes: 0,
        response_tokens: 0,
        source_paths: 1,
        source_nonblank_lines: 16,
        source_tokens: None,
        body_nonblank_lines: Some(0),
    };
    let phase = |label| PhaseBudget {
        label,
        limits,
        packet: PacketCost::new("o200k_base", &[]),
    };
    let mut ledger = CostLedger::new(
        &fixture,
        "checkpoints",
        "o200k_base",
        vec![phase("initial"), phase("follow-up")],
        Limits { calls: 2, ..limits },
    );
    let packet = PacketCost::new(
        "o200k_base",
        &[EvidenceFragment {
            path: "policy.py",
            source: "return 499\n",
        }],
    );
    assert_eq!(packet.tokens, ledger.counter.count("return 499\n"));
    assert_eq!(
        packet.escaped_source_bytes,
        serde_json::to_vec("return 499\n").unwrap().len()
    );
    for index in 0..2 {
        let mut command = CommandCost::pending(
            &ledger.counter,
            "observed error",
            index,
            &["read", "."],
            CacheState::Warm,
        );
        command.capture(
            &ledger.counter,
            b"",
            b"{\"error\":\"stale\"}\n",
            Some(1),
            0.0,
        );
        ledger.report.commands.push(command);
        ledger.refresh_totals();
        let phase = ledger.checkpoint();
        assert_eq!(phase.metrics.calls, 1);
        assert_eq!(phase.excesses.len(), 2);
        assert_eq!(phase.accounting_gaps, Vec::<String>::new());
    }
    let episode = ledger.finish();
    assert_eq!(episode.metrics.calls, 2);
    let artifact: Value = serde_json::from_slice(&fs::read(&ledger.artifact).unwrap()).unwrap();
    assert_eq!(artifact["checkpoints"].as_array().unwrap().len(), 2);
    assert_eq!(artifact["episode"]["metrics"]["calls"], 2);
    assert_eq!(artifact["commands"][0]["exit"], 1);
    assert_eq!(artifact["commands"][1]["exit"], 1);
    assert_eq!(artifact["phases"][1]["packet"]["tokens"], 0);
    assert_eq!(artifact["phases"][1]["packet"]["nonblank_lines"], 0);
}

fn nonblank_lines(text: &str) -> usize {
    text.lines().filter(|line| !line.trim().is_empty()).count()
}

fn collect_stream(counter: &TokenCounter, command: &mut CommandCost, stream: &str, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if std::str::from_utf8(bytes).is_err() {
        command.accounting_gaps.push(format!(
            "{stream} is not UTF-8; token counts use replacement characters"
        ));
    }
    for (index, record) in serde_json::Deserializer::from_slice(bytes)
        .into_iter::<Value>()
        .enumerate()
    {
        match record {
            Ok(value) => collect_value(
                counter,
                command,
                &value,
                &format!("/{stream}/{index}"),
                None,
                &BTreeMap::new(),
            ),
            Err(error) => {
                // Plain stderr diagnostics still consume their complete raw response cost.
                if stream == "stdout" {
                    command.accounting_gaps.push(format!(
                        "source accounting needs JSON/NDJSON stdout: {error}"
                    ));
                }
                break;
            }
        }
    }
}

fn collect_value(
    counter: &TokenCounter,
    command: &mut CommandCost,
    value: &Value,
    pointer: &str,
    inherited_path: Option<&str>,
    inherited_files: &BTreeMap<u64, String>,
) {
    match value {
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_value(
                    counter,
                    command,
                    item,
                    &format!("{pointer}/{index}"),
                    inherited_path,
                    inherited_files,
                );
            }
        }
        Value::Object(object) => {
            let files = object.get("files").and_then(Value::as_array).map_or_else(
                || inherited_files.clone(),
                |entries| {
                    entries
                        .iter()
                        .filter_map(|file| {
                            Some((file["id"].as_u64()?, file["path"].as_str()?.to_owned()))
                        })
                        .collect()
                },
            );
            let path = object
                .get("path")
                .and_then(Value::as_str)
                .or_else(|| {
                    object
                        .get("file")
                        .and_then(Value::as_u64)
                        .and_then(|id| files.get(&id).map(String::as_str))
                })
                .or(inherited_path);
            for (key, child) in object {
                let child_pointer = format!("{pointer}/{key}");
                if let (Some(text), Some(kind)) = (child.as_str(), source_kind(object, key)) {
                    let paths: Vec<_> = if matches!(kind, SourceKind::Patch) {
                        ["base", "head"]
                            .into_iter()
                            .filter_map(|side| object.get(side)?.get("path")?.as_str())
                            .collect()
                    } else {
                        path.into_iter().collect()
                    };
                    count_occurrence(counter, command, &child_pointer, &paths, text, kind);
                } else {
                    collect_value(counter, command, child, &child_pointer, path, &files);
                }
            }
        }
        _ => {}
    }
}

fn source_kind(object: &Map<String, Value>, key: &str) -> Option<SourceKind> {
    match key {
        "signature" => Some(SourceKind::Signature),
        "diff"
            if object.contains_key("diff_tokens")
                && ["base", "head"].into_iter().any(|side| {
                    object
                        .get(side)
                        .and_then(|value| value.get("path"))
                        .and_then(Value::as_str)
                        .is_some()
                }) =>
        {
            Some(SourceKind::Patch)
        }
        "content" if object.contains_key("file") && object.contains_key("span") => {
            Some(SourceKind::Body)
        }
        "snippet"
            if object.get("path").and_then(Value::as_str).is_some()
                && ["start_line", "end_line", "start_byte", "end_byte"]
                    .into_iter()
                    .all(|field| object.get(field).and_then(Value::as_u64).is_some()) =>
        {
            Some(SourceKind::Body)
        }
        "source"
            if object.contains_key("path")
                && (object.contains_key("snapshot") || object.contains_key("sha256")) =>
        {
            Some(SourceKind::Body)
        }
        _ => None,
    }
}

fn count_occurrence(
    counter: &TokenCounter,
    command: &mut CommandCost,
    pointer: &str,
    paths: &[&str],
    text: &str,
    kind: SourceKind,
) {
    let lines = nonblank_lines(text);
    let tokens = counter.count(text);
    command.metrics.source_nonblank_lines += lines;
    command.metrics.source_tokens += tokens;
    command
        .metrics
        .source_paths
        .extend(paths.iter().map(|path| (*path).to_owned()));
    if paths.is_empty() {
        command.accounting_gaps.push(format!(
            "emitted source lacks a repository-relative path at {pointer}"
        ));
    }
    if !matches!(kind, SourceKind::Signature) {
        command.metrics.body_nonblank_lines += lines;
        command.metrics.body_tokens += tokens;
        command
            .metrics
            .body_paths
            .extend(paths.iter().map(|path| (*path).to_owned()));
    }
    command.occurrences.push(Occurrence {
        pointer: pointer.to_owned(),
        paths: paths.iter().map(|path| (*path).to_owned()).collect(),
        kind,
        nonblank_lines: lines,
        tokens,
    });
}

fn sum_metrics<'command>(commands: impl Iterator<Item = &'command CommandCost>) -> Metrics {
    let mut total = Metrics::default();
    for command in commands {
        let cost = &command.metrics;
        total.calls += cost.calls;
        total.stdout_bytes += cost.stdout_bytes;
        total.stderr_bytes += cost.stderr_bytes;
        total.response_bytes += cost.response_bytes;
        total.response_tokens += cost.response_tokens;
        total.argv_bytes += cost.argv_bytes;
        total.argv_tokens += cost.argv_tokens;
        total.source_paths.extend(cost.source_paths.iter().cloned());
        total.source_nonblank_lines += cost.source_nonblank_lines;
        total.source_tokens += cost.source_tokens;
        total.body_paths.extend(cost.body_paths.iter().cloned());
        total.body_nonblank_lines += cost.body_nonblank_lines;
        total.body_tokens += cost.body_tokens;
        total.wall_seconds += cost.wall_seconds;
    }
    total.interaction_bytes = total.response_bytes + total.argv_bytes;
    total.interaction_tokens = total.response_tokens + total.argv_tokens;
    total
}

fn assess<'command>(
    label: &str,
    limits: Limits,
    commands: impl Iterator<Item = &'command CommandCost> + Clone,
) -> Assessment {
    let metrics = sum_metrics(commands.clone());
    let mut excesses = Vec::new();
    for (name, actual, ceiling) in [
        ("calls", metrics.calls, limits.calls),
        (
            "response bytes",
            metrics.response_bytes,
            limits.response_bytes,
        ),
        (
            "response tokens",
            metrics.response_tokens,
            limits.response_tokens,
        ),
        (
            "source paths",
            metrics.source_paths.len(),
            limits.source_paths,
        ),
        (
            "nonblank source lines",
            metrics.source_nonblank_lines,
            limits.source_nonblank_lines,
        ),
    ] {
        if actual > ceiling {
            excesses.push(format!("{name}: {actual} exceeds {ceiling}"));
        }
    }
    for (name, actual, ceiling) in [
        ("source tokens", metrics.source_tokens, limits.source_tokens),
        (
            "body lines",
            metrics.body_nonblank_lines,
            limits.body_nonblank_lines,
        ),
    ] {
        if let Some(ceiling) = ceiling
            && actual > ceiling
        {
            excesses.push(format!("{name}: {actual} exceeds {ceiling}"));
        }
    }
    Assessment {
        label: label.to_owned(),
        metrics,
        excesses,
        accounting_gaps: commands
            .flat_map(|command| command.accounting_gaps.iter().cloned())
            .collect(),
    }
}
