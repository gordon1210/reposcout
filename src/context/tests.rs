use super::*;
use crate::graph::GraphFileSignal;
use crate::model::{Churn, Complexity, FunctionComplexity, Halstead};
use std::collections::BTreeMap;

fn file(path: &str, tokens: usize) -> FileReport {
    FileReport {
        path: PathBuf::from(path),
        language: "Rust".to_string(),
        bytes: 0,
        tokens,
        loc: 1,
        sloc: 1,
        comment_lines: 0,
        comment_ratio: 0.0,
        line_metrics_approximate: false,
        complexity: None,
        imports: Vec::new(),
        markers: BTreeMap::new(),
        marker_occurrences: Vec::new(),
        churn: None,
        approximate: false,
        symbols: None,
        skip_hint: None,
        has_inline_tests: false,
    }
}

#[test]
fn selection_never_exceeds_token_or_file_budgets() {
    let files = [
        file("src/main.rs", 60),
        file("src/lib.rs", 40),
        file("src/extra.rs", 20),
    ];
    let cfg = Config {
        context: true,
        context_budget: 70,
        context_max_files: 2,
        ..Config::default()
    };

    let plan = build(
        &files,
        &[],
        &BTreeMap::new(),
        None,
        Path::new("/repo"),
        &cfg,
        None,
    );

    assert!(plan.selected_tokens <= 70);
    assert!(plan.files.len() <= 2);
    assert_eq!(plan.selected_tokens, 60);
    assert_eq!(
        plan.files
            .iter()
            .map(|file| file.path.as_path())
            .collect::<Vec<_>>(),
        [Path::new("src/lib.rs"), Path::new("src/extra.rs")]
    );
}

#[test]
fn focus_prioritizes_direct_graph_neighbors_and_matching_tests() {
    let files = [
        file("src/focus.ts", 20),
        file("src/dependency.ts", 20),
        file("src/dependent.ts", 20),
        file("tests/focus.test.ts", 20),
        file("src/unrelated.ts", 20),
    ];
    let mut graph = GraphSignals {
        languages: vec!["TypeScript".to_string()],
        ..GraphSignals::default()
    };
    graph.files.insert(
        "src/focus.ts".to_string(),
        GraphFileSignal {
            dependencies: vec!["src/dependency.ts".to_string()],
            dependents: vec!["src/dependent.ts".to_string()],
            ..GraphFileSignal::default()
        },
    );
    let cfg = Config {
        context: true,
        context_budget: 80,
        context_max_files: 4,
        context_focus: vec![PathBuf::from("src/focus.ts")],
        ..Config::default()
    };

    let plan = build(
        &files,
        &[],
        &BTreeMap::new(),
        Some(&graph),
        Path::new("/repo"),
        &cfg,
        None,
    );
    let selected = plan
        .files
        .iter()
        .map(|file| file.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert_eq!(selected[0], "src/focus.ts");
    assert!(selected.contains(&"src/dependency.ts".to_string()));
    assert!(selected.contains(&"src/dependent.ts".to_string()));
    assert!(selected.contains(&"tests/focus.test.ts".to_string()));
    assert!(!selected.contains(&"src/unrelated.ts".to_string()));
}

#[test]
fn focus_resolves_relative_to_a_nested_scan_target() {
    let files = [
        file("packages/app/math.rs", 20),
        file("packages/app/sibling.rs", 20),
    ];
    let cfg = Config {
        context: true,
        context_focus: vec![PathBuf::from("math.rs")],
        ..Config::default()
    };

    let plan = build_for_target(
        &files,
        &[],
        &BTreeMap::new(),
        None,
        PlanningPaths {
            root: Path::new("/repo"),
            target: Path::new("/repo/packages/app"),
        },
        &cfg,
        None,
    )
    .unwrap();

    assert_eq!(plan.focus, [PathBuf::from("packages/app/math.rs")]);
    assert!(plan.unmatched_focus.is_empty());
    assert_eq!(plan.files[0].path, PathBuf::from("packages/app/math.rs"));
}

#[test]
fn unmatched_focus_is_reported_without_inventing_a_seed() {
    let files = [file("packages/app/math.rs", 20)];
    let cfg = Config {
        context: true,
        context_focus: vec![PathBuf::from("missing.rs")],
        ..Config::default()
    };

    let plan = build_for_target(
        &files,
        &[],
        &BTreeMap::new(),
        None,
        PlanningPaths {
            root: Path::new("/repo"),
            target: Path::new("/repo/packages/app"),
        },
        &cfg,
        None,
    )
    .unwrap();

    assert!(plan.focus.is_empty());
    assert_eq!(plan.unmatched_focus, [PathBuf::from("missing.rs")]);
    assert!(
        plan.files[0]
            .reasons
            .iter()
            .all(|reason| reason != "focus path")
    );
}

#[test]
fn ambiguous_target_relative_focus_is_rejected() {
    let files = [file("math.rs", 20), file("packages/app/math.rs", 20)];
    let cfg = Config {
        context: true,
        context_focus: vec![PathBuf::from("math.rs")],
        ..Config::default()
    };

    let error = build_for_target(
        &files,
        &[],
        &BTreeMap::new(),
        None,
        PlanningPaths {
            root: Path::new("/repo"),
            target: Path::new("/repo/packages/app"),
        },
        &cfg,
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("ambiguous"), "error was: {error}");
    assert!(error.contains("math.rs"), "error was: {error}");
    assert!(error.contains("packages/app/math.rs"), "error was: {error}");
}

#[test]
fn generated_files_are_skipped_unless_explicitly_focused() {
    let mut generated = file("src/generated.rs", 10);
    generated.skip_hint = Some("generated".to_string());
    let normal = file("src/lib.rs", 10);
    let mut cfg = Config {
        context: true,
        ..Config::default()
    };

    let plan = build(
        &[generated.clone(), normal.clone()],
        &[],
        &BTreeMap::new(),
        None,
        Path::new("/repo"),
        &cfg,
        None,
    );
    assert!(plan.files.iter().all(|file| file.path != generated.path));

    cfg.context_focus = vec![generated.path.clone()];
    let focused = build(
        &[generated, normal],
        &[],
        &BTreeMap::new(),
        None,
        Path::new("/repo"),
        &cfg,
        None,
    );
    assert_eq!(focused.files[0].path, PathBuf::from("src/generated.rs"));
}

#[test]
fn focus_prefers_same_directory_when_graph_signals_are_unavailable() {
    let files = [
        file("src/focus.rs", 20),
        file("src/sibling.rs", 20),
        file("other/unrelated.rs", 20),
    ];
    let cfg = Config {
        context: true,
        context_budget: 40,
        context_max_files: 2,
        context_focus: vec![PathBuf::from("src/focus.rs")],
        ..Config::default()
    };

    let plan = build(
        &files,
        &[],
        &BTreeMap::new(),
        None,
        Path::new("/repo"),
        &cfg,
        None,
    );

    assert_eq!(plan.files[0].path, PathBuf::from("src/focus.rs"));
    assert_eq!(plan.files[1].path, PathBuf::from("src/sibling.rs"));
    assert!(
        plan.files[1]
            .reasons
            .contains(&"shares focus directory".to_string())
    );
}

#[test]
fn repository_instructions_outrank_ordinary_focus_siblings() {
    let files = [
        file("src/focus.rs", 10),
        file("src/sibling.rs", 10),
        file("AGENTS.md", 10),
    ];
    let cfg = Config {
        context: true,
        context_budget: 20,
        context_max_files: 2,
        context_focus: vec![PathBuf::from("src/focus.rs")],
        ..Config::default()
    };

    let plan = build(
        &files,
        &[],
        &BTreeMap::new(),
        None,
        Path::new("/repo"),
        &cfg,
        None,
    );

    assert_eq!(plan.files[0].path, PathBuf::from("src/focus.rs"));
    assert_eq!(plan.files[1].path, PathBuf::from("AGENTS.md"));
    assert!(
        plan.files[1]
            .reasons
            .contains(&"repository instructions".to_string())
    );
}

#[test]
fn risk_churn_and_complexity_produce_explainable_reasons() {
    let mut risky = file("src/risky.rs", 10);
    risky.churn = Some(Churn {
        commits: 9,
        authors: 2,
        ..Churn::default()
    });
    risky.complexity = Some(Complexity {
        cyclomatic: 15,
        cognitive: 0,
        max_nesting: 0,
        halstead: Halstead::default(),
        maintainability_index: 0.0,
        functions: Vec::new(),
    });
    let risk = RiskEntry {
        path: "src/risky.rs".to_string(),
        score: 0.8,
        ..RiskEntry::default()
    };
    let cfg = Config {
        context: true,
        ..Config::default()
    };

    let plan = build(
        &[risky],
        &[risk],
        &BTreeMap::new(),
        None,
        Path::new("/repo"),
        &cfg,
        None,
    );
    let reasons = &plan.files[0].reasons;

    assert!(reasons.iter().any(|reason| reason.contains("high risk")));
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("active history"))
    );
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("complex control"))
    );
}

#[test]
fn change_seeds_select_dependencies_tests_and_transitive_dependents_with_evidence() {
    let files = [
        file("src/changed.ts", 10),
        file("src/dependency.ts", 10),
        file("src/direct.ts", 10),
        file("src/transitive.ts", 10),
        file("tests/changed.test.ts", 10),
        file("src/unrelated.ts", 10),
    ];
    let mut graph = GraphSignals {
        languages: vec!["TypeScript".to_string()],
        ..GraphSignals::default()
    };
    graph.files.insert(
        "src/changed.ts".to_string(),
        GraphFileSignal {
            dependencies: vec!["src/dependency.ts".to_string()],
            dependents: vec!["src/direct.ts".to_string()],
            dependency_resolvers: BTreeMap::from([(
                "src/dependency.ts".to_string(),
                "relative".to_string(),
            )]),
            dependent_resolvers: BTreeMap::from([(
                "src/direct.ts".to_string(),
                "relative".to_string(),
            )]),
            ..GraphFileSignal::default()
        },
    );
    graph.files.insert(
        "src/direct.ts".to_string(),
        GraphFileSignal {
            dependents: vec!["src/transitive.ts".to_string()],
            dependent_resolvers: BTreeMap::from([(
                "src/transitive.ts".to_string(),
                "package-exports".to_string(),
            )]),
            ..GraphFileSignal::default()
        },
    );
    let changes = ChangeSeeds {
        scope: "working".to_string(),
        paths: HashSet::from([PathBuf::from("src/changed.ts")]),
    };
    let cfg = Config {
        context: true,
        context_budget: 50,
        context_max_files: 5,
        ..Config::default()
    };

    let plan = build(
        &files,
        &[],
        &BTreeMap::new(),
        Some(&graph),
        Path::new("/repo"),
        &cfg,
        Some(&changes),
    );
    let selected = plan
        .files
        .iter()
        .map(|file| file.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert_eq!(plan.change_scope.as_deref(), Some("working"));
    assert_eq!(plan.changed_files, [PathBuf::from("src/changed.ts")]);
    assert_eq!(selected[0], "src/changed.ts");
    for expected in [
        "src/dependency.ts",
        "src/direct.ts",
        "src/transitive.ts",
        "tests/changed.test.ts",
    ] {
        assert!(
            selected.contains(&expected.to_string()),
            "missing {expected}"
        );
    }
    assert!(!selected.contains(&"src/unrelated.ts".to_string()));

    let evidence = |path: &str, role: &str| {
        plan.files
            .iter()
            .find(|file| file.path == Path::new(path))
            .and_then(|file| file.evidence.iter().find(|item| item.role == role))
            .cloned()
            .unwrap_or_else(|| panic!("missing {role} evidence for {path}"))
    };
    let dependency = evidence("src/dependency.ts", "dependency");
    assert_eq!(dependency.confidence, "high");
    assert_eq!(dependency.resolver.as_deref(), Some("relative"));
    let direct = evidence("src/direct.ts", "dependent");
    assert_eq!(direct.confidence, "high");
    assert_eq!(direct.distance, Some(1));
    let transitive = evidence("src/transitive.ts", "dependent");
    assert_eq!(transitive.confidence, "partial");
    assert_eq!(transitive.distance, Some(2));
    assert_eq!(
        evidence("tests/changed.test.ts", "matching-test").confidence,
        "partial"
    );
}

#[test]
fn structural_outlines_have_a_hard_payload_bound_and_report_omissions() {
    let files = [file("src/lib.rs", 10)];
    let symbols = (0..30)
        .map(|index| SymbolOutline {
            name: format!("PublicType{index}"),
            kind: "type".to_string(),
            signature: format!("pub struct PublicType{index}<T: LongTraitName>"),
            line: index + 1,
            exported: true,
            reasons: vec!["exported/public declaration".to_string()],
        })
        .collect::<Vec<_>>();
    let outlines = BTreeMap::from([(PathBuf::from("src/lib.rs"), symbols)]);
    let cfg = Config {
        context: true,
        context_budget: 100,
        context_max_files: 1,
        ..Config::default()
    };

    let plan = build(&files, &[], &outlines, None, Path::new("/repo"), &cfg, None);
    let retained = &plan.files[0].symbols;
    let measured = retained.iter().map(outline_payload_bytes).sum::<usize>();

    assert!(!retained.is_empty());
    assert!(retained.len() <= MAX_OUTLINE_SYMBOLS_PER_FILE);
    assert!(measured <= MAX_OUTLINE_BYTES_PER_FILE);
    assert_eq!(plan.outline_symbols, retained.len());
    assert_eq!(plan.outline_bytes, measured);
    assert!(plan.outline_omitted_symbols > 0);
    assert!(
        retained
            .iter()
            .all(|symbol| !symbol.reasons.is_empty() && !symbol.signature.contains('{'))
    );
}

#[test]
fn oversized_focus_retains_a_bounded_outline_without_spending_source_tokens() {
    let files = [file("src/large.rs", 500)];
    let outlines = BTreeMap::from([(
        PathBuf::from("src/large.rs"),
        vec![SymbolOutline {
            name: "ImportantType".to_string(),
            kind: "struct".to_string(),
            signature: "pub struct ImportantType".to_string(),
            line: 1,
            exported: true,
            reasons: Vec::new(),
        }],
    )]);
    let cfg = Config {
        context: true,
        context_budget: 100,
        context_focus: vec![PathBuf::from("src/large.rs")],
        ..Config::default()
    };

    let plan = build(&files, &[], &outlines, None, Path::new("/repo"), &cfg, None);

    assert!(plan.files.is_empty());
    assert_eq!(plan.selected_tokens, 0);
    assert_eq!(plan.outline_only.len(), 1);
    assert_eq!(plan.outline_only[0].path, PathBuf::from("src/large.rs"));
    assert_eq!(plan.outline_only[0].source_tokens, 500);
    assert_eq!(plan.outline_only[0].symbols[0].name, "ImportantType");
    assert_eq!(plan.outline_symbols, 1);
    assert!(plan.outline_bytes > 0);
    assert_eq!(plan.omitted[0].reason, "file exceeds total token budget");
}

#[test]
fn change_plans_prefer_nearby_risk_over_ordinary_siblings() {
    let files = [
        file("src/changed.rs", 10),
        file("src/nearby_risk.rs", 10),
        file("src/plain.rs", 10),
        file("other/distant_risk.rs", 10),
    ];
    let risks = [
        RiskEntry {
            path: "src/nearby_risk.rs".to_string(),
            score: 0.8,
            ..RiskEntry::default()
        },
        RiskEntry {
            path: "other/distant_risk.rs".to_string(),
            score: 0.8,
            ..RiskEntry::default()
        },
    ];
    let changes = ChangeSeeds {
        scope: "working".to_string(),
        paths: HashSet::from([PathBuf::from("src/changed.rs")]),
    };
    let cfg = Config {
        context: true,
        context_budget: 20,
        context_max_files: 2,
        ..Config::default()
    };

    let plan = build(
        &files,
        &risks,
        &BTreeMap::new(),
        None,
        Path::new("/repo"),
        &cfg,
        Some(&changes),
    );

    assert_eq!(plan.files[0].path, PathBuf::from("src/changed.rs"));
    assert_eq!(plan.files[1].path, PathBuf::from("src/nearby_risk.rs"));
    assert!(
        plan.files[1]
            .reasons
            .contains(&"nearby elevated-risk code".to_string())
    );
}

#[test]
fn complexity_reasons_attach_only_to_the_matching_callable_outline() {
    let mut source = file("src/service.ts", 10);
    source.complexity = Some(Complexity {
        functions: vec![FunctionComplexity {
            name: "run".to_string(),
            line: 1,
            cyclomatic: 30,
            ..FunctionComplexity::default()
        }],
        ..Complexity::default()
    });
    let outlines = BTreeMap::from([(
        PathBuf::from("src/service.ts"),
        vec![
            SymbolOutline {
                name: "Service".to_string(),
                kind: "class".to_string(),
                line: 1,
                exported: true,
                ..SymbolOutline::default()
            },
            SymbolOutline {
                name: "Service.run".to_string(),
                kind: "method".to_string(),
                line: 1,
                exported: true,
                ..SymbolOutline::default()
            },
        ],
    )]);
    let cfg = Config {
        context: true,
        max_complexity: 20,
        ..Config::default()
    };

    let plan = build(
        &[source],
        &[],
        &outlines,
        None,
        Path::new("/repo"),
        &cfg,
        None,
    );
    let symbols = &plan.files[0].symbols;
    let class = symbols
        .iter()
        .find(|symbol| symbol.name == "Service")
        .unwrap();
    let method = symbols
        .iter()
        .find(|symbol| symbol.name == "Service.run")
        .unwrap();

    assert!(!symbol_has_finding(class));
    assert!(symbol_has_finding(method));
}

#[test]
fn evidence_confidence_distinguishes_configured_and_inferred_resolvers() {
    for resolver in [
        "relative",
        "python-relative",
        "tsconfig-paths",
        "package-exports",
        "composer-psr-4",
        "composer-psr-0",
        "php-include",
    ] {
        assert_eq!(
            graph_evidence("dependency", 1, Some(resolver.to_string())).confidence,
            "high",
            "{resolver} should be precise"
        );
    }
    for resolver in [
        "python-absolute",
        "python-src-root",
        "package-subpath",
        "package-index",
        "heuristic-alias",
        "php-namespace-heuristic",
    ] {
        assert_eq!(
            graph_evidence("dependency", 1, Some(resolver.to_string())).confidence,
            "partial",
            "{resolver} should remain heuristic"
        );
    }
    assert_eq!(
        graph_evidence("dependent", 2, Some("relative".to_string())).confidence,
        "partial"
    );
}
