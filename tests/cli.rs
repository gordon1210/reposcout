#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration tests intentionally fail immediately when fixtures or assertions are invalid"
)]

//! End-to-end integration tests that exercise the compiled `reposcout` binary
//! against a small multi-language fixture tree. Assertions target stable
//! behaviour (tokens, line metrics, language breakdown, markers, output
//! formats, analyzer selection and the `--fail-on` gate) so they remain valid
//! as the individual analyzers evolve.

use serde_json::Value;
use std::fmt::Write as _;
use std::path::Path;
#[path = "support/command.rs"]
mod test_command;
use test_command::{reposcout_command, test_global_config};

/// Absolute path to the bundled fixture tree.
fn fixture() -> String {
    format!("{}/tests/fixtures/sample", env!("CARGO_MANIFEST_DIR"))
}

/// Run reposcout with the given args plus `--no-cache --quiet`, returning parsed
/// JSON. Caching is disabled so tests never share state via `.reposcout/`.
fn run_json(args: &[&str]) -> Value {
    let mut cmd = reposcout_command();
    cmd.args(args);
    // Global flags must follow the (optional) subcommand, so append them last.
    cmd.arg("--no-cache").arg("--quiet");
    let output = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
}

fn language_names(v: &Value) -> Vec<String> {
    v["summary"]["languages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["name"].as_str().unwrap().to_string())
        .collect()
}

fn write_health_scope_fixture(root: &Path) {
    let source_body = (0..24).fold(String::new(), |mut body, value| {
        let _ = writeln!(body, "    total += {value};");
        body
    });
    for (name, function) in [("first.rs", "first"), ("second.rs", "second")] {
        std::fs::write(
            root.join(name),
            format!(
                "pub fn {function}(input: i32) -> i32 {{\n    // TODO source marker\n    let mut total = input;\n{source_body}    total\n}}\n"
            ),
        )
        .unwrap();
    }

    let entries = (0..24).fold(String::new(), |mut body, value| {
        let _ = writeln!(body, "  \"field_{value}\": {value},");
        body
    });
    let json = format!("{{\n  \"note\": \"TODO content marker\",\n{entries}  \"end\": true\n}}\n");
    std::fs::write(root.join("first.json"), &json).unwrap();
    std::fs::write(root.join("second.json"), json).unwrap();
}

#[path = "cli/core.rs"]
mod core;

#[path = "cli/agent_summary.rs"]
mod agent_summary;

#[path = "cli/context.rs"]
mod context;

#[path = "cli/metrics.rs"]
mod metrics;

#[path = "cli/duplication.rs"]
mod duplication;

#[path = "cli/baseline.rs"]
mod baseline;

#[path = "cli/review.rs"]
mod review;

#[path = "cli/explain.rs"]
mod explain;

#[path = "cli/graph_output.rs"]
mod graph_output;

#[path = "cli/filesystem.rs"]
mod filesystem;

fn commit_all(repo: &git2::Repository, message: &str) {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("reposcout tests", "tests@example.com").unwrap();

    if let Ok(head) = repo.head()
        && let Ok(parent) = head.peel_to_commit()
    {
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        )
        .unwrap();
    } else {
        repo.commit(Some("HEAD"), &signature, &signature, message, &tree, &[])
            .unwrap();
    }
}

#[path = "cli/change_summary.rs"]
mod change_summary;

#[path = "cli/impact.rs"]
mod impact;
