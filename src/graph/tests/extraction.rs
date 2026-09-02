use super::*;
use crate::graph::source::{extract_js_specifiers, extract_py_specifiers, extract_specifiers};

// -- extract_js_specifiers -----------------------------------------------

#[test]
fn js_specifiers_basic() {
    let src = "import x from './a';\nconst y = require(\"../b/c\");\nimport('@/lib/d');\nimport 'side-effect';";
    let specs = extract_js_specifiers(src);
    assert!(specs.contains(&"./a".to_string()), "missing ./a: {specs:?}");
    assert!(
        specs.contains(&"../b/c".to_string()),
        "missing ../b/c: {specs:?}"
    );
    assert!(
        specs.contains(&"@/lib/d".to_string()),
        "missing @/lib/d: {specs:?}"
    );
    assert!(
        specs.contains(&"side-effect".to_string()),
        "missing side-effect: {specs:?}"
    );
}

#[test]
fn js_specifiers_no_false_ident() {
    // `from` inside an identifier must not be extracted.
    let src = "const transformFrom = ''; const x = platformFrom;";
    let specs = extract_js_specifiers(src);
    // Neither of those variable names should produce a specifier.
    assert!(specs.is_empty(), "unexpected specifiers: {specs:?}");
}

#[test]
fn js_specifiers_are_unicode_safe_and_ast_scoped() {
    let src = r#"
const café = "require('./in-string')";
// import './in-comment';
client.require('./member');
const local = require ( './local' );
export { value } from "./reexport";
"#;
    assert_eq!(
        extract_js_specifiers(src),
        vec!["./local".to_string(), "./reexport".to_string()]
    );
}

// -- extract_py_specifiers -----------------------------------------------

#[test]
fn py_specifiers_basic() {
    let src = "from .foo import x\nimport os\nfrom ..pkg.mod import y";
    let specs = extract_py_specifiers(src);
    assert!(
        specs.contains(&".foo".to_string()),
        "missing .foo: {specs:?}"
    );
    assert!(specs.contains(&"os".to_string()), "missing os: {specs:?}");
    assert!(
        specs.contains(&"..pkg.mod".to_string()),
        "missing ..pkg.mod: {specs:?}"
    );
}

#[test]
fn py_current_package_specifiers_handle_star_and_parenthesized_names() {
    let specs = extract_py_specifiers("from . import *\nfrom . import (alpha, beta as renamed)\n");

    assert_eq!(specs, vec![".", ".alpha", ".beta"]);
}

#[test]
fn py_current_package_specifiers_handle_multiline_parenthesized_names() {
    let specs = extract_py_specifiers("from . import (\n    alpha,\n    beta as renamed,\n)\n");

    assert_eq!(specs, vec![".alpha", ".beta"]);
}

#[test]
fn py_specifiers_ignore_comments_and_strings() {
    let source = r#"
example = "from .fake import value"
# from .comment import value
from . import (
    alpha,
    beta as renamed,
)
"#;
    assert_eq!(extract_py_specifiers(source), vec![".alpha", ".beta"]);
}

#[test]
fn php_specifiers_are_ast_scoped_and_keep_static_include_kinds() {
    let source = r#"<?php
use App\Service\{UserService, AuditService as Audit};
use function App\Support\helper;
$example = "use Fake\\Ignored;";
// require __DIR__ . '/ignored.php';
require_once __DIR__ . '/../bootstrap.php';
include 'config/routes.php';
include $dynamic;
"#;
    let extraction = extract_specifiers(FirstClass::Php, source);

    assert_eq!(extraction.parse_errors, 0);
    assert_eq!(
        extraction.specifiers,
        vec![
            ImportSpecifier::PhpNamespace("App\\Service\\UserService".into()),
            ImportSpecifier::PhpNamespace("App\\Service\\AuditService".into()),
            ImportSpecifier::PhpNamespace("App\\Support\\helper".into()),
            ImportSpecifier::PhpInclude(StaticInclude::DirectoryRelative {
                parents: 0,
                path: "/../bootstrap.php".into(),
            }),
            ImportSpecifier::PhpInclude(StaticInclude::Literal("config/routes.php".into())),
        ]
    );
}

#[test]
fn rust_specifiers_keep_module_context_and_expand_grouped_uses() {
    let source = r#"
mod api;
#[path = "support/custom.rs"]
mod custom;
use crate::{domain::Service, util};
mod inline {
    mod child;
    use super::api::Client;
}
"#;
    let extraction = extract_specifiers(FirstClass::Rust, source);

    assert_eq!(extraction.parse_errors, 0);
    assert_eq!(
        extraction.specifiers,
        vec![
            ImportSpecifier::Rust(RustImport::Module {
                name: "api".into(),
                path: None,
                inline_modules: vec![],
            }),
            ImportSpecifier::Rust(RustImport::Module {
                name: "custom".into(),
                path: Some("support/custom.rs".into()),
                inline_modules: vec![],
            }),
            ImportSpecifier::Rust(RustImport::Use {
                path: "crate::domain::Service".into(),
                inline_modules: vec![],
            }),
            ImportSpecifier::Rust(RustImport::Use {
                path: "crate::util".into(),
                inline_modules: vec![],
            }),
            ImportSpecifier::Rust(RustImport::Module {
                name: "child".into(),
                path: None,
                inline_modules: vec!["inline".into()],
            }),
            ImportSpecifier::Rust(RustImport::Use {
                path: "super::api::Client".into(),
                inline_modules: vec!["inline".into()],
            }),
        ]
    );
}

#[test]
fn go_specifiers_are_ast_scoped() {
    let source = r#"
package main
import (
    "example.com/project/internal/store"
    alias "example.com/project/pkg/api"
)
var ignored = "example.com/project/not-an-import"
"#;
    let extraction = extract_specifiers(FirstClass::Go, source);

    assert_eq!(extraction.parse_errors, 0);
    assert_eq!(
        extraction.specifiers,
        vec![
            ImportSpecifier::GoPackage("example.com/project/internal/store".into()),
            ImportSpecifier::GoPackage("example.com/project/pkg/api".into()),
        ]
    );
}

#[test]
fn malformed_graph_source_records_parse_errors() {
    let extraction = extract_specifiers(FirstClass::JavaScript, "import { from './x';");
    assert!(extraction.parse_errors > 0);
}

#[test]
fn mixed_graph_retains_every_first_class_language() {
    let dir = tempdir().unwrap();
    for (path, source) in [
        ("sample.rs", "pub fn value() {}\n"),
        ("sample.py", "VALUE = 1\n"),
        ("sample.js", "export const value = 1;\n"),
        ("sample.ts", "export const value: number = 1;\n"),
        ("sample.tsx", "export const value = <div />;\n"),
        ("sample.go", "package sample\nconst Value = 1\n"),
        ("sample.php", "<?php\nconst VALUE = 1;\n"),
    ] {
        std::fs::write(dir.path().join(path), source).unwrap();
    }
    let graph = build(
        &[
            file_report("sample.rs"),
            file_report("sample.py"),
            file_report("sample.js"),
            file_report("sample.ts"),
            file_report("sample.tsx"),
            file_report("sample.go"),
            file_report("sample.php"),
        ],
        dir.path(),
    );

    assert_eq!(graph.nodes, 7);
    assert_eq!(
        graph.languages,
        [
            "Go",
            "JavaScript",
            "PHP",
            "Python",
            "Rust",
            "TSX",
            "TypeScript",
        ]
    );
}

#[test]
fn graph_projects_explicit_type_relationships_separately_from_imports() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/Http")).unwrap();
    std::fs::write(
        dir.path().join("src/Http/HttpClient.php"),
        "<?php namespace App\\Http; abstract class HttpClient {}\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/Http/Guzzle.php"),
        "<?php namespace App\\Http; class Guzzle extends HttpClient {}\n",
    )
    .unwrap();

    let graph = build(
        &[
            file_report("src/Http/HttpClient.php"),
            file_report("src/Http/Guzzle.php"),
        ],
        dir.path(),
    );

    assert_eq!(graph.edges, 0, "type edges must not masquerade as imports");
    assert_eq!(graph.symbol_edges.len(), 1);
    assert_eq!(graph.symbol_edges[0].relation, "extends");
    assert_eq!(graph.unresolved_symbol_relations, 0);
    let base = graph
        .files
        .iter()
        .find(|file| file.path.ends_with("HttpClient.php"))
        .unwrap();
    assert_eq!(base.symbol_reach.as_ref().unwrap().name, "HttpClient");
    assert_eq!(base.symbol_reach.as_ref().unwrap().fan_in, 1);
}

#[test]
fn collect_resolver_configs_follows_tsconfig_extends_and_references() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("configs")).unwrap();
    std::fs::create_dir_all(dir.path().join("packages/shared")).unwrap();
    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{
  "extends": "./configs/base.json",
  "references": [{ "path": "./packages/shared" }],
  "compilerOptions": { "baseUrl": "." }
}
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("configs/base.json"),
        r#"{ "compilerOptions": { "strict": true }, "extends": "./strict.json" }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("configs/strict.json"),
        r#"{ "compilerOptions": { "noImplicitAny": true } }"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("packages/shared/tsconfig.json"),
        r#"{ "compilerOptions": { "composite": true } }"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("index.ts"), "export const value = 1;\n").unwrap();

    let configs = collect_resolver_configs(
        dir.path(),
        &[PathBuf::from("index.ts")],
        &GraphReadLimits::default(),
    );
    assert!(configs.contains_key("tsconfig.json"));
    assert!(configs.contains_key("configs/base.json"));
    assert!(configs.contains_key("configs/strict.json"));
    assert!(configs.contains_key("packages/shared/tsconfig.json"));
}

#[test]
fn oversized_package_json_is_not_loaded_by_the_resolver() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("index.ts"), "export const value = 1;\n").unwrap();
    // Larger than the graph budget; must not be fully loaded.
    std::fs::write(dir.path().join("package.json"), vec![b'x'; 256 * 1024]).unwrap();
    let files = [file_report("index.ts")];
    let limits = GraphReadLimits {
        max_file_bytes: 1024,
        max_total_bytes: 2048,
        max_files: 10,
        facts_only_sources: false,
        deadline: None,
    };
    let report = build_with_limits(&files, dir.path(), limits, None);
    assert_eq!(report.nodes, 1);
    assert!(
        report
            .config_files
            .iter()
            .all(|path| path != "package.json"),
        "oversized package.json must not contribute resolver config"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_resolver_config_is_rejected() {
    let dir = tempdir().unwrap();
    let external = dir.path().join("outside-package.json");
    std::fs::write(&external, r#"{"name":"evil"}"#).unwrap();
    std::os::unix::fs::symlink(&external, dir.path().join("package.json")).unwrap();
    std::fs::write(dir.path().join("index.ts"), "export const value = 1;\n").unwrap();
    let files = [file_report("index.ts")];
    let report = build_with_limits(&files, dir.path(), GraphReadLimits::default(), None);
    assert!(
        report
            .config_files
            .iter()
            .all(|path| path != "package.json"),
        "symlink package.json must not be followed"
    );
}

#[test]
fn cached_source_facts_produce_the_same_graph_without_rereading_sources() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let sources = [
        ("src/base.ts", "export interface Service {}\n"),
        (
            "src/app.ts",
            "import { Service } from './base';\nexport class App implements Service {}\n",
        ),
    ];
    for (path, source) in sources {
        std::fs::write(dir.path().join(path), source).unwrap();
    }
    let files = [file_report("src/base.ts"), file_report("src/app.ts")];
    let expected = analyze_with_query(&files, dir.path(), &[], GraphDirection::Both, 1).report;
    let facts = sources
        .into_iter()
        .map(|(path, source)| {
            (
                PathBuf::from(path),
                extract_source_facts(FirstClass::TypeScript, path, source),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for (path, _) in sources {
        std::fs::remove_file(dir.path().join(path)).unwrap();
    }

    let actual = analyze_with_query_facts(
        &files,
        dir.path(),
        &facts,
        None,
        GraphReadLimits::default(),
        &[],
        GraphDirection::Both,
        1,
    )
    .report;

    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn impact_fallback_reuses_facts_and_honors_limits_and_deadlines() {
    let dir = tempdir().unwrap();
    let changed_source = "export const value = 1;\n";
    let importer_source = "import { value } from './changed';\nexport const imported = value;\n";
    std::fs::write(dir.path().join("changed.js"), changed_source).unwrap();
    std::fs::write(dir.path().join("importer.js"), importer_source).unwrap();
    let paths = [PathBuf::from("changed.js"), PathBuf::from("importer.js")];
    let changed = HashSet::from([PathBuf::from("changed.js")]);
    let facts = BTreeMap::from([(
        PathBuf::from("changed.js"),
        extract_source_facts(FirstClass::JavaScript, "changed.js", changed_source),
    )]);
    std::fs::remove_file(dir.path().join("changed.js")).unwrap();

    let cases = [
        GraphReadLimits {
            max_file_bytes: 32,
            max_total_bytes: 32,
            max_files: 1,
            facts_only_sources: false,
            deadline: None,
        },
        GraphReadLimits {
            deadline: Some(std::time::Instant::now()),
            ..GraphReadLimits::default()
        },
    ];
    for limits in cases {
        let analysis = analyze_paths_with_fallback_facts(
            &paths,
            dir.path(),
            &HashSet::new(),
            &facts,
            None,
            limits,
        );
        let impact = impact_from_analysis(&analysis, &changed);

        assert_eq!(impact.graph_changed_files, ["changed.js"]);
        assert!(impact.direct_dependents.is_empty());
        assert_eq!(impact.confidence, "partial");
    }
}

#[test]
fn parse_errors_reduce_impact_confidence() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("changed.js"), "import { from './dep';\n").unwrap();
    std::fs::write(dir.path().join("dep.js"), "export const value = 1;\n").unwrap();
    let paths = vec![PathBuf::from("changed.js"), PathBuf::from("dep.js")];
    let changed = HashSet::from([PathBuf::from("changed.js")]);

    let impact = impact(&paths, dir.path(), &changed);

    assert!(impact.parse_errors > 0);
    assert_eq!(impact.confidence, "partial");
}
