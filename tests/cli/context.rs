use super::*;

#[test]
fn context_plan_is_focus_aware_bounded_and_kept_in_summary_json() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("focus.js"),
        "import { dep } from './dependency';\nexport const focus = dep;\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("dependency.js"), "export const dep = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("dependent.js"),
        "import { focus } from './focus';\nexport const result = focus;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("focus.test.js"),
        "import { focus } from './focus';\nvoid focus;\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("unrelated.js"), "export const other = 1;\n").unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--summary",
        "--context",
        "--context-budget",
        "1000",
        "--context-max-files",
        "4",
        "--focus",
        "focus.js",
        dir.path().to_str().unwrap(),
    ]);
    let context = &report["context"];
    let selected = context["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert!(report.get("files").is_none());
    assert!(context["selected_tokens"].as_u64().unwrap() <= 1000);
    assert_eq!(selected.len(), 4);
    for expected in ["focus.js", "dependency.js", "dependent.js", "focus.test.js"] {
        assert!(
            selected.contains(&expected),
            "missing {expected}: {selected:?}"
        );
    }
    assert!(!selected.contains(&"unrelated.js"));
    assert!(
        context["graph_languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|language| language == "JavaScript")
    );
    assert!(context["files"].as_array().unwrap().iter().any(|file| {
        file["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "direct dependency of focus")
    }));
}

#[test]
fn context_plan_is_rendered_for_humans() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

    for (format, heading) in [
        ("table", "Agent context plan"),
        ("markdown", "## Agent context plan"),
    ] {
        let mut command = reposcout_command();
        command.args([
            "-f",
            format,
            "--context",
            "--no-cache",
            "--quiet",
            dir.path().to_str().unwrap(),
        ]);
        let output = command.assert().success().get_output().stdout.clone();
        let rendered = String::from_utf8(output).unwrap();
        assert!(
            rendered.contains(heading),
            "{format} output was: {rendered}"
        );
        assert!(
            rendered.contains("entrypoint"),
            "{format} omitted selection reasons"
        );
        assert!(
            rendered.contains("Selected symbol outlines"),
            "{format} omitted structural context"
        );
    }
}

#[test]
fn context_plan_projects_bounded_body_free_symbol_outlines() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("lib.rs"),
        concat!(
            "pub struct Request { pub value: String }\n",
            "pub fn execute(request: Request) -> usize {\n",
            "    let secret_body = request.value.len();\n",
            "    secret_body\n",
            "}\n",
        ),
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--context",
        "--only",
        "tokens",
        dir.path().to_str().unwrap(),
    ]);
    let context = &report["context"];
    let symbols = context["files"][0]["symbols"].as_array().unwrap();

    assert_eq!(context["strategy_version"], 3);
    assert_eq!(report["summary"]["symbols"]["functions"], 0);
    assert!(context["planning_ms"].is_number());
    assert_eq!(context["outline_symbols"], symbols.len());
    assert!(context["outline_bytes"].as_u64().unwrap() > 0);
    assert!(symbols.iter().any(|symbol| {
        symbol["name"] == "execute"
            && symbol["signature"]
                .as_str()
                .is_some_and(|signature| signature.contains("pub fn execute"))
    }));
    assert!(symbols.iter().all(|symbol| {
        !symbol["signature"]
            .as_str()
            .unwrap()
            .contains("secret_body")
            && !symbol["reasons"].as_array().unwrap().is_empty()
    }));

    let summary = run_json(&[
        "-f",
        "json",
        "--summary",
        "--context",
        "--only",
        "tokens",
        dir.path().to_str().unwrap(),
    ]);
    let compact_context = &summary["context"];
    assert_eq!(compact_context["outline_symbols"], symbols.len());
    assert_eq!(compact_context["outline_details_omitted"], true);
    assert!(
        compact_context["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file.get("symbols").is_none()),
        "summary context must not retain declaration detail objects"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "this integration scenario deliberately verifies scoped metrics, full-tree planning, graph evidence, and output projection together"
)]
fn working_context_uses_full_tree_without_widening_scoped_report_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("tests")).unwrap();
    std::fs::write(
        dir.path().join("src/dependency.ts"),
        "export const dependency = 1;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/changed.ts"),
        "import { dependency } from './dependency.js';\nexport const changed = dependency;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/direct.ts"),
        "import { changed } from './changed.js';\nexport const direct = changed;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/transitive.ts"),
        "import { direct } from './direct.js';\nexport const transitive = direct;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests/changed.test.ts"),
        "import { changed } from '../src/changed.js';\nvoid changed;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/unrelated.ts"),
        "export const unrelated = 1;\n",
    )
    .unwrap();
    commit_all(&repo, "initial context graph");
    std::fs::write(
        dir.path().join("src/changed.ts"),
        "import { dependency } from './dependency.js';\nexport const changed = dependency + 1;\n",
    )
    .unwrap();

    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--context",
        "--context-budget",
        "10000",
        "--context-max-files",
        "5",
        "--impact",
        "--review",
        dir.path().to_str().unwrap(),
    ]);
    let context = &report["context"];
    let selected = context["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(report["summary"]["files"], 1);
    assert_eq!(report["files"].as_array().unwrap().len(), 1);
    assert_eq!(report["files"][0]["path"], "src/changed.ts");
    assert_eq!(report["work_scope"]["basis"], serde_json::json!(["diff"]));
    assert_eq!(report["work_scope"]["inventory"]["source_files"], 1);
    assert_eq!(
        report["work_scope"]["confidence"]["primary"]["diff_scoped"],
        true
    );
    assert_eq!(
        report["work_scope"]["confidence"]["planning_universe"]["analyzed_files"],
        6
    );
    assert_eq!(report["work_scope"]["impact"]["direct_dependents"], 2);
    assert_eq!(report["work_scope"]["impact"]["transitive_dependents"], 1);
    assert_eq!(report["work_scope"]["impact"]["matching_tests"], 1);
    assert_eq!(context["change_scope"], "working");
    assert_eq!(
        context["changed_files"],
        serde_json::json!(["src/changed.ts"])
    );
    assert_eq!(context["planning_diagnostics"]["analyzed_files"], 6);
    assert_eq!(context["planning_diagnostics"]["unreadable_files"], 0);
    for expected in [
        "src/changed.ts",
        "src/dependency.ts",
        "src/direct.ts",
        "src/transitive.ts",
        "tests/changed.test.ts",
    ] {
        assert!(
            selected.contains(&expected),
            "missing {expected}: {selected:?}"
        );
    }
    assert!(!selected.contains(&"src/unrelated.ts"));

    let evidence = |path: &str, role: &str| {
        context["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["path"] == path)
            .and_then(|file| {
                file["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|evidence| evidence["role"] == role)
            })
            .cloned()
            .unwrap_or_else(|| panic!("missing {role} evidence for {path}"))
    };
    assert_eq!(
        evidence("src/dependency.ts", "dependency")["confidence"],
        "high"
    );
    assert_eq!(evidence("src/direct.ts", "dependent")["distance"], 1);
    assert_eq!(
        evidence("src/transitive.ts", "dependent")["confidence"],
        "partial"
    );
    assert_eq!(
        evidence("tests/changed.test.ts", "matching-test")["confidence"],
        "partial"
    );
    assert!(report["review"].is_object());
    assert!(
        report["impact"]["direct_dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "src/direct.ts")
    );
    assert!(
        report["impact"]["transitive_dependents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == "src/transitive.ts")
    );
}

#[test]
fn change_aware_context_accepts_a_deleted_file_seed() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    std::fs::write(dir.path().join("changed.js"), "export const value = 1;\n").unwrap();
    std::fs::write(
        dir.path().join("direct.js"),
        "import { value } from './changed.js';\nexport const direct = value;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("transitive.js"),
        "import { direct } from './direct.js';\nexport const transitive = direct;\n",
    )
    .unwrap();
    commit_all(&repo, "initial deleted context graph");
    std::fs::remove_file(dir.path().join("changed.js")).unwrap();

    let target = dir.path().join("changed.js");
    let report = run_json(&[
        "-f",
        "json",
        "--working",
        "--context",
        "--context-max-files",
        "2",
        target.to_str().unwrap(),
    ]);
    let context = &report["context"];
    let selected = context["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].as_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(report["summary"]["files"], 0);
    assert_eq!(context["changed_files"], serde_json::json!(["changed.js"]));
    assert!(selected.contains(&"direct.js"));
    assert!(selected.contains(&"transitive.js"));
}

#[test]
fn output_file_cannot_overwrite_a_scanned_file_target() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("source.rs");
    let original = "pub fn source() {}\n";
    std::fs::write(&file, original).unwrap();
    let path = file.to_str().unwrap();

    let mut cmd = reposcout_command();
    cmd.args(["-f", "json", "--no-cache", "--quiet", "-o", path, path]);
    let output = cmd.assert().code(1).get_output().stderr.clone();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("output path cannot be the scan target")
    );
    assert_eq!(std::fs::read_to_string(file).unwrap(), original);
}

#[test]
fn output_file_cannot_recreate_a_deleted_impact_target() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(dir.path()).unwrap();
    let file = dir.path().join("deleted.js");
    std::fs::write(&file, "export const value = 1;\n").unwrap();
    commit_all(&repo, "initial deleted target");
    std::fs::remove_file(&file).unwrap();
    let path = file.to_str().unwrap();

    let mut cmd = reposcout_command();
    cmd.args([
        "-f",
        "json",
        "--working",
        "--impact",
        "--no-cache",
        "--quiet",
        "-o",
        path,
        path,
    ]);
    let output = cmd.assert().code(1).get_output().stderr.clone();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("output path cannot be the scan target")
    );
    assert!(!file.exists());
}
