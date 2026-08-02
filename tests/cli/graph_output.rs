use super::*;

#[test]
fn graph_flag_produces_graph_object() {
    let v = run_json(&["-f", "json", "--graph", &fixture()]);
    let g = &v["graph"];
    assert!(
        g.is_object(),
        "expected graph object when --graph is passed"
    );
    assert!(g["nodes"].as_u64().is_some(), "graph.nodes must be present");
    assert!(g["edges"].as_u64().is_some(), "graph.edges must be present");
    assert!(g["cycles"].is_array(), "graph.cycles must be an array");
    assert!(g["orphans"].is_array(), "graph.orphans must be an array");
    assert!(
        g["top_depended"].is_array(),
        "graph.top_depended must be an array"
    );
    assert!(
        g["most_dependent"].is_array(),
        "graph.most_dependent must be an array"
    );
    assert!(
        g["unresolved_imports"].as_u64().is_some(),
        "graph.unresolved_imports must be present"
    );
    // The fixture contains app.js and util.py — both are JS/Python graph files.
    assert!(
        g["nodes"].as_u64().unwrap() >= 2,
        "expected at least 2 nodes (app.js + util.py)"
    );
}

#[test]
fn graph_absent_without_flag() {
    let v = run_json(&["-f", "json", &fixture()]);
    assert!(
        v["graph"].is_null(),
        "graph field must be absent when --graph is not passed"
    );
}

#[test]
fn graph_table_shows_section_header() {
    let mut cmd = reposcout_command();
    cmd.args([
        "--no-cache",
        "--quiet",
        "-f",
        "table",
        "--graph",
        &fixture(),
    ]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("Dependency graph"),
        "table output must contain 'Dependency graph' section"
    );
}

#[test]
fn graph_query_exposes_bounded_adjacency_and_tsconfig_edges() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/core")).unwrap();
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{
          "compilerOptions": {
            "baseUrl": ".",
            "paths": { "@core/*": ["src/core/*"] }
          }
        }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/app.ts"),
        "import { service } from '@core/service';\nexport { service };\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/core/service.ts"),
        "import { db } from './db';\nexport const service = db;\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/core/db.ts"), "export const db = 1;\n").unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--only",
        "imports",
        "--graph-focus",
        "src/core/service.ts",
        "--graph-direction",
        "dependents",
        "--graph-depth",
        "1",
        dir.path().to_str().unwrap(),
    ]);
    let graph = &report["graph"];

    assert_eq!(graph["nodes"], 2);
    assert_eq!(graph["edges"], 1);
    assert_eq!(graph["direction"], "dependents");
    assert_eq!(graph["depth"], 1);
    assert_eq!(graph["config_files"], serde_json::json!(["tsconfig.json"]));
    assert_eq!(
        graph["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["src/app.ts", "src/core/service.ts"]
    );
    assert_eq!(graph["edge_list"][0]["source"], "src/app.ts");
    assert_eq!(graph["edge_list"][0]["target"], "src/core/service.ts");
    assert_eq!(graph["edge_list"][0]["resolver"], "tsconfig-paths");
}

#[test]
fn graph_json_exposes_package_and_python_absolute_resolver_provenance() {
    let dir = tempfile::tempdir().unwrap();
    for directory in ["apps/web", "apps/tools", "packages/core/src", "src/domain"] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    std::fs::write(
        dir.path().join("packages/core/package.json"),
        r#"{
          "name": "@acme/core",
          "exports": { ".": "./src/index.js" }
        }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/web/main.ts"),
        "import '@acme/core';\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("packages/core/src/index.ts"),
        "export const core = 1;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/tools/main.py"),
        "from domain.service import VALUE\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/domain/service.py"), "VALUE = 1\n").unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--only",
        "imports",
        "--graph",
        dir.path().to_str().unwrap(),
    ]);
    let graph = &report["graph"];

    assert!(
        graph["config_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "packages/core/package.json")
    );
    assert!(graph["edge_list"].as_array().unwrap().iter().any(|edge| {
        edge["source"] == "apps/web/main.ts"
            && edge["target"] == "packages/core/src/index.ts"
            && edge["resolver"] == "package-exports"
    }));
    assert!(graph["edge_list"].as_array().unwrap().iter().any(|edge| {
        edge["source"] == "apps/tools/main.py"
            && edge["target"] == "src/domain/service.py"
            && edge["resolver"] == "python-src-root"
    }));
}

#[test]
fn graph_json_exposes_rust_and_go_module_resolver_provenance() {
    let dir = tempfile::tempdir().unwrap();
    for directory in ["src", "cmd", "internal/store"] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    std::fs::write(
        dir.path().join("Cargo.toml"),
        r#"[package]
name = "mixed-rust"
version = "0.1.0"
edition = "2024"
"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub mod service;\n").unwrap();
    std::fs::write(dir.path().join("src/service.rs"), "pub fn run() {}\n").unwrap();
    std::fs::write(dir.path().join("go.mod"), "module example.com/mixed\n").unwrap();
    std::fs::write(
        dir.path().join("cmd/main.go"),
        "package main\n\nimport \"example.com/mixed/internal/store\"\n\nfunc main() { store.Open() }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("internal/store/store.go"),
        "package store\n\nfunc Open() {}\n",
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--only",
        "imports",
        "--graph",
        dir.path().to_str().unwrap(),
    ]);
    let graph = &report["graph"];

    assert_eq!(graph["languages"], serde_json::json!(["Go", "Rust"]));
    assert_eq!(
        graph["config_files"],
        serde_json::json!(["Cargo.toml", "go.mod"])
    );
    assert!(graph["edge_list"].as_array().unwrap().iter().any(|edge| {
        edge["source"] == "src/lib.rs"
            && edge["target"] == "src/service.rs"
            && edge["resolver"] == "rust-mod"
    }));
    assert!(graph["edge_list"].as_array().unwrap().iter().any(|edge| {
        edge["source"] == "cmd/main.go"
            && edge["target"] == "internal/store/store.go"
            && edge["resolver"] == "go-module"
    }));
}

#[test]
fn php_is_first_class_across_scan_context_tests_and_composer_graphs() {
    let dir = tempfile::tempdir().unwrap();
    for directory in ["src/Http", "src/Service", "tests/Service"] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{
          "autoload": { "psr-4": { "App\\": "src/" } },
          "autoload-dev": { "psr-4": { "Tests\\": "tests/" } }
        }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/Service/UserService.php"),
        r#"<?php
namespace App\Service;
use Psr\Log\LoggerInterface;

final class UserService {
    public function find(bool $active): int {
        $TODO = "TODO in a string";
        // TODO real marker
        return $active ? 1 : 0;
    }
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/Http/Controller.php"),
        "<?php\nnamespace App\\Http;\nuse App\\Service\\UserService;\nfinal class Controller {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests/Service/UserServiceTest.php"),
        "<?php\nnamespace Tests\\Service;\nuse App\\Service\\UserService;\nfinal class UserServiceTest {}\n",
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--graph",
        "--context",
        "--focus",
        "src/Service/UserService.php",
        dir.path().to_str().unwrap(),
    ]);
    let service = report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "src/Service/UserService.php")
        .unwrap();

    assert_eq!(service["language"], "PHP");
    assert_eq!(service["approximate"], false);
    assert!(service.get("line_metrics_approximate").is_none());
    assert_eq!(service["markers"]["TODO"], 1);
    assert_eq!(service["imports"], serde_json::json!(["Psr"]));
    assert_eq!(service["symbols"]["types"], 1);
    assert_eq!(service["symbols"]["functions"], 1);
    assert_eq!(service["complexity"]["functions"][0]["name"], "find");
    assert_eq!(service["complexity"]["functions"][0]["cyclomatic"], 2);
    assert!(
        !report["summary"]["test_presence"]["untested_samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "src/Service/UserService.php")
    );
    assert!(
        report["context"]["graph_languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|language| language == "PHP")
    );
    assert_eq!(
        report["graph"]["config_files"],
        serde_json::json!(["composer.json"])
    );
    assert!(
        report["graph"]["edge_list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| {
                edge["source"] == "src/Http/Controller.php"
                    && edge["target"] == "src/Service/UserService.php"
                    && edge["resolver"] == "composer-psr-4"
            })
    );
}

#[test]
fn dot_and_mermaid_formats_export_the_graph_without_external_tools() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.ts"), "import './b';\n").unwrap();
    std::fs::write(dir.path().join("b.ts"), "export const b = 1;\n").unwrap();

    for (format, expected) in [("dot", "digraph reposcout"), ("mermaid", "flowchart LR")] {
        let mut cmd = reposcout_command();
        cmd.args([
            "--only",
            "imports",
            "--no-cache",
            "--quiet",
            "-f",
            format,
            dir.path().to_str().unwrap(),
        ]);
        let output = cmd.assert().success().get_output().stdout.clone();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains(expected), "output was: {rendered}");
        assert!(rendered.contains("a.ts"));
        assert!(rendered.contains("b.ts"));
    }

    let inferred = dir.path().join("focused.mmd");
    let mut cmd = reposcout_command();
    cmd.args([
        "--only",
        "imports",
        "--no-cache",
        "--quiet",
        "--graph-focus",
        "a.ts",
        "-o",
        inferred.to_str().unwrap(),
        dir.path().to_str().unwrap(),
    ]);
    cmd.assert().success();
    let rendered = std::fs::read_to_string(inferred).unwrap();
    assert!(rendered.starts_with("flowchart LR"));
    assert!(rendered.contains("class n0 focus"));
}

#[test]
fn ndjson_output_has_summary_and_file_lines() {
    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "ndjson",
        "--context",
        "--graph",
        "--no-cache",
        "--quiet",
        &fixture(),
    ]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = text.split('\n').filter(|l| !l.is_empty()).collect();
    assert!(
        lines.len() >= 2,
        "expected at least 2 NDJSON lines, got {}",
        lines.len()
    );
    for line in &lines {
        serde_json::from_str::<Value>(line)
            .unwrap_or_else(|e| panic!("NDJSON line is not valid JSON: {e}\nline: {line}"));
    }
    let first: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(
        first["kind"], "summary",
        "first line must have kind=summary"
    );
    assert!(
        first["diagnostics"].is_object(),
        "summary line must carry scan diagnostics"
    );
    assert!(
        first["context"].is_object(),
        "summary line must carry a requested context plan"
    );
    assert!(
        first["work_scope"].is_object(),
        "summary line must carry work-scope evidence"
    );
    assert!(
        first["graph"]["files"].is_array(),
        "full NDJSON summary must carry deterministic graph files"
    );
    for field in [
        "schema_version",
        "root",
        "target",
        "generated_at",
        "encoding",
        "analysis_profile",
    ] {
        assert!(!first[field].is_null(), "summary line omitted {field}");
    }
    let has_file_line = lines
        .iter()
        .any(|l| serde_json::from_str::<Value>(l).is_ok_and(|v| v["kind"] == "file"));
    assert!(has_file_line, "at least one line must have kind=file");
    let finding = lines
        .iter()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value["kind"] == "finding")
        .expect("full NDJSON output must append duplicate findings");
    assert!(matches!(
        finding["finding_kind"].as_str(),
        Some("exact" | "type2")
    ));
}

#[test]
fn broken_stdout_pipe_exits_successfully_without_noise() {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..512 {
        std::fs::write(
            dir.path().join(format!("bounded-{index:03}.txt")),
            format!("bounded pipe fixture {index}\n"),
        )
        .unwrap();
    }
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin!("reposcout"))
        .env("REPOSCOUT_GLOBAL_CONFIG", test_global_config())
        .args([
            "tokens",
            "-f",
            "ndjson",
            "--no-cache",
            "--quiet",
            dir.path().to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take().unwrap());

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "status was {}", output.status);
    assert!(
        output.stderr.is_empty(),
        "stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sarif_output_is_valid_2_1_0() {
    let v = run_json(&["-f", "sarif", "--max-complexity", "1", &fixture()]);
    assert_eq!(v["version"], "2.1.0");
    assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "reposcout");
    let rules = v["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .expect("rules must be an array");
    assert!(!rules.is_empty(), "rules must be non-empty");
    assert!(
        v["runs"][0]["results"].is_array(),
        "results must be an array"
    );
    assert_eq!(v["runs"][0]["columnKind"], "unicodeCodePoints");
    let duplicate = v["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["ruleId"] == "reposcout/duplicate-code")
        .expect("duplicate SARIF result");
    assert!(duplicate["locations"][0]["physicalLocation"]["region"]["startColumn"].is_number());
    assert!(
        duplicate["relatedLocations"][0]["physicalLocation"]["region"]["endColumn"].is_number()
    );
    let complexity = v["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["ruleId"] == "reposcout/high-complexity-function")
        .expect("per-function complexity SARIF result");
    assert_eq!(complexity["level"], "warning");
    assert_eq!(complexity["properties"]["maximum"], 1);
    assert!(complexity["properties"]["cyclomatic"].as_u64().unwrap() > 1);
}
