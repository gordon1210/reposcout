use crate::efficiency::support::{CacheState, CostLedger};
use crate::journeys::support::Journey;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// These are user inputs and interaction ceilings, never fixture or probe data.
#[derive(Serialize)]
pub(super) struct Task {
    pub(super) description: &'static str,
    pub(super) file: &'static str,
    pub(super) symbol: &'static str,
    pub(super) encoding: &'static str,
    pub(super) response_tokens: usize,
    pub(super) response_bytes: usize,
}

pub(super) fn investigate(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    task: &Task,
) -> Vec<Value> {
    let first = run(
        journey,
        ledger,
        task,
        "read the user's known payable definition",
        vec![
            "read".to_owned(),
            ".".to_owned(),
            "--symbol".to_owned(),
            task.file.to_owned(),
            task.symbol.to_owned(),
        ],
        (750, 2304),
        CacheState::Cold,
    );
    let query = format!("test {}", task.symbol);
    let search = run(
        journey,
        ledger,
        task,
        "find regression candidates for the user's known production definition",
        ["find", &query, ".", "--match", "all", "--limit", "4"]
            .map(str::to_owned)
            .to_vec(),
        (850, 2560),
        CacheState::Warm,
    );

    let mut reports = vec![first, search];
    let imports = regression_and_binding_targets(task, &reports);
    reports.push(run(
        journey,
        ledger,
        task,
        "read the discovered regression and a bounded window before the known definition",
        imports,
        (1100, 3584),
        CacheState::Warm,
    ));

    if let Some(support) = read_imported_support(journey, ledger, task, &reports) {
        reports.push(support);
    }
    reports
}

fn regression_and_binding_targets(task: &Task, reports: &[Value]) -> Vec<String> {
    let mut files = BTreeMap::new();
    for hit in array(&reports[1]["hits"]) {
        if hit["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("test_"))
        {
            let read = &hit["read"];
            if let Some((path, hash)) = read["path"].as_str().zip(read["expected_hash"].as_str())
                && path != task.file
            {
                files.insert(path.to_owned(), Some(hash.to_owned()));
            }
        }
    }
    let mut args = vec!["read".to_owned(), ".".to_owned()];
    for (path, hash) in files.into_iter().take(3) {
        args.extend(["--file".to_owned(), path.clone()]);
        if let Some(hash) = hash {
            args.extend(["--expect-hash".to_owned(), path, hash]);
        }
    }
    // Follow the delivered definition location. The preceding window stays bounded
    // even when natural module helpers appear before the binding and definition.
    let binding_end = array(&reports[0]["sources"])
        .iter()
        .filter_map(|source| source["span"]["start_line"].as_u64())
        .min()
        .and_then(|start| start.checked_sub(1).filter(|end| *end > 0))
        .unwrap_or(16);
    let binding_start = binding_end.saturating_sub(15).max(1);
    args.extend([
        "--range".to_owned(),
        task.file.to_owned(),
        binding_start.to_string(),
        binding_end.to_string(),
    ]);
    if let Some(hash) = observed_hash(reports, task.file) {
        args.extend(["--expect-hash".to_owned(), task.file.to_owned(), hash]);
    }
    args
}

fn read_imported_support(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    task: &Task,
    reports: &[Value],
) -> Option<Value> {
    let names: BTreeSet<_> = array(&reports[0]["sources"])
        .iter()
        .filter_map(|chunk| chunk["content"].as_str())
        .flat_map(source_names)
        .collect();
    let files: BTreeSet<_> = array(&reports[2]["sources"])
        .iter()
        .filter_map(|chunk| chunk["content"].as_str())
        .flat_map(|source| imported_modules(source, &names))
        .filter(|path| path != task.file)
        .collect();
    if !files.is_empty() {
        let mut args = vec!["read".to_owned(), ".".to_owned()];
        for path in files.into_iter().take(3) {
            args.extend(["--file".to_owned(), path.clone()]);
            if let Some(hash) = observed_hash(reports, &path) {
                args.extend(["--expect-hash".to_owned(), path, hash]);
            }
        }
        let tokens = task
            .response_tokens
            .saturating_sub(ledger.totals().response_tokens);
        let bytes = task
            .response_bytes
            .saturating_sub(ledger.totals().response_bytes);
        if tokens >= 256 && bytes >= 1024 {
            return Some(run(
                journey,
                ledger,
                task,
                "read supporting modules named by delivered import bindings",
                args,
                (tokens, bytes),
                CacheState::Warm,
            ));
        }
    }
    None
}

fn imported_modules(source: &str, names: &BTreeSet<String>) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    let mut statement = String::new();
    for line in source.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        if statement.is_empty() {
            if !code.starts_with("from ") {
                continue;
            }
            statement.push_str(code);
        } else {
            statement.push(' ');
            statement.push_str(code);
        }
        if statement.contains('(') && !statement.contains(')') {
            continue;
        }
        if let Some(path) = imported_module(&statement, names) {
            files.insert(path);
        }
        statement.clear();
    }
    files
}

fn imported_module(statement: &str, names: &BTreeSet<String>) -> Option<String> {
    let (prefix, bindings) = statement.split_once(" import ")?;
    let module = prefix.strip_prefix("from ")?.trim();
    let simple_module = module.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    });
    let bindings = if bindings.starts_with('(') {
        bindings.strip_prefix('(')?.strip_suffix(')')?.trim()
    } else {
        bindings
    };
    let needed = bindings.split(',').any(|binding| {
        let words: Vec<_> = binding.split_whitespace().collect();
        let local = match words.as_slice() {
            [name] => name,
            [_, "as", alias] => alias,
            _ => return false,
        };
        names.contains(*local)
    });
    (simple_module && needed).then(|| format!("{}.py", module.replace('.', "/")))
}

fn observed_hash(reports: &[Value], path: &str) -> Option<String> {
    reports
        .iter()
        .flat_map(|report| array(&report["files"]))
        .find(|file| file["path"] == path)
        .and_then(|file| file["sha256"].as_str())
        .map(str::to_owned)
}

fn run(
    journey: &mut Journey<'_>,
    ledger: &mut CostLedger,
    task: &Task,
    label: &str,
    mut args: Vec<String>,
    allowance: (usize, usize),
    cache: CacheState,
) -> Value {
    let tokens = allowance.0.min(
        task.response_tokens
            .saturating_sub(ledger.totals().response_tokens),
    );
    let bytes = allowance.1.min(
        task.response_bytes
            .saturating_sub(ledger.totals().response_bytes),
    );
    assert!(
        tokens >= 256 && bytes >= 1024,
        "frozen episode leaves no valid public query budget"
    );
    args.extend([
        "--budget".to_owned(),
        tokens.to_string(),
        "--max-output-bytes".to_owned(),
        bytes.to_string(),
        "--encoding".to_owned(),
        task.encoding.to_owned(),
        "-f".to_owned(),
        "json".to_owned(),
        "--quiet".to_owned(),
    ]);
    let step = ledger.step(
        journey,
        label,
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        cache,
    );
    if step.exit_code() != Some(0) {
        return json!({"driver_cli_error": {"label": label, "exit": step.exit_code()}});
    }
    serde_json::from_slice(step.stdout_bytes()).unwrap_or_else(|error| {
        json!({"driver_cli_error": {"label": label, "invalid_stdout_json": error.to_string()}})
    })
}

fn source_names(source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in source.lines() {
        if line.trim_start().starts_with("def ") {
            for annotation in line.split(':').skip(1) {
                let name = annotation
                    .trim_start()
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if name.chars().next().is_some_and(char::is_uppercase) {
                    names.insert(name.to_owned());
                }
            }
        } else if !line.trim_start().starts_with("\"\"\"") {
            for prefix in line.split('(').take(line.matches('(').count()) {
                let name = prefix
                    .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("");
                if !name.is_empty() {
                    names.insert(name.to_owned());
                }
            }
        }
    }
    names
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}
