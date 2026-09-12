#![allow(
    clippy::unwrap_used,
    reason = "tests fail immediately for invalid synthetic setup and assertions"
)]

use reposcout::context::definitions::{
    DefinitionPlanInput, DefinitionPlanLimits, DefinitionSeed, DefinitionSelector, plan,
};
use reposcout::model::{
    DefinitionEnvironment, DefinitionFact, DefinitionFacts, DefinitionPlanningFact,
    DefinitionPlanningFacts, DefinitionStatus, SourceRevision, SourceSpan, SymbolOutline,
};

fn input(path: &str, definitions: &[(&str, usize, usize, usize)]) -> DefinitionPlanInput {
    DefinitionPlanInput {
        path: path.into(),
        snapshot: SourceRevision::Worktree,
        definitions: DefinitionFacts {
            status: DefinitionStatus::Available,
            definitions: definitions
                .iter()
                .map(|&(name, start, end, _)| {
                    let span = SourceSpan {
                        start_byte: start,
                        end_byte: end,
                        start_line: start + 1,
                        end_line: end,
                    };
                    DefinitionFact {
                        symbol: SymbolOutline {
                            name: name.into(),
                            kind: "function".into(),
                            signature: String::new(),
                            line: start + 1,
                            exported: true,
                            reasons: Vec::new(),
                        },
                        declaration_span: span,
                        source_span: Some(span),
                    }
                })
                .collect(),
        },
        planning: DefinitionPlanningFacts {
            sha256: "a".repeat(64),
            encoding: "o200k_base".into(),
            definitions: definitions
                .iter()
                .enumerate()
                .map(|(definition, &(_, _, _, tokens))| DefinitionPlanningFact {
                    definition,
                    tokens,
                    environment: Vec::new(),
                    gaps: Vec::new(),
                })
                .collect(),
            omitted_definitions: 0,
        },
    }
}

fn seed(path: &str, name: &str) -> DefinitionSeed {
    DefinitionSeed {
        path: path.into(),
        snapshot: SourceRevision::Worktree,
        selector: DefinitionSelector::Symbol(name.into()),
    }
}

fn limits(tokens: usize) -> DefinitionPlanLimits {
    DefinitionPlanLimits {
        token_budget: tokens,
        ..DefinitionPlanLimits::default()
    }
}

#[test]
fn small_definitions_from_large_files_share_one_budget_and_keep_seed_order() {
    let files = [
        input(
            "a.rs",
            &[
                ("large_a", 0, 1_000_000, 100_000),
                ("small_a", 1_000_001, 1_000_010, 8),
            ],
        ),
        input(
            "b.rs",
            &[
                ("large_b", 0, 1_000_000, 100_000),
                ("small_b", 1_000_001, 1_000_010, 9),
            ],
        ),
    ];
    let seeds = [
        seed("b.rs", "small_b"),
        seed("a.rs", "small_a"),
        seed("b.rs", "small_b"),
    ];
    let report = plan(&files, &seeds, &limits(17));
    assert_eq!(
        report
            .selected
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["small_b", "small_a"]
    );
    assert_eq!(report.selected_tokens, 17);
    assert_eq!(report.selected_files, 2);
    assert_eq!(report.candidate_definitions, 2);
    assert_eq!(report.input_definitions, 4);
    assert_eq!(report.omitted_definitions, 0);
    assert!(report.selected.iter().all(|item| item.role == "direct"));
    assert_eq!(report.selected[0].file, report.files[1].id);
    let repeated = plan(&files, &seeds, &limits(17));
    assert_eq!(
        serde_json::to_value(report).unwrap(),
        serde_json::to_value(repeated).unwrap()
    );
}

#[test]
fn containing_and_shared_wrapper_spans_count_once_in_either_seed_order() {
    let mut file = input(
        "a.rs",
        &[
            ("outer", 0, 100, 20),
            ("inner", 20, 40, 8),
            ("alias", 20, 40, 8),
        ],
    );
    file.definitions.definitions[2].declaration_span.start_byte = 21;
    for names in [["inner", "alias", "outer"], ["outer", "inner", "alias"]] {
        let seeds = names.map(|name| seed("a.rs", name));
        let report = plan(std::slice::from_ref(&file), &seeds, &limits(20));
        assert_eq!(report.selected.len(), 3);
        assert_eq!(report.selected_tokens, 20);
        assert_eq!(report.omitted_definitions, 0);
    }
    let report = plan(
        &[file],
        &[seed("a.rs", "inner"), seed("a.rs", "alias")],
        &limits(8),
    );
    assert_eq!(report.selected.len(), 2);
    assert_eq!(report.selected_tokens, 8);
}

#[test]
fn crossing_ranges_and_inconsistent_equal_costs_are_explicitly_unavailable() {
    let crossed = input("a.rs", &[("left", 0, 20, 8), ("right", 10, 30, 8)]);
    let report = plan(
        &[crossed],
        &[seed("a.rs", "left"), seed("a.rs", "right")],
        &limits(30),
    );
    assert_eq!(report.selected_tokens, 8);
    assert_eq!(report.omitted_definitions, 1);
    assert_eq!(report.omissions[0].reason, "crossing-source-spans");
    let inconsistent = input("a.rs", &[("one", 0, 20, 8), ("two", 0, 20, 9)]);
    let report = plan(
        &[inconsistent],
        &[seed("a.rs", "one"), seed("a.rs", "two")],
        &limits(30),
    );
    assert_eq!(report.omissions[0].reason, "inconsistent-span-cost");
}

#[test]
fn unseeded_expansion_never_claims_direct_evidence_and_counts_limits() {
    let files = [
        input("b.rs", &[("b", 0, 20, 8)]),
        input("a.rs", &[("a", 0, 20, 8)]),
    ];
    let report = plan(
        &files,
        &[],
        &DefinitionPlanLimits {
            max_files: 1,
            ..limits(20)
        },
    );
    assert_eq!(report.selected.len(), 1);
    assert_eq!(report.selected[0].name, "a");
    assert_eq!(report.selected[0].role, "expansion");
    assert_eq!(report.candidate_definitions, 2);
    assert_eq!(report.omitted_definitions, 1);
    assert_eq!(report.omissions[0].reason, "file-limit");
    let report = plan(
        &files,
        &[],
        &DefinitionPlanLimits {
            max_definitions: 0,
            ..limits(20)
        },
    );
    assert!(report.selected.is_empty());
    assert_eq!(report.omitted_definitions, 2);
}

#[test]
fn ambiguous_unresolved_unsupported_and_oversized_seeds_remain_distinct() {
    let mut unsupported = input("data.tscn", &[]);
    unsupported.definitions.status = DefinitionStatus::Unsupported;
    let files = [
        input(
            "a.rs",
            &[
                ("A::run", 0, 20, 8),
                ("B::run", 20, 40, 8),
                ("huge", 40, 100, 90),
            ],
        ),
        unsupported,
    ];
    let seeds = [
        seed("a.rs", "run"),
        seed("a.rs", "missing"),
        seed("absent.rs", "missing"),
        seed("data.tscn", "run"),
        seed("a.rs", "huge"),
    ];
    let report = plan(&files, &seeds, &limits(20));
    assert!(report.selected.is_empty());
    assert_eq!(report.ambiguous_seeds, 1);
    assert_eq!(report.unresolved_seeds, 2);
    assert_eq!(report.unavailable_seeds, 1);
    assert_eq!(report.candidate_definitions, 3);
    assert_eq!(report.omitted_definitions, 3);
    for reason in [
        "ambiguous-definition",
        "unresolved-definition",
        "unresolved-file",
        "unsupported-extraction",
        "oversized-definition",
    ] {
        assert!(
            report.omissions.iter().any(|item| item.reason == reason),
            "{reason}"
        );
    }
}

#[test]
fn line_seeds_choose_innermost_but_keep_same_line_siblings_ambiguous() {
    let mut file = input(
        "a.rs",
        &[
            ("outer", 0, 100, 20),
            ("inner", 20, 40, 8),
            ("sibling", 50, 60, 8),
        ],
    );
    for fact in &mut file.definitions.definitions {
        fact.declaration_span.start_line = 1;
        fact.declaration_span.end_line = 1;
    }
    let seed = DefinitionSeed {
        path: "a.rs".into(),
        snapshot: SourceRevision::Worktree,
        selector: DefinitionSelector::Line(1),
    };
    let report = plan(
        std::slice::from_ref(&file),
        std::slice::from_ref(&seed),
        &limits(20),
    );
    assert_eq!(report.ambiguous_seeds, 1);
    assert_eq!(report.candidate_definitions, 2);
    file.definitions.definitions[2].declaration_span.start_line = 2;
    file.definitions.definitions[2].declaration_span.end_line = 2;
    let report = plan(&[file], &[seed], &limits(20));
    assert_eq!(report.selected[0].name, "inner");
}

#[test]
fn direct_seeds_precede_bounded_environment_and_failed_environment_leaves_gap() {
    let mut file = input(
        "a.rs",
        &[
            ("first", 0, 10, 5),
            ("second", 10, 20, 5),
            ("Needed", 20, 30, 7),
            ("Transitive", 30, 40, 1),
        ],
    );
    file.planning.definitions[0]
        .environment
        .push(DefinitionEnvironment {
            definition: 2,
            reason: "local-signature-type".into(),
        });
    file.planning.definitions[0]
        .gaps
        .push("dynamic-body-dependencies".into());
    file.planning.definitions[2]
        .environment
        .push(DefinitionEnvironment {
            definition: 3,
            reason: "local-signature-type".into(),
        });
    let seeds = [seed("a.rs", "first"), seed("a.rs", "second")];
    let report = plan(std::slice::from_ref(&file), &seeds, &limits(17));
    assert_eq!(
        report
            .selected
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", "Needed"]
    );
    assert_eq!(report.selected[2].role, "environment");
    assert_eq!(report.candidate_definitions, 3);
    assert_eq!(report.selected_tokens, 17);
    assert!(
        report.selected[0]
            .environment_gaps
            .contains(&"dynamic-body-dependencies".into())
    );
    let report = plan(&[file], &seeds, &limits(10));
    assert_eq!(report.selected.len(), 2);
    assert_eq!(report.selected_tokens, 10);
    assert_eq!(report.omitted_definitions, 1);
    assert!(
        report.selected[0]
            .environment_gaps
            .iter()
            .any(|gap| gap == "environment:local-signature-type:token-budget")
    );
}

#[test]
fn file_seeds_preserve_cost_coverage_and_bounded_omission_details() {
    let mut file = input("a.rs", &[("one", 0, 10, 5), ("two", 10, 20, 5)]);
    file.planning.definitions.pop();
    file.planning.omitted_definitions = 1;
    let report = plan(
        &[file],
        &[DefinitionSeed {
            path: "a.rs".into(),
            snapshot: SourceRevision::Worktree,
            selector: DefinitionSelector::File,
        }],
        &limits(10),
    );
    assert_eq!(report.selected.len(), 1);
    assert_eq!(report.omitted_definitions, 1);
    assert_eq!(report.files[0].planning_omitted_definitions, 1);
    assert_eq!(report.omissions[0].reason, "cost-unavailable");
    let seeds = (0..40)
        .map(|i| seed("missing.rs", &format!("missing{i}")))
        .collect::<Vec<_>>();
    let report = plan(&[], &seeds, &limits(10));
    assert_eq!(report.unresolved_seeds, 40);
    assert_eq!(report.omissions.len(), 32);
    assert_eq!(report.omitted_details, 8);
}

#[cfg(unix)]
mod query_tests {
    use super::*;
    use reposcout::config::Config;
    use reposcout::metrics::tokens::TokenCounter;
    use reposcout::model::SourceQueryStatus;
    use reposcout::query::{DefinitionPlanQueryOptions, plan_definitions};
    use reposcout::query::{SourceQueryOptions, SourceQueryTarget, SourceSelector, read_source};
    use reposcout::report::Format;
    use std::fmt::Write as _;
    use std::fs;
    use std::path::Path;

    fn config() -> Config {
        Config {
            use_cache: false,
            jobs: 2,
            ..Config::default()
        }
    }

    fn options() -> DefinitionPlanQueryOptions {
        DefinitionPlanQueryOptions {
            targets: vec![SourceQueryTarget {
                path: "lib.rs".into(),
                selector: SourceSelector::Symbol("run".into()),
                expected_hash: None,
                snapshot: SourceRevision::Worktree,
            }],
            snapshot: SourceRevision::Worktree,
            context_budget: 12_000,
            token_budget: 4_096,
            byte_budget: 65_536,
            max_files: 8,
            max_definitions: 16,
            include_source: false,
            format: Format::Json,
            pretty_json: false,
        }
    }

    #[test]
    fn plan_selects_local_type_environment_and_source_uses_shared_complete_reads() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "pub struct Item { pub value: u32 }\npub fn run(item: Item) -> u32 { let secret_body_marker = item.value; secret_body_marker }\n").unwrap();
        let mut cfg = config();
        cfg.enabled.tokens = false;
        let output = plan_definitions(dir.path(), &cfg, &[], &options()).unwrap();
        assert!(!output.rendered.contains("secret_body_marker"));
        assert!(output.report.source.is_none());
        assert!(
            output
                .report
                .selected
                .iter()
                .any(|selected| selected.name == "run" && selected.role == "direct")
        );
        assert!(
            output
                .report
                .selected
                .iter()
                .any(|selected| selected.name == "Item" && selected.role == "environment")
        );
        let mut with_source = options();
        with_source.include_source = true;
        let output = plan_definitions(dir.path(), &cfg, &[], &with_source).unwrap();
        let source = output.report.source.unwrap();
        assert_eq!(source.results.len(), 2);
        assert!(
            source
                .results
                .iter()
                .all(|result| result.status == SourceQueryStatus::Complete)
        );
        assert!(
            source
                .sources
                .iter()
                .any(|chunk| chunk.content.ends_with("secret_body_marker }"))
        );
        assert!(
            source
                .sources
                .iter()
                .any(|chunk| chunk.content == "pub struct Item { pub value: u32 }")
        );
    }

    #[test]
    fn plan_followup_hash_rejects_after_edit_and_explicit_expectations_apply_filewide() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "fn run() { let old = 1; }\n").unwrap();
        let plan = plan_definitions(dir.path(), &config(), &[], &options())
            .unwrap()
            .report;
        let hash = plan.files[0].sha256.clone();
        fs::write(dir.path().join("lib.rs"), "fn run() { let new = 2; }\n").unwrap();
        let read = read_source(
            dir.path(),
            &config(),
            &[],
            &SourceQueryOptions {
                targets: vec![SourceQueryTarget {
                    expected_hash: Some(hash.clone()),
                    ..options().targets.remove(0)
                }],
                token_budget: 4_096,
                byte_budget: 65_536,
                format: Format::Json,
                pretty_json: false,
            },
        )
        .unwrap();
        assert_eq!(read.report.results[0].status, SourceQueryStatus::Stale);
        assert!(read.report.sources.is_empty());
        let mut stale = options();
        stale.targets.push(stale.targets[0].clone());
        stale.targets[0].expected_hash = Some(hash);
        let output = plan_definitions(dir.path(), &config(), &[], &stale).unwrap();
        assert!(output.report.selected.is_empty());
        assert_eq!(output.report.unavailable_seeds, 2);
        assert!(
            output
                .report
                .omissions
                .iter()
                .all(|omission| omission.reason == "stale")
        );
    }

    #[test]
    fn explicit_index_plan_and_source_never_fall_back_to_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "fn run() { let staged_marker = 1; }\n",
        )
        .unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("lib.rs")).unwrap();
        index.write().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "fn other() { let working_marker = 2; }\n",
        )
        .unwrap();
        let mut selected = options();
        selected.snapshot = SourceRevision::Index;
        selected.include_source = true;
        let output = plan_definitions(dir.path(), &config(), &[], &selected).unwrap();
        assert!(output.rendered.contains("staged_marker"));
        assert!(!output.rendered.contains("working_marker"));
        assert!(
            output
                .report
                .files
                .iter()
                .all(|file| file.snapshot == SourceRevision::Index)
        );
    }

    #[test]
    fn combined_source_and_plan_obey_actual_format_budgets() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            format!(
                "fn run() {{ let payload = \"{}\"; }}\n",
                "escaped\\\"漢字".repeat(300)
            ),
        )
        .unwrap();
        let counter = TokenCounter::new("o200k_base").unwrap();
        for format in [
            Format::Json,
            Format::Ndjson,
            Format::Table,
            Format::Markdown,
        ] {
            for pretty in [false, true] {
                if pretty && format != Format::Json {
                    continue;
                }
                let mut selected = options();
                selected.include_source = true;
                selected.format = format;
                selected.pretty_json = pretty;
                selected.token_budget = 1_024;
                selected.byte_budget = 4_096;
                let output = plan_definitions(dir.path(), &config(), &[], &selected).unwrap();
                assert!(output.rendered.len() <= selected.byte_budget);
                assert!(counter.count(&output.rendered) <= selected.token_budget);
                assert!(output.rendered.ends_with('\n'));
                if format == Format::Ndjson {
                    assert_eq!(output.rendered.lines().count(), 1);
                }
                let source = output.report.source.unwrap();
                assert!(source.sources.is_empty());
                assert_eq!(source.requested_targets, 1);
                assert!(
                    source.omitted_targets == 1
                        || source
                            .results
                            .iter()
                            .all(|result| result.status == SourceQueryStatus::BudgetOmitted)
                );
            }
        }
    }

    #[test]
    fn unseeded_discovery_stays_generic_and_reports_hidden_source_policy() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("lib.rs"), "fn run() {}\n").unwrap();
        fs::write(dir.path().join(".hidden.rs"), "fn secret() {}\n").unwrap();
        let mut selected = options();
        selected.targets.clear();
        let output = plan_definitions(dir.path(), &config(), &[], &selected).unwrap();
        assert!(!output.report.selected.is_empty());
        assert!(
            output
                .report
                .selected
                .iter()
                .all(|item| item.role == "expansion")
        );
        assert!(
            output
                .report
                .files
                .iter()
                .all(|file| file.path != Path::new(".hidden.rs"))
        );
        selected.snapshot = SourceRevision::Index;
        assert!(plan_definitions(dir.path(), &config(), &[], &selected).is_err());
        let mut known = options();
        known.targets[0].path = "../escape.rs".into();
        let output = plan_definitions(dir.path(), &config(), &[], &known).unwrap();
        assert!(output.report.selected.is_empty());
        assert_eq!(output.report.omissions[0].reason, "invalid-path");
    }

    #[test]
    fn output_projection_retains_planning_totals_and_valid_file_references() {
        let dir = tempfile::tempdir().unwrap();
        let source = (0..20).fold(String::new(), |mut source, index| {
            writeln!(source, "fn entry_{index}_{}() {{}}", "long_name".repeat(15)).unwrap();
            source
        });
        fs::write(dir.path().join("lib.rs"), source).unwrap();
        let mut selected = options();
        selected.targets[0].selector = SourceSelector::Outline;
        selected.max_definitions = 32;
        selected.token_budget = 512;
        selected.byte_budget = 2_048;
        let output = plan_definitions(dir.path(), &config(), &[], &selected).unwrap();
        assert_eq!(output.report.candidate_definitions, 20);
        assert_eq!(output.report.omitted_definitions, 0);
        assert_eq!(
            output.report.selected.len() + output.report.output_omitted,
            20
        );
        assert!(output.report.output_omitted > 0);
        assert!(
            output.report.selected.iter().all(|item| output
                .report
                .files
                .iter()
                .any(|file| file.id == item.file))
        );
        assert!(output.rendered.len() <= selected.byte_budget);
    }

    #[test]
    fn projected_plans_request_source_only_for_retained_definitions() {
        let dir = tempfile::tempdir().unwrap();
        let mut selected = options();
        selected.targets.clear();
        for index in 0..8 {
            let path = format!("entry_{index}.rs");
            let name = format!("entry_{index}_{}", "long_name".repeat(10));
            fs::write(dir.path().join(&path), format!("fn {name}() {{}}\n")).unwrap();
            selected.targets.push(SourceQueryTarget {
                path: path.into(),
                selector: SourceSelector::Symbol(name),
                expected_hash: None,
                snapshot: SourceRevision::Worktree,
            });
        }
        selected.include_source = true;
        selected.token_budget = 512;
        selected.byte_budget = 2_048;
        for format in [Format::Json, Format::Table, Format::Markdown] {
            selected.format = format;
            let output = plan_definitions(dir.path(), &config(), &[], &selected).unwrap();
            assert!(output.report.output_omitted > 0);
            assert_eq!(
                output.report.selected.len() + output.report.output_omitted,
                8
            );
            let source = output.report.source.as_ref().unwrap();
            assert_eq!(source.requested_targets, output.report.selected.len());
            for file in &source.files {
                assert!(output.report.files.iter().any(|planned| {
                    planned.path == file.path
                        && Some(planned.sha256.as_str()) == file.sha256.as_deref()
                }));
            }
            for result in &source.results {
                if let Some(definition) = &result.definition {
                    assert!(output.report.selected.iter().any(|planned| {
                        planned.name == definition.name
                            && planned.declaration_span == definition.declaration_span
                    }));
                }
            }
            assert!(output.rendered.len() <= selected.byte_budget);
        }
    }

    #[test]
    fn capabilities_match_query_defaults_and_rejected_limits() {
        let capability = reposcout::query::capabilities().definition_plan.unwrap();
        let defaults = options();
        assert_eq!(capability.default_context_budget, defaults.context_budget);
        assert_eq!(capability.default_token_budget, defaults.token_budget);
        assert_eq!(capability.default_byte_budget, defaults.byte_budget);
        assert_eq!(capability.default_files, defaults.max_files);
        assert_eq!(capability.default_definitions, defaults.max_definitions);
        assert_eq!(capability.formats, ["table", "json", "markdown", "ndjson"]);
        assert_eq!(capability.snapshots, ["worktree", "index", "git-tree"]);
        let mut invalid = defaults;
        invalid.context_budget = capability.max_context_budget + 1;
        assert!(
            plan_definitions(Path::new("nonexistent-plan-root"), &config(), &[], &invalid)
                .err()
                .unwrap()
                .to_string()
                .contains("context budget")
        );
    }

    #[test]
    fn unseeded_subdirectory_plan_reads_target_relative_sources() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = git2::Repository::init(dir.path()).unwrap();
        let target = dir.path().join("src");
        fs::create_dir(&target).unwrap();
        fs::write(
            target.join("lib.rs"),
            "fn run() { let nested_marker = 1; }\n",
        )
        .unwrap();
        let mut selected = options();
        selected.targets.clear();
        selected.include_source = true;
        let output = plan_definitions(&target, &config(), &[], &selected).unwrap();
        assert_eq!(output.report.files[0].path, Path::new("lib.rs"));
        let source = output.report.source.unwrap();
        assert_eq!(source.results[0].status, SourceQueryStatus::Complete);
        assert!(source.sources[0].content.contains("nested_marker"));
        assert_eq!(
            source.files[0].sha256.as_deref(),
            Some(output.report.files[0].sha256.as_str())
        );
    }

    #[test]
    fn normalized_diagnostic_json_hands_resolved_location_to_definition_plan() {
        use reposcout::model::TaskDiagnosticFormat;
        use reposcout::task_diagnostics::{self, TaskDiagnosticLimits};
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("lib.rs"),
            "fn unrelated() {}\nfn run() { let diagnostic_marker = 1; }\n",
        )
        .unwrap();
        let raw = serde_json::json!({"message":"type mismatch","level":"error","spans":[{"file_name":"lib.rs","is_primary":true,"line_start":2,"column_start":1}]}).to_string();
        let parsed = task_diagnostics::read_input(
            std::io::Cursor::new(raw),
            TaskDiagnosticFormat::RustcJson,
            TaskDiagnosticLimits::for_safe(false),
        )
        .unwrap();
        let root = dir.path().canonicalize().unwrap();
        let resolved = task_diagnostics::resolve(parsed, &root, &root, &["lib.rs".into()]);
        let serialized = serde_json::to_value(&resolved.records[0]).unwrap();
        let mut selected = options();
        selected.targets[0].path = serialized["path"].as_str().unwrap().into();
        selected.targets[0].selector =
            SourceSelector::Line(usize::try_from(serialized["line"].as_u64().unwrap()).unwrap());
        selected.include_source = true;
        let output = plan_definitions(&root, &config(), &[], &selected).unwrap();
        assert_eq!(output.report.selected[0].name, "run");
        let source = output.report.source.unwrap();
        assert!(source.sources[0].content.contains("diagnostic_marker"));
        assert!(!source.sources[0].content.contains("unrelated"));
    }
}
