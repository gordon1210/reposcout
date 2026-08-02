use super::*;

#[test]
fn python_absolute_imports_resolve_from_conventional_src_roots() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("apps")).unwrap();
    std::fs::create_dir_all(dir.path().join("src/domain")).unwrap();
    std::fs::write(
        dir.path().join("apps/main.py"),
        "from domain.service import VALUE\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/domain/__init__.py"), "\n").unwrap();
    std::fs::write(dir.path().join("src/domain/service.py"), "VALUE = 1\n").unwrap();

    let graph = build(
        &[
            file_report("apps/main.py"),
            file_report("src/domain/__init__.py"),
            file_report("src/domain/service.py"),
        ],
        dir.path(),
    );

    assert_eq!(graph.unresolved_imports, 0);
    assert!(graph.edge_list.iter().any(|edge| {
        edge.source == "apps/main.py"
            && edge.target == "src/domain/service.py"
            && edge.resolver == "python-src-root"
    }));
}

#[test]
fn python_absolute_imports_prefer_the_nearest_package_src_root() {
    let dir = tempdir().unwrap();
    for directory in [
        "packages/a/tests",
        "packages/a/src/domain",
        "packages/b/src/domain",
    ] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    std::fs::write(
        dir.path().join("packages/a/tests/main.py"),
        "from domain.service import VALUE\n",
    )
    .unwrap();
    for path in [
        "packages/a/src/domain/service.py",
        "packages/b/src/domain/service.py",
    ] {
        std::fs::write(dir.path().join(path), "VALUE = 1\n").unwrap();
    }

    let graph = build(
        &[
            file_report("packages/a/tests/main.py"),
            file_report("packages/a/src/domain/service.py"),
            file_report("packages/b/src/domain/service.py"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 1);
    assert!(graph.edge_list.iter().any(|edge| {
        edge.source == "packages/a/tests/main.py"
            && edge.target == "packages/a/src/domain/service.py"
            && edge.resolver == "python-src-root"
    }));
}

#[test]
fn python_absolute_imports_leave_unrelated_src_roots_ambiguous() {
    let dir = tempdir().unwrap();
    for directory in ["apps", "packages/a/src/domain", "packages/b/src/domain"] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    std::fs::write(
        dir.path().join("apps/main.py"),
        "from domain.service import VALUE\n",
    )
    .unwrap();
    for path in [
        "packages/a/src/domain/service.py",
        "packages/b/src/domain/service.py",
    ] {
        std::fs::write(dir.path().join(path), "VALUE = 1\n").unwrap();
    }

    let graph = build(
        &[
            file_report("apps/main.py"),
            file_report("packages/a/src/domain/service.py"),
            file_report("packages/b/src/domain/service.py"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 0);
    assert_eq!(graph.unresolved_imports, 1);
}

#[test]
fn javascript_runtime_extensions_substitute_typescript_sources() {
    let nodes = HashSet::from(["src/value.ts".to_string(), "src/component.tsx".to_string()]);
    assert_eq!(
        try_resolve_js("src/value.js", &nodes),
        Some("src/value.ts".to_string())
    );
    assert_eq!(
        try_resolve_js("src/component.jsx", &nodes),
        Some("src/component.tsx".to_string())
    );
}

#[test]
fn package_internal_specifiers_without_metadata_are_unresolved() {
    assert!(matches!(
        JsResolver::default().resolve("src/main.ts", "#internal", &HashSet::new()),
        ImportResolution::Unresolved
    ));
}

#[test]
fn virtual_deleted_files_still_seed_existing_dependents() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/consumer.ts"),
        "import './deleted.js';\n",
    )
    .unwrap();
    let virtual_paths = HashSet::from(["src/deleted.ts".to_string()]);
    let analysis = analyze_paths(
        &[
            PathBuf::from("src/consumer.ts"),
            PathBuf::from("src/deleted.ts"),
        ],
        dir.path(),
        &virtual_paths,
    );

    let deleted = analysis.signals.files.get("src/deleted.ts").unwrap();
    assert_eq!(deleted.dependents, ["src/consumer.ts"]);
    assert_eq!(
        deleted
            .dependent_resolvers
            .get("src/consumer.ts")
            .map(String::as_str),
        Some("relative")
    );
    assert!(analysis.topology.unreadable_nodes.is_empty());
}

// -- resolve_py ----------------------------------------------------------

#[test]
fn resolve_py_relative() {
    let nodes: HashSet<String> = ["pkg/foo.py", "pkg/sub/__init__.py", "pkg/bar/__init__.py"]
        .iter()
        .map(ToString::to_string)
        .collect();

    assert_eq!(
        resolve_py("pkg/x.py", ".foo", &nodes),
        Some("pkg/foo.py".to_string())
    );
    assert_eq!(
        resolve_py("pkg/sub/x.py", "..bar", &nodes),
        Some("pkg/bar/__init__.py".to_string())
    );
}

#[test]
fn resolve_py_absolute_is_none() {
    let nodes: HashSet<String> = ["os.py"].iter().map(ToString::to_string).collect();
    assert_eq!(resolve_py("pkg/x.py", "os", &nodes), None);
}

// -- strongly_connected --------------------------------------------------

#[test]
fn scc_two_cycle() {
    let nodes: Vec<String> = vec!["a".to_string(), "b".to_string()];
    // 0 -> 1, 1 -> 0
    let edges = vec![(0usize, 1usize), (1, 0)];
    let sccs = strongly_connected(&nodes, &edges);
    let big: Vec<_> = sccs.iter().filter(|c| c.len() >= 2).collect();
    assert_eq!(big.len(), 1, "expected exactly one cycle component");
    let mut comp = big[0].clone();
    comp.sort_unstable();
    assert_eq!(comp, vec![0, 1]);
}

#[test]
fn scc_acyclic() {
    let nodes: Vec<String> = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    // 0 -> 1 -> 2 (no cycle)
    let edges = vec![(0usize, 1usize), (1, 2)];
    let sccs = strongly_connected(&nodes, &edges);
    // Every component must be a singleton.
    assert!(
        sccs.iter().all(|c| c.len() == 1),
        "expected only singletons: {sccs:?}"
    );
}

// -- is_entrypoint -------------------------------------------------------

#[test]
fn entrypoints() {
    assert!(is_entrypoint("index.ts"));
    assert!(is_entrypoint("src/index.js"));
    assert!(is_entrypoint("__init__.py"));
    assert!(is_entrypoint("pkg/__init__.py"));
    assert!(is_entrypoint("next.config.js"));
    assert!(is_entrypoint("vite.config.ts"));
    assert!(is_entrypoint("types.d.ts"));
    assert!(is_entrypoint("src/main.ts"));
    assert!(is_entrypoint("app.py"));
}

#[test]
fn non_entrypoints() {
    assert!(!is_entrypoint("foo.ts"));
    assert!(!is_entrypoint("util.py"));
    assert!(!is_entrypoint("helper.js"));
}

// -- normalize_path / go_up (smoke tests) --------------------------------

#[test]
fn normalize_dots() {
    assert_eq!(normalize_path("src/./a"), "src/a");
    assert_eq!(normalize_path("src/sub/../c"), "src/c");
    assert_eq!(normalize_path("./a"), "a");
}

#[test]
fn go_up_levels() {
    assert_eq!(go_up("a/b/c", 1), "a/b");
    assert_eq!(go_up("a/b/c", 2), "a");
    assert_eq!(go_up("a", 1), "");
}
