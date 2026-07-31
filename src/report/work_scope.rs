use crate::model::{WorkScope, WorkScopeCoverage};
use crate::report::{human_bytes, markdown_code_span, markdown_text, terminal_text, thousands};
use owo_colors::OwoColorize;
use std::fmt::Write as _;

const HUMAN_COMPONENTS: usize = 3;

pub(crate) fn table(out: &mut String, scope: &WorkScope, color: bool) {
    let heading = if color {
        format!("{}", "Work scope".cyan().bold())
    } else {
        "Work scope".to_string()
    };
    let _ = writeln!(out, "{heading}");
    kv(out, "Basis", &scope.basis.join(" + "));
    kv(out, "Primary inventory", &table_inventory(scope));
    if let Some(production) = &scope.production_duplication {
        kv(
            out,
            "Production duplication",
            &format!(
                "{:.1}% ({} / {} {} lines) · {}",
                production.duplicated_pct,
                thousands(production.duplicated_lines),
                thousands(production.analyzed_lines),
                terminal_text(&production.corpus),
                if production.complete {
                    "complete"
                } else {
                    "partial"
                }
            ),
        );
    }
    if let Some(seeds) = &scope.seeds {
        if let Some(focus) = &seeds.focus {
            let mut details = format!(
                "{} resolved · {} unmatched",
                thousands(focus.resolved),
                thousands(focus.unresolved)
            );
            let paths = focus
                .paths
                .iter()
                .chain(&focus.unmatched_paths)
                .map(|path| terminal_text(path))
                .collect::<Vec<_>>();
            if !paths.is_empty() {
                details.push_str(&format!(" · {}", paths.join(", ")));
            }
            append_omission(&mut details, focus.omitted);
            kv(out, "Focus seeds", &details);
        }
        if let Some(changes) = &seeds.changes {
            let mut details = format!(
                "{} · {} path{}",
                changes.scope,
                thousands(changes.total),
                if changes.total == 1 { "" } else { "s" }
            );
            if !changes.paths.is_empty() {
                details.push_str(&format!(
                    " · {}",
                    changes
                        .paths
                        .iter()
                        .map(|path| terminal_text(path))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            append_omission(&mut details, changes.omitted);
            kv(out, "Change seeds", &details);
        }
    }
    if let Some(context) = &scope.context {
        kv(
            out,
            "Context",
            &format!(
                "{} / {} files · {} / {} tokens · {} files / {} tokens omitted · {} skipped",
                thousands(context.selected_files),
                thousands(context.candidate_files),
                thousands(context.selected_tokens),
                thousands(context.budget_tokens),
                thousands(context.omitted_files),
                thousands(context.omitted_tokens),
                thousands(context.skipped_files)
            ),
        );
        if context.outline_only_files > 0
            || context.outline_symbols > 0
            || context.outline_omitted_symbols > 0
        {
            kv(
                out,
                "Outlines",
                &format!(
                    "{} {} / {} source tokens · {} symbols / {} retained · {} symbols omitted",
                    thousands(context.outline_only_files),
                    plural(context.outline_only_files, "file", "files"),
                    thousands(context.outline_only_tokens),
                    thousands(context.outline_symbols),
                    human_bytes(context.outline_bytes as u64),
                    thousands(context.outline_omitted_symbols)
                ),
            );
        }
    }
    if let Some(impact) = &scope.impact {
        let tests = if impact.matching_tests_known {
            format!("{} matching tests", thousands(impact.matching_tests))
        } else {
            "matching tests not evaluated".to_string()
        };
        kv(
            out,
            "Observed impact",
            &format!(
                "{} / {} graph seeds covered · {} direct / {} transitive dependents · {tests}",
                thousands(impact.graph_covered_seed_files),
                thousands(impact.graph_eligible_seed_files),
                thousands(impact.direct_dependents),
                thousands(impact.transitive_dependents)
            ),
        );
    }
    if let Some(structure) = &scope.structure {
        kv(
            out,
            "Graph structure",
            &format!(
                "{} files · {} components · largest {} {}",
                thousands(structure.graph_files),
                thousands(structure.components),
                thousands(structure.largest_component_files),
                plural(structure.largest_component_files, "file", "files")
            ),
        );
        for (index, component) in human_components(scope).iter().enumerate() {
            let paths = if component.representative_paths.is_empty() {
                "no representative path retained".to_string()
            } else {
                component
                    .representative_paths
                    .iter()
                    .map(|path| terminal_text(path))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let mut details = format!(
                "{} {} · {} {} · {paths}",
                thousands(component.files),
                plural(component.files, "file", "files"),
                thousands(component.seed_files),
                plural(component.seed_files, "seed", "seeds")
            );
            append_omission(&mut details, component.representative_paths_omitted);
            kv(out, &format!("Component {}", index + 1), &details);
        }
    }
    if coverage_has_gaps(&scope.confidence.primary) {
        render_coverage_table(out, "Primary coverage", &scope.confidence.primary);
    }
    if let Some(planning) = &scope.confidence.planning_universe
        && coverage_has_gaps(planning)
    {
        render_coverage_table(out, "Planning coverage", planning);
    }
    if scope.confidence.graph_unresolved_imports > 0
        || scope.confidence.graph_parse_errors > 0
        || scope.confidence.graph_config_errors > 0
    {
        kv(
            out,
            "Graph gaps",
            &format!(
                "{} unresolved · {} parse · {} config",
                thousands(scope.confidence.graph_unresolved_imports),
                thousands(scope.confidence.graph_parse_errors),
                thousands(scope.confidence.graph_config_errors)
            ),
        );
    }
    if scope.confidence.type2_analysis_partial {
        kv(out, "Type-2 duplication", "partial");
    }
    if !scope.confidence.unavailable_signals.is_empty() {
        kv(
            out,
            "Unavailable signals",
            &scope.confidence.unavailable_signals.join(", "),
        );
    }
    let _ = writeln!(out);
}

pub(crate) fn markdown(out: &mut String, scope: &WorkScope) {
    let _ = writeln!(out, "## Work scope\n");
    let _ = writeln!(out, "- Basis: {}", markdown_text(&scope.basis.join(" + ")));
    let _ = writeln!(out, "- Primary inventory: {}", markdown_inventory(scope));
    if let Some(production) = &scope.production_duplication {
        let _ = writeln!(
            out,
            "- Production duplication: **{:.1}%** (**{} / {}** {} lines); **{}**",
            production.duplicated_pct,
            thousands(production.duplicated_lines),
            thousands(production.analyzed_lines),
            markdown_text(&production.corpus),
            if production.complete {
                "complete"
            } else {
                "partial"
            }
        );
    }
    if let Some(seeds) = &scope.seeds {
        if let Some(focus) = &seeds.focus {
            let paths = focus
                .paths
                .iter()
                .chain(&focus.unmatched_paths)
                .map(|path| markdown_code_span(path))
                .collect::<Vec<_>>();
            let path_suffix = if paths.is_empty() {
                String::new()
            } else {
                format!(" — {}", paths.join(", "))
            };
            let _ = writeln!(
                out,
                "- Focus seeds: **{}** resolved, **{}** unmatched{}{}",
                thousands(focus.resolved),
                thousands(focus.unresolved),
                path_suffix,
                markdown_omission(focus.omitted)
            );
        }
        if let Some(changes) = &seeds.changes {
            let paths = changes
                .paths
                .iter()
                .map(|path| markdown_code_span(path))
                .collect::<Vec<_>>();
            let path_suffix = if paths.is_empty() {
                String::new()
            } else {
                format!(" — {}", paths.join(", "))
            };
            let _ = writeln!(
                out,
                "- Change seeds: {} with **{}** paths{}{}",
                markdown_code_span(&changes.scope),
                thousands(changes.total),
                path_suffix,
                markdown_omission(changes.omitted)
            );
        }
    }
    if let Some(context) = &scope.context {
        let _ = writeln!(
            out,
            "- Context: **{} / {}** files and **{} / {}** tokens selected; **{}** files / **{}** tokens omitted; **{}** skipped",
            thousands(context.selected_files),
            thousands(context.candidate_files),
            thousands(context.selected_tokens),
            thousands(context.budget_tokens),
            thousands(context.omitted_files),
            thousands(context.omitted_tokens),
            thousands(context.skipped_files)
        );
        if context.outline_only_files > 0
            || context.outline_symbols > 0
            || context.outline_omitted_symbols > 0
        {
            let _ = writeln!(
                out,
                "- Outlines: **{}** {} / **{}** source tokens; **{}** symbols / **{}** retained; **{}** symbols omitted",
                thousands(context.outline_only_files),
                plural(context.outline_only_files, "file", "files"),
                thousands(context.outline_only_tokens),
                thousands(context.outline_symbols),
                human_bytes(context.outline_bytes as u64),
                thousands(context.outline_omitted_symbols)
            );
        }
    }
    if let Some(impact) = &scope.impact {
        let tests = if impact.matching_tests_known {
            format!("{} matching tests", thousands(impact.matching_tests))
        } else {
            "matching tests not evaluated".to_string()
        };
        let _ = writeln!(
            out,
            "- Observed impact: **{} / {}** graph seeds covered; **{}** direct / **{}** transitive dependents; {}",
            thousands(impact.graph_covered_seed_files),
            thousands(impact.graph_eligible_seed_files),
            thousands(impact.direct_dependents),
            thousands(impact.transitive_dependents),
            markdown_text(&tests)
        );
    }
    if let Some(structure) = &scope.structure {
        let _ = writeln!(
            out,
            "- Graph structure: **{}** files across **{}** components; largest has **{}** {}",
            thousands(structure.graph_files),
            thousands(structure.components),
            thousands(structure.largest_component_files),
            plural(structure.largest_component_files, "file", "files")
        );
        for (index, component) in human_components(scope).iter().enumerate() {
            let paths = component
                .representative_paths
                .iter()
                .map(|path| markdown_code_span(path))
                .collect::<Vec<_>>();
            let paths = if paths.is_empty() {
                "no representative path retained".to_string()
            } else {
                paths.join(", ")
            };
            let _ = writeln!(
                out,
                "  - Component {}: {} {}, {} {} — {}{}",
                index + 1,
                thousands(component.files),
                plural(component.files, "file", "files"),
                thousands(component.seed_files),
                plural(component.seed_files, "seed", "seeds"),
                paths,
                markdown_omission(component.representative_paths_omitted)
            );
        }
    }
    if coverage_has_gaps(&scope.confidence.primary) {
        render_coverage_markdown(out, "Primary coverage", &scope.confidence.primary);
    }
    if let Some(planning) = &scope.confidence.planning_universe
        && coverage_has_gaps(planning)
    {
        render_coverage_markdown(out, "Planning coverage", planning);
    }
    if scope.confidence.graph_unresolved_imports > 0
        || scope.confidence.graph_parse_errors > 0
        || scope.confidence.graph_config_errors > 0
    {
        let _ = writeln!(
            out,
            "- Graph gaps: **{}** unresolved, **{}** parse, **{}** config",
            thousands(scope.confidence.graph_unresolved_imports),
            thousands(scope.confidence.graph_parse_errors),
            thousands(scope.confidence.graph_config_errors)
        );
    }
    if scope.confidence.type2_analysis_partial {
        let _ = writeln!(out, "- Type-2 duplication: **partial**");
    }
    if !scope.confidence.unavailable_signals.is_empty() {
        let _ = writeln!(
            out,
            "- Unavailable signals: {}",
            markdown_text(&scope.confidence.unavailable_signals.join(", "))
        );
    }
    let _ = writeln!(out);
}

fn render_coverage_table(out: &mut String, label: &str, coverage: &WorkScopeCoverage) {
    let mut value = coverage_inventory_table(coverage);
    value.push_str(&format!(
        " · {} unsupported · {} unreadable · {} walker errors",
        thousands(coverage.unsupported_files),
        thousands(coverage.unreadable_files),
        thousands(coverage.walker_errors)
    ));
    if coverage.oversized_files > 0 {
        value.push_str(&format!(
            " · {} oversized ({})",
            thousands(coverage.oversized_files),
            human_bytes(coverage.oversized_bytes)
        ));
    }
    if coverage.files_omitted_by_limit > 0 {
        value.push_str(&format!(
            " · {}{} limit omissions ({})",
            if coverage.omitted_count_incomplete {
                "at least "
            } else {
                ""
            },
            thousands(coverage.files_omitted_by_limit),
            human_bytes(coverage.bytes_omitted_by_limit)
        ));
    }
    if coverage.duration_limit_reached {
        value.push_str(" · duration limit reached");
    }
    if coverage.truncated {
        value.push_str(" · partial");
    }
    kv(out, label, &value);
}

fn render_coverage_markdown(out: &mut String, label: &str, coverage: &WorkScopeCoverage) {
    let oversized = if coverage.oversized_files > 0 {
        format!(
            "; **{}** oversized ({})",
            thousands(coverage.oversized_files),
            human_bytes(coverage.oversized_bytes)
        )
    } else {
        String::new()
    };
    let omitted = if coverage.files_omitted_by_limit > 0 {
        format!(
            "; **{}{}** limit omissions ({})",
            if coverage.omitted_count_incomplete {
                "at least "
            } else {
                ""
            },
            thousands(coverage.files_omitted_by_limit),
            human_bytes(coverage.bytes_omitted_by_limit)
        )
    } else {
        String::new()
    };
    let duration = if coverage.duration_limit_reached {
        "; **duration limit reached**"
    } else {
        ""
    };
    let partial = if coverage.truncated {
        "; **partial**"
    } else {
        ""
    };
    let inventory = coverage_inventory_markdown(coverage);
    let _ = writeln!(
        out,
        "- {}: {}; **{}** unsupported, **{}** unreadable, **{}** walker errors{}{}{}{}",
        markdown_text(label),
        inventory,
        thousands(coverage.unsupported_files),
        thousands(coverage.unreadable_files),
        thousands(coverage.walker_errors),
        oversized,
        omitted,
        duration,
        partial
    );
}

fn table_inventory(scope: &WorkScope) -> String {
    if scope.diff_scope.is_some() {
        format!(
            "{} diff files · {} source files · {} source tokens · {} repository paths discovered",
            thousands(scope.inventory.primary_files),
            thousands(scope.inventory.source_files),
            thousands(scope.inventory.source_tokens),
            thousands(scope.inventory.discovery_files)
        )
    } else {
        format!(
            "{} / {} analyzed · {} source files · {} source tokens",
            thousands(scope.inventory.primary_files),
            thousands(scope.inventory.discovery_files),
            thousands(scope.inventory.source_files),
            thousands(scope.inventory.source_tokens)
        )
    }
}

fn markdown_inventory(scope: &WorkScope) -> String {
    if scope.diff_scope.is_some() {
        format!(
            "**{}** diff files; **{}** source files; **{}** source tokens; **{}** repository paths discovered",
            thousands(scope.inventory.primary_files),
            thousands(scope.inventory.source_files),
            thousands(scope.inventory.source_tokens),
            thousands(scope.inventory.discovery_files)
        )
    } else {
        format!(
            "**{} / {}** files analyzed; **{}** source files; **{}** source tokens",
            thousands(scope.inventory.primary_files),
            thousands(scope.inventory.discovery_files),
            thousands(scope.inventory.source_files),
            thousands(scope.inventory.source_tokens)
        )
    }
}

fn coverage_inventory_table(coverage: &WorkScopeCoverage) -> String {
    if coverage.diff_scoped {
        format!(
            "{} primary diff files · {} repository paths discovered",
            thousands(coverage.analyzed_files),
            thousands(coverage.discovered_files)
        )
    } else {
        format!(
            "{} / {} analyzed",
            thousands(coverage.analyzed_files),
            thousands(coverage.discovered_files)
        )
    }
}

fn coverage_inventory_markdown(coverage: &WorkScopeCoverage) -> String {
    if coverage.diff_scoped {
        format!(
            "**{}** primary diff files; **{}** repository paths discovered",
            thousands(coverage.analyzed_files),
            thousands(coverage.discovered_files)
        )
    } else {
        format!(
            "**{} / {}** analyzed",
            thousands(coverage.analyzed_files),
            thousands(coverage.discovered_files)
        )
    }
}

fn append_omission(value: &mut String, omitted: usize) {
    if omitted > 0 {
        value.push_str(&format!(" · {} omitted", thousands(omitted)));
    }
}

fn markdown_omission(omitted: usize) -> String {
    if omitted > 0 {
        format!("; **{}** omitted", thousands(omitted))
    } else {
        String::new()
    }
}

fn human_components(scope: &WorkScope) -> Vec<&crate::model::WorkScopeComponent> {
    let Some(structure) = &scope.structure else {
        return Vec::new();
    };
    let seeded = structure
        .entries
        .iter()
        .filter(|component| component.seed_files > 0)
        .take(HUMAN_COMPONENTS)
        .collect::<Vec<_>>();
    if seeded.is_empty() {
        structure.entries.iter().take(HUMAN_COMPONENTS).collect()
    } else if seeded.len() > 1 {
        seeded
    } else {
        Vec::new()
    }
}

fn coverage_has_gaps(coverage: &WorkScopeCoverage) -> bool {
    coverage.unsupported_files > 0
        || coverage.unreadable_files > 0
        || coverage.walker_errors > 0
        || coverage.oversized_files > 0
        || coverage.files_omitted_by_limit > 0
        || coverage.duration_limit_reached
        || coverage.truncated
}

fn plural<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

fn kv(out: &mut String, key: &str, value: &str) {
    let _ = writeln!(out, "  {key:<20} {value}");
}
