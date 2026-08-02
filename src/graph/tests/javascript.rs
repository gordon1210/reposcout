use super::*;
use crate::graph::algorithms::resolve_js;

// -- resolve_js ----------------------------------------------------------

#[test]
fn resolve_js_relative() {
    let nodes: HashSet<String> = ["src/a.ts", "src/b/index.ts", "src/c.js"]
        .iter()
        .map(ToString::to_string)
        .collect();

    assert_eq!(
        resolve_js("src/x.ts", "./a", &nodes),
        Some("src/a.ts".to_string())
    );
    assert_eq!(
        resolve_js("src/x.ts", "./b", &nodes),
        Some("src/b/index.ts".to_string())
    );
    assert_eq!(resolve_js("src/x.ts", "react", &nodes), None);
    assert_eq!(
        resolve_js("src/sub/x.ts", "../c", &nodes),
        Some("src/c.js".to_string())
    );
}

#[test]
fn resolve_js_alias() {
    let nodes: HashSet<String> = ["src/lib/d.ts"].iter().map(ToString::to_string).collect();
    assert_eq!(
        resolve_js("app/x.ts", "@/lib/d", &nodes),
        Some("src/lib/d.ts".to_string())
    );
}

#[test]
fn tsconfig_paths_resolve_jsonc_aliases_and_expose_edge_provenance() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("app")).unwrap();
    std::fs::create_dir_all(dir.path().join("src/core")).unwrap();
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{
                "files": [],
                "references": [{ "path": "./tsconfig.app.json" }]
            }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tsconfig.app.json"),
        r##"{
                // Repo-local aliases used by agents and builds.
                "compilerOptions": {
                    "baseUrl": ".",
                    "paths": {
                        "@core/*": ["src/core/*"],
                        "#exact": ["src/exact.ts"],
                    },
                },
            }"##,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("app/main.ts"),
        "import { util } from '@core/util';\nimport '#exact';\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/core/util.ts"),
        "export const util = 1;\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/exact.ts"), "export const exact = 1;\n").unwrap();

    let graph = build(
        &[
            file_report("app/main.ts"),
            file_report("src/core/util.ts"),
            file_report("src/exact.ts"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 2);
    assert_eq!(graph.unresolved_imports, 0);
    assert_eq!(graph.config_errors, 0);
    assert_eq!(graph.config_files, ["tsconfig.app.json"]);
    assert_eq!(
        graph
            .edge_list
            .iter()
            .map(|edge| (
                edge.source.as_str(),
                edge.target.as_str(),
                edge.resolver.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("app/main.ts", "src/core/util.ts", "tsconfig-paths"),
            ("app/main.ts", "src/exact.ts", "tsconfig-paths"),
        ]
    );
}

#[test]
fn focused_graph_queries_follow_the_requested_direction_and_depth() {
    let dir = tempdir().unwrap();
    for (path, content) in [
        ("app.ts", "import './service';\n"),
        ("service.ts", "import './db';\n"),
        ("db.ts", "export const db = 1;\n"),
    ] {
        std::fs::write(dir.path().join(path), content).unwrap();
    }
    let files = [
        file_report("app.ts"),
        file_report("service.ts"),
        file_report("db.ts"),
    ];

    let dependencies = analyze_with_query(
        &files,
        dir.path(),
        &[PathBuf::from("service.ts")],
        GraphDirection::Dependencies,
        1,
    )
    .report;
    assert_eq!(
        dependencies
            .files
            .iter()
            .map(|file| (file.path.as_str(), file.focus_distance))
            .collect::<Vec<_>>(),
        vec![("db.ts", Some(1)), ("service.ts", Some(0))]
    );
    assert_eq!(dependencies.edges, 1);

    let dependents = analyze_with_query(
        &files,
        dir.path(),
        &[PathBuf::from("service.ts")],
        GraphDirection::Dependents,
        1,
    )
    .report;
    assert_eq!(
        dependents
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["app.ts", "service.ts"]
    );

    let seed_only = analyze_with_query(
        &files,
        dir.path(),
        &[PathBuf::from("service.ts")],
        GraphDirection::Both,
        0,
    )
    .report;
    assert_eq!(seed_only.nodes, 1);
    assert_eq!(seed_only.edges, 0);
    assert_eq!(seed_only.files[0].path, "service.ts");

    let unmatched = analyze_with_query(
        &files,
        dir.path(),
        &[PathBuf::from("missing")],
        GraphDirection::Both,
        2,
    )
    .report;
    assert_eq!(unmatched.nodes, 0);
    assert_eq!(unmatched.unmatched_focus, ["missing"]);
}

#[test]
fn invalid_tsconfig_is_diagnostic_and_reduces_impact_confidence() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("tsconfig.json"), "{ invalid json").unwrap();
    std::fs::write(dir.path().join("changed.ts"), "export const changed = 1;\n").unwrap();
    std::fs::write(dir.path().join("consumer.ts"), "import './changed';\n").unwrap();
    let paths = vec![PathBuf::from("changed.ts"), PathBuf::from("consumer.ts")];

    let graph = build(
        &[file_report("changed.ts"), file_report("consumer.ts")],
        dir.path(),
    );
    assert_eq!(graph.config_errors, 1);

    let impact = impact(
        &paths,
        dir.path(),
        &HashSet::from([PathBuf::from("changed.ts")]),
    );
    assert_eq!(impact.config_errors, 1);
    assert_eq!(impact.confidence, "partial");
}

#[test]
fn diagnostic_facts_merge_all_errors_for_the_same_path() {
    let analysis = GraphAnalysis {
        report: DepGraph::default(),
        signals: GraphSignals::default(),
        topology: Topology {
            graph_files: vec!["src/main.rs".to_string()],
            edges: Vec::new(),
            unresolved_imports: 2,
            unresolved_by_node: vec![2],
            parse_errors_by_node: vec![1],
            unreadable_nodes: HashSet::new(),
            parse_errors: 1,
            edge_resolvers: BTreeMap::new(),
            config_errors: 3,
            config_errors_by_path: BTreeMap::from([("src/main.rs".to_string(), 3)]),
            config_files: Vec::new(),
            symbols: Vec::new(),
            symbol_edges: Vec::new(),
            unresolved_symbol_relations: 0,
            unresolved_symbol_relations_by_path: HashMap::new(),
        },
    };

    let facts = diagnostic_facts(&analysis);

    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].path, "src/main.rs");
    assert_eq!(facts[0].parse_errors, 1);
    assert_eq!(facts[0].unresolved_imports, 2);
    assert_eq!(facts[0].config_errors, 3);
}

#[test]
fn local_package_exports_and_imports_resolve_with_provenance() {
    let dir = tempdir().unwrap();
    for directory in ["apps/web", "packages/core/src"] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    std::fs::write(
        dir.path().join("packages/core/package.json"),
        r##"{
                "name": "@acme/core",
                "exports": {
                    ".": "./src/index.js",
                    "./*": { "import": "./src/*.js", "types": "./src/*.d.ts" }
                },
                "imports": { "#internal": "./src/internal.js" }
            }"##,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/web/main.ts"),
        "import '@acme/core';\nimport '@acme/core/feature';\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("packages/core/src/index.ts"),
        "import '#internal';\nexport const core = 1;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("packages/core/src/feature.ts"),
        "export const feature = 1;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("packages/core/src/internal.ts"),
        "export const internal = 1;\n",
    )
    .unwrap();

    let graph = build(
        &[
            file_report("apps/web/main.ts"),
            file_report("packages/core/src/index.ts"),
            file_report("packages/core/src/feature.ts"),
            file_report("packages/core/src/internal.ts"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 3);
    assert_eq!(graph.unresolved_imports, 0);
    assert!(
        graph
            .config_files
            .contains(&"packages/core/package.json".to_string())
    );
    assert!(graph.edge_list.iter().any(|edge| {
        edge.source == "apps/web/main.ts"
            && edge.target == "packages/core/src/index.ts"
            && edge.resolver == "package-exports"
    }));
    assert!(graph.edge_list.iter().any(|edge| {
        edge.source == "packages/core/src/index.ts"
            && edge.target == "packages/core/src/internal.ts"
            && edge.resolver == "package-imports"
    }));
}

#[test]
fn explicit_null_package_exports_do_not_fall_back_to_entrypoints() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("apps/web")).unwrap();
    std::fs::create_dir_all(dir.path().join("packages/core/src")).unwrap();
    std::fs::write(
        dir.path().join("packages/core/package.json"),
        r#"{
                "name": "@acme/core",
                "exports": { ".": null },
                "main": "./src/index.js"
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

    let graph = build(
        &[
            file_report("apps/web/main.ts"),
            file_report("packages/core/src/index.ts"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 0);
    assert_eq!(graph.unresolved_imports, 1);
}

#[test]
fn explicit_package_export_blocks_override_broader_wildcards() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("apps/web")).unwrap();
    std::fs::create_dir_all(dir.path().join("packages/core/src")).unwrap();
    std::fs::write(
        dir.path().join("packages/core/package.json"),
        r#"{
                "name": "@acme/core",
                "exports": {
                    "./private": null,
                    "./*": "./src/*.js"
                }
            }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("apps/web/main.ts"),
        "import '@acme/core/private';\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("packages/core/src/private.ts"),
        "export const privateValue = 1;\n",
    )
    .unwrap();

    let graph = build(
        &[
            file_report("apps/web/main.ts"),
            file_report("packages/core/src/private.ts"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 0);
    assert_eq!(graph.unresolved_imports, 1);
}

#[test]
fn duplicate_local_package_names_are_diagnostic_and_unresolved() {
    let dir = tempdir().unwrap();
    for directory in ["apps/web", "packages/a/src", "packages/b/src"] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    for package in ["a", "b"] {
        std::fs::write(
            dir.path().join(format!("packages/{package}/package.json")),
            r#"{ "name": "@acme/core", "main": "./src/index.js" }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(format!("packages/{package}/src/index.ts")),
            "export const core = 1;\n",
        )
        .unwrap();
    }
    std::fs::write(
        dir.path().join("apps/web/main.ts"),
        "import '@acme/core';\n",
    )
    .unwrap();

    let graph = build(
        &[
            file_report("apps/web/main.ts"),
            file_report("packages/a/src/index.ts"),
            file_report("packages/b/src/index.ts"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 0);
    assert_eq!(graph.unresolved_imports, 1);
    assert_eq!(graph.config_errors, 1);
}
