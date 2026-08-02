use super::*;

#[test]
fn py_from_current_package_import_resolves_sibling_module() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("pkg")).unwrap();
    std::fs::write(
        dir.path().join("pkg/consumer.py"),
        "from . import sibling\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("pkg/sibling.py"), "VALUE = 1\n").unwrap();

    let graph = build(
        &[
            file_report("pkg/consumer.py"),
            file_report("pkg/sibling.py"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 1);
    assert_eq!(graph.unresolved_imports, 0);
    assert_eq!(graph.most_dependent[0].path, "pkg/consumer.py");
    assert_eq!(graph.most_dependent[0].fan_out, 1);
    assert_eq!(graph.top_depended[0].path, "pkg/sibling.py");
}

#[test]
fn py_from_current_package_import_resolves_comma_separated_aliased_siblings() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("pkg")).unwrap();
    std::fs::write(
        dir.path().join("pkg/consumer.py"),
        "from . import alpha, beta as renamed\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("pkg/alpha.py"), "VALUE = 1\n").unwrap();
    std::fs::write(dir.path().join("pkg/beta.py"), "VALUE = 2\n").unwrap();

    let graph = build(
        &[
            file_report("pkg/consumer.py"),
            file_report("pkg/alpha.py"),
            file_report("pkg/beta.py"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 2);
    assert_eq!(graph.unresolved_imports, 0);
    assert_eq!(graph.most_dependent[0].path, "pkg/consumer.py");
    assert_eq!(graph.most_dependent[0].fan_out, 2);
    assert_eq!(
        graph
            .top_depended
            .iter()
            .map(|node| node.path.as_str())
            .collect::<Vec<_>>(),
        vec!["pkg/alpha.py", "pkg/beta.py"]
    );
}

#[test]
fn py_from_named_package_import_keeps_dependency_on_package() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("pkg/subpkg")).unwrap();
    std::fs::write(
        dir.path().join("pkg/consumer.py"),
        "from .subpkg import name\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("pkg/subpkg/__init__.py"), "\n").unwrap();
    std::fs::write(dir.path().join("pkg/subpkg/name.py"), "VALUE = 1\n").unwrap();

    let graph = build(
        &[
            file_report("pkg/consumer.py"),
            file_report("pkg/subpkg/__init__.py"),
            file_report("pkg/subpkg/name.py"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 1);
    assert_eq!(graph.unresolved_imports, 0);
    assert_eq!(graph.top_depended.len(), 1);
    assert_eq!(graph.top_depended[0].path, "pkg/subpkg/__init__.py");
    assert_eq!(graph.top_depended[0].fan_in, 1);
}

#[test]
fn php_composer_and_static_includes_build_edges_with_provenance() {
    let dir = tempdir().unwrap();
    for directory in ["src/Http", "src/Service", "src/Support", "tests"] {
        std::fs::create_dir_all(dir.path().join(directory)).unwrap();
    }
    std::fs::write(
        dir.path().join("composer.json"),
        r#"{
                "autoload": { "psr-4": { "App\\": "src/" } },
                "autoload-dev": { "psr-4": { "Tests\\": ["tests/"] } }
            }"#,
    )
    .unwrap();
    std::fs::write(
            dir.path().join("src/Http/Controller.php"),
            "<?php\nuse App\\Service\\UserService;\nrequire_once __DIR__ . '/../Support/helpers.php';\nrequire_once __DIR__ . '/../../vendor/autoload.php';\n",
        )
        .unwrap();
    std::fs::write(
        dir.path().join("src/Service/UserService.php"),
        "<?php\nnamespace App\\Service;\nclass UserService {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/Support/helpers.php"),
        "<?php\nfunction helper(): void {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests/ControllerTest.php"),
        "<?php\nuse App\\Http\\Controller;\n",
    )
    .unwrap();

    let graph = build(
        &[
            file_report("src/Http/Controller.php"),
            file_report("src/Service/UserService.php"),
            file_report("src/Support/helpers.php"),
            file_report("tests/ControllerTest.php"),
        ],
        dir.path(),
    );

    assert_eq!(graph.nodes, 4);
    assert_eq!(graph.edges, 3);
    assert_eq!(graph.unresolved_imports, 0);
    assert_eq!(graph.config_errors, 0);
    assert_eq!(graph.config_files, ["composer.json"]);
    let provenance = graph
        .edge_list
        .iter()
        .map(|edge| {
            (
                (edge.source.as_str(), edge.target.as_str()),
                edge.resolver.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        provenance.get(&("src/Http/Controller.php", "src/Service/UserService.php")),
        Some(&"composer-psr-4")
    );
    assert_eq!(
        provenance.get(&("src/Http/Controller.php", "src/Support/helpers.php")),
        Some(&"php-include")
    );
    assert_eq!(
        provenance.get(&("tests/ControllerTest.php", "src/Http/Controller.php")),
        Some(&"composer-psr-4")
    );

    let paths = [
        "src/Http/Controller.php",
        "src/Service/UserService.php",
        "src/Support/helpers.php",
        "tests/ControllerTest.php",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect::<Vec<_>>();
    let impact = impact(
        &paths,
        dir.path(),
        &HashSet::from([PathBuf::from("src/Service/UserService.php")]),
    );
    assert_eq!(impact.direct_dependents, ["src/Http/Controller.php"]);
    assert_eq!(impact.transitive_dependents, ["tests/ControllerTest.php"]);
    assert_eq!(impact.confidence, "high");
}

#[test]
fn rust_modules_uses_and_local_cargo_crates_build_edges_with_provenance() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/service")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "pub mod service;\nuse crate::service::Worker;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/main.rs"),
        "use demo_app::service::Worker;\nfn main() {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/service.rs"),
        "pub mod nested;\npub struct Worker;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/service/nested.rs"),
        "pub struct Nested;\n",
    )
    .unwrap();

    let graph = build(
        &[
            file_report("src/lib.rs"),
            file_report("src/main.rs"),
            file_report("src/service.rs"),
            file_report("src/service/nested.rs"),
        ],
        dir.path(),
    );

    assert_eq!(graph.languages, ["Rust"]);
    assert_eq!(graph.nodes, 4);
    assert_eq!(graph.edges, 3);
    assert_eq!(graph.unresolved_imports, 0);
    assert_eq!(graph.config_errors, 0);
    assert_eq!(graph.config_files, ["Cargo.toml"]);
    let edges = graph
        .edge_list
        .iter()
        .map(|edge| {
            (
                edge.source.as_str(),
                edge.target.as_str(),
                edge.resolver.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert!(edges.contains(&("src/lib.rs", "src/service.rs", "rust-mod")));
    assert!(edges.contains(&("src/main.rs", "src/service.rs", "rust-workspace")));
    assert!(edges.contains(&("src/service.rs", "src/service/nested.rs", "rust-mod")));
}

#[test]
fn rust_inline_test_imports_from_the_same_file_are_not_graph_gaps() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        r"
pub struct Service;
fn helper() {}

#[cfg(test)]
mod tests {
    use super::helper;
    use crate::Service;
}
",
    )
    .unwrap();

    let graph = build(&[file_report("src/lib.rs")], dir.path());

    assert_eq!(graph.nodes, 1);
    assert_eq!(graph.edges, 0);
    assert_eq!(graph.unresolved_imports, 0);
    assert!(graph.cycles.is_empty());
}

#[test]
fn rust_missing_local_use_targets_inside_inline_modules_remain_graph_gaps() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        r"
#[cfg(test)]
mod tests {
    use crate::missing::Service;
}
",
    )
    .unwrap();

    let graph = build(&[file_report("src/lib.rs")], dir.path());

    assert_eq!(graph.edges, 0);
    assert_eq!(graph.unresolved_imports, 1);
}

#[test]
fn rust_crate_paths_inside_nested_files_still_start_at_the_crate_root() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub mod report;\n").unwrap();
    std::fs::write(
        dir.path().join("src/report.rs"),
        r"
pub struct SomeType;

#[cfg(test)]
mod tests {
    use crate::report::SomeType;
    use crate::MissingAtRoot;
}
",
    )
    .unwrap();

    let graph = build(
        &[file_report("src/lib.rs"), file_report("src/report.rs")],
        dir.path(),
    );

    assert_eq!(graph.edges, 1);
    assert_eq!(graph.unresolved_imports, 1);
    assert!(
        !graph
            .edge_list
            .iter()
            .any(|edge| { edge.source == "src/report.rs" && edge.target == "src/lib.rs" })
    );
}

#[test]
fn rust_inline_test_imports_do_not_fall_back_to_the_parent_module() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/report")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub mod report;\n").unwrap();
    std::fs::write(dir.path().join("src/report/mod.rs"), "pub mod table;\n").unwrap();
    std::fs::write(
        dir.path().join("src/report/table.rs"),
        r"
pub fn render() {}

#[cfg(test)]
mod tests {
    use super::render;
}
",
    )
    .unwrap();

    let graph = build(
        &[
            file_report("src/lib.rs"),
            file_report("src/report/mod.rs"),
            file_report("src/report/table.rs"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 2);
    assert_eq!(graph.unresolved_imports, 0);
    assert!(!graph.edge_list.iter().any(|edge| {
        edge.source == "src/report/table.rs" && edge.target == "src/report/mod.rs"
    }));
}

#[test]
fn rust_missing_external_modules_remain_graph_gaps() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "mod missing;\n").unwrap();

    let graph = build(&[file_report("src/lib.rs")], dir.path());

    assert_eq!(graph.edges, 0);
    assert_eq!(graph.unresolved_imports, 1);
}

#[test]
fn rust_missing_local_use_targets_remain_graph_gaps() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/lib.rs"),
        "use crate::missing::Service;\n",
    )
    .unwrap();

    let graph = build(&[file_report("src/lib.rs")], dir.path());

    assert_eq!(graph.edges, 0);
    assert_eq!(graph.unresolved_imports, 1);
}

#[test]
fn javascript_local_resource_imports_are_not_graph_gaps() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/assets")).unwrap();
    std::fs::write(
        dir.path().join("src/main.tsx"),
        "import './index.css';\nimport logo from '@/assets/logo.png';\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/index.css"), "body {}\n").unwrap();
    std::fs::write(dir.path().join("src/assets/logo.png"), []).unwrap();

    let graph = build(&[file_report("src/main.tsx")], dir.path());

    assert_eq!(graph.edges, 0);
    assert_eq!(graph.unresolved_imports, 0);
}

#[test]
fn javascript_missing_source_imports_remain_graph_gaps() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.ts"), "import './missing.ts';\n").unwrap();

    let graph = build(&[file_report("src/main.ts")], dir.path());

    assert_eq!(graph.edges, 0);
    assert_eq!(graph.unresolved_imports, 1);
}

#[test]
fn go_module_imports_resolve_to_a_stable_package_representative() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("cmd/app")).unwrap();
    std::fs::create_dir_all(dir.path().join("internal/store")).unwrap();
    std::fs::write(
        dir.path().join("go.mod"),
        "module example.com/demo\n\ngo 1.24\n",
    )
    .unwrap();
    std::fs::write(
            dir.path().join("cmd/app/main.go"),
            "package main\nimport (\"fmt\"; \"example.com/demo/internal/store\")\nfunc main() { fmt.Println(store.Value) }\n",
        )
        .unwrap();
    std::fs::write(
        dir.path().join("internal/store/store.go"),
        "package store\nconst Value = 1\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("internal/store/helpers.go"),
        "package store\nfunc helper() {}\n",
    )
    .unwrap();

    let graph = build(
        &[
            file_report("cmd/app/main.go"),
            file_report("internal/store/helpers.go"),
            file_report("internal/store/store.go"),
        ],
        dir.path(),
    );

    assert_eq!(graph.languages, ["Go"]);
    assert_eq!(graph.nodes, 3);
    assert_eq!(graph.edges, 1);
    assert_eq!(graph.unresolved_imports, 0);
    assert_eq!(graph.config_errors, 0);
    assert_eq!(graph.config_files, ["go.mod"]);
    assert!(graph.orphans.is_empty());
    assert_eq!(
        (
            graph.edge_list[0].source.as_str(),
            graph.edge_list[0].target.as_str(),
            graph.edge_list[0].resolver.as_str(),
        ),
        ("cmd/app/main.go", "internal/store/store.go", "go-module")
    );
}

#[test]
fn php_psr_zero_and_invalid_composer_configs_are_accounted_for() {
    let valid = tempdir().unwrap();
    std::fs::create_dir_all(valid.path().join("legacy/Legacy/Service")).unwrap();
    std::fs::write(
        valid.path().join("composer.json"),
        r#"{"autoload":{"psr-0":{"Legacy_":"legacy/"}}}"#,
    )
    .unwrap();
    std::fs::write(
        valid.path().join("consumer.php"),
        "<?php\nuse Legacy_Service_User;\n",
    )
    .unwrap();
    std::fs::write(
        valid.path().join("legacy/Legacy/Service/User.php"),
        "<?php\nclass Legacy_Service_User {}\n",
    )
    .unwrap();
    let graph = build(
        &[
            file_report("consumer.php"),
            file_report("legacy/Legacy/Service/User.php"),
        ],
        valid.path(),
    );
    assert_eq!(graph.edges, 1);
    assert_eq!(graph.edge_list[0].resolver, "composer-psr-0");

    let invalid = tempdir().unwrap();
    std::fs::write(invalid.path().join("composer.json"), "{ invalid").unwrap();
    std::fs::write(invalid.path().join("index.php"), "<?php\n").unwrap();
    let graph = build(&[file_report("index.php")], invalid.path());
    assert_eq!(graph.config_errors, 1);
    assert_eq!(graph.config_files, Vec::<String>::new());
}
