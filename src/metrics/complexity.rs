//! Complexity metrics: cyclomatic, cognitive, max nesting depth, Halstead, and
//! the Maintainability Index.
//!
//! ## Contract (frozen)
//! `analyze(lang, content, tree, lines) -> (Complexity, approximate)`
//! - `tree` is `Some` for first-class languages (accurate, AST-based) and
//!   `None` otherwise (heuristic fallback; `approximate = true`).
//! - `lines` provides SLOC/comment counts required by the Maintainability Index.

use crate::lang::{FirstClass, LangInfo};
use crate::metrics::lines::LineStats;
use crate::model::{Complexity, FunctionComplexity, Halstead};
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

/// Analyze complexity for one file. Returns metrics and whether they are
/// approximate (heuristic fallback used because no grammar was available).
pub fn analyze(
    lang: &LangInfo,
    content: &str,
    tree: Option<&Tree>,
    lines: &LineStats,
) -> (Complexity, bool) {
    let approximate = tree.is_none();

    let mut complexity = match (lang.first_class, tree) {
        (Some(fc), Some(tree)) => analyze_ast(fc, content, tree),
        _ => analyze_heuristic(content),
    };
    complexity.maintainability_index =
        maintainability_index(&complexity.halstead, complexity.cyclomatic, lines);

    (complexity, approximate)
}

/// Microsoft Maintainability Index, normalized to `0..100`.
///
/// `MI = max(0, (171 - 5.2*ln(HV) - 0.23*CC - 16.2*ln(SLOC)) * 100 / 171)`
///
/// Microsoft computes its current source-based metric from logical operations.
/// RepoScout uses nonblank, non-comment SLOC as the closest cross-language
/// source-level proxy.
pub fn maintainability_index(halstead: &Halstead, cyclomatic: u32, lines: &LineStats) -> f64 {
    let hv = halstead.volume.max(1.0);
    let cc = cyclomatic as f64;
    let sloc = (lines.sloc.max(1)) as f64;
    let raw = 171.0 - 5.2 * hv.ln() - 0.23 * cc - 16.2 * sloc.ln();
    ((raw.max(0.0) / 171.0) * 100.0).min(100.0)
}

#[derive(Clone, Copy)]
struct LangConfig {
    function_kinds: &'static [&'static str],
    decision_kinds: &'static [&'static str],
    case_kinds: &'static [&'static str],
    cognitive_structure_kinds: &'static [&'static str],
    nesting_kinds: &'static [&'static str],
    else_clause_kinds: &'static [&'static str],
    jump_kinds: &'static [&'static str],
}

#[derive(Default)]
struct ScopeMetrics {
    decision_points: u32,
    cognitive: u32,
    max_nesting: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenClass {
    Operator,
    Operand,
}

const RUST_FUNCTIONS: &[&str] = &["function_item", "closure_expression"];
const RUST_DECISIONS: &[&str] = &[
    "if_expression",
    "while_expression",
    "while_let_expression",
    "for_expression",
    "loop_expression",
];
const RUST_CASES: &[&str] = &["match_arm"];
const RUST_COGNITIVE: &[&str] = &[
    "if_expression",
    "while_expression",
    "while_let_expression",
    "for_expression",
    "loop_expression",
    "match_expression",
];
const RUST_NESTING: &[&str] = RUST_COGNITIVE;
const RUST_ELSE: &[&str] = &[];
const RUST_JUMPS: &[&str] = &["break_expression", "continue_expression"];

const PY_FUNCTIONS: &[&str] = &["function_definition", "lambda"];
const PY_DECISIONS: &[&str] = &[
    "if_statement",
    "elif_clause",
    "for_statement",
    "while_statement",
    "except_clause",
    "conditional_expression",
    "for_in_clause",
    "if_clause",
];
const PY_CASES: &[&str] = &["case_clause"];
const PY_COGNITIVE: &[&str] = &[
    "if_statement",
    "for_statement",
    "while_statement",
    "except_clause",
    "conditional_expression",
    "match_statement",
    "for_in_clause",
    "if_clause",
];
const PY_NESTING: &[&str] = PY_COGNITIVE;
const PY_ELSE: &[&str] = &["elif_clause", "else_clause"];
const PY_JUMPS: &[&str] = &[];

const JS_FUNCTIONS: &[&str] = &[
    "function_declaration",
    "function_expression",
    "function",
    "arrow_function",
    "method_definition",
    "generator_function_declaration",
    "generator_function",
];
const JS_DECISIONS: &[&str] = &[
    "if_statement",
    "for_statement",
    "for_in_statement",
    "for_of_statement",
    "while_statement",
    "do_statement",
    "catch_clause",
    "ternary_expression",
    "assignment_pattern",
    "object_assignment_pattern",
    "optional_chain",
];
const JS_CASES: &[&str] = &["switch_case"];
const JS_COGNITIVE: &[&str] = &[
    "if_statement",
    "for_statement",
    "for_in_statement",
    "for_of_statement",
    "while_statement",
    "do_statement",
    "catch_clause",
    "ternary_expression",
    "switch_statement",
];
const JS_NESTING: &[&str] = JS_COGNITIVE;
const JS_ELSE: &[&str] = &["else_clause"];
const JS_JUMPS: &[&str] = &["break_statement", "continue_statement"];

const GO_FUNCTIONS: &[&str] = &["function_declaration", "method_declaration", "func_literal"];
const GO_DECISIONS: &[&str] = &["if_statement", "for_statement"];
const GO_CASES: &[&str] = &["expression_case", "type_case", "communication_case"];
const GO_COGNITIVE: &[&str] = &[
    "if_statement",
    "for_statement",
    "switch_statement",
    "expression_switch_statement",
    "type_switch_statement",
    "select_statement",
];
const GO_NESTING: &[&str] = GO_COGNITIVE;
const GO_ELSE: &[&str] = &[];
const GO_JUMPS: &[&str] = &["branch_statement"];

const PHP_FUNCTIONS: &[&str] = &[
    "function_definition",
    "method_declaration",
    "anonymous_function",
    "arrow_function",
];
const PHP_DECISIONS: &[&str] = &[
    "if_statement",
    "else_if_clause",
    "for_statement",
    "foreach_statement",
    "while_statement",
    "do_statement",
    "catch_clause",
    "conditional_expression",
];
const PHP_CASES: &[&str] = &["case_statement", "match_conditional_expression"];
const PHP_COGNITIVE: &[&str] = &[
    "if_statement",
    "for_statement",
    "foreach_statement",
    "while_statement",
    "do_statement",
    "catch_clause",
    "conditional_expression",
    "switch_statement",
    "match_expression",
];
const PHP_NESTING: &[&str] = PHP_COGNITIVE;
const PHP_ELSE: &[&str] = &["else_if_clause", "else_clause"];
const PHP_JUMPS: &[&str] = &["break_statement", "continue_statement"];

fn config(fc: FirstClass) -> LangConfig {
    match fc {
        FirstClass::Rust => LangConfig {
            function_kinds: RUST_FUNCTIONS,
            decision_kinds: RUST_DECISIONS,
            case_kinds: RUST_CASES,
            cognitive_structure_kinds: RUST_COGNITIVE,
            nesting_kinds: RUST_NESTING,
            else_clause_kinds: RUST_ELSE,
            jump_kinds: RUST_JUMPS,
        },
        FirstClass::Python => LangConfig {
            function_kinds: PY_FUNCTIONS,
            decision_kinds: PY_DECISIONS,
            case_kinds: PY_CASES,
            cognitive_structure_kinds: PY_COGNITIVE,
            nesting_kinds: PY_NESTING,
            else_clause_kinds: PY_ELSE,
            jump_kinds: PY_JUMPS,
        },
        FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx => LangConfig {
            function_kinds: JS_FUNCTIONS,
            decision_kinds: JS_DECISIONS,
            case_kinds: JS_CASES,
            cognitive_structure_kinds: JS_COGNITIVE,
            nesting_kinds: JS_NESTING,
            else_clause_kinds: JS_ELSE,
            jump_kinds: JS_JUMPS,
        },
        FirstClass::Go => LangConfig {
            function_kinds: GO_FUNCTIONS,
            decision_kinds: GO_DECISIONS,
            case_kinds: GO_CASES,
            cognitive_structure_kinds: GO_COGNITIVE,
            nesting_kinds: GO_NESTING,
            else_clause_kinds: GO_ELSE,
            jump_kinds: GO_JUMPS,
        },
        FirstClass::Php => LangConfig {
            function_kinds: PHP_FUNCTIONS,
            decision_kinds: PHP_DECISIONS,
            case_kinds: PHP_CASES,
            cognitive_structure_kinds: PHP_COGNITIVE,
            nesting_kinds: PHP_NESTING,
            else_clause_kinds: PHP_ELSE,
            jump_kinds: PHP_JUMPS,
        },
    }
}

fn analyze_ast(fc: FirstClass, content: &str, tree: &Tree) -> Complexity {
    let cfg = config(fc);
    let root = tree.root_node();
    let top_level = analyze_scope(root, fc, &cfg, content, true);
    let mut functions = collect_functions(root, fc, &cfg, content);

    let mut cognitive = top_level.cognitive;
    let mut max_nesting = top_level.max_nesting;
    for function in &functions {
        cognitive = cognitive.saturating_add(function.cognitive);
        max_nesting = max_nesting.max(function.max_nesting);
    }

    functions.sort_by_key(|f| f.line);
    let cyclomatic = if functions.is_empty() {
        1u32.saturating_add(top_level.decision_points)
    } else {
        functions
            .iter()
            .fold(top_level.decision_points, |total, function| {
                total.saturating_add(function.cyclomatic)
            })
    };

    Complexity {
        cyclomatic,
        cognitive,
        max_nesting,
        halstead: halstead_from_ast(root, content),
        maintainability_index: 0.0,
        functions,
    }
}

fn analyze_scope<'a>(
    root: Node<'a>,
    fc: FirstClass,
    cfg: &LangConfig,
    content: &str,
    skip_nested_functions: bool,
) -> ScopeMetrics {
    let root_start = root.start_byte();
    let root_end = root.end_byte();
    let root_kind = root.kind();
    let mut metrics = ScopeMetrics::default();
    let mut stack = vec![(root, 0u32)];

    while let Some((node, nesting)) = stack.pop() {
        if skip_nested_functions
            && !same_node(node, root_start, root_end, root_kind)
            && is_function_node(node, cfg)
        {
            continue;
        }

        metrics.decision_points = metrics
            .decision_points
            .saturating_add(decision_increment(node, fc, cfg, content));

        if is_top_level_boolean_expression(node, fc, content) {
            metrics.cognitive = metrics
                .cognitive
                .saturating_add(count_boolean_sequences(node, fc, content));
        }

        if is_labeled_jump(node, fc, cfg, content) {
            metrics.cognitive = metrics.cognitive.saturating_add(1);
        }

        let else_if = is_else_if_node(node, cfg);
        let else_clause = is_else_clause(node, cfg, content);
        if else_clause {
            metrics.cognitive = metrics.cognitive.saturating_add(1);
        }

        let structural = contains(cfg.cognitive_structure_kinds, node.kind()) && !else_if;
        if structural {
            metrics.cognitive = metrics
                .cognitive
                .saturating_add(1u32.saturating_add(nesting));
        }

        let increases_nesting = contains(cfg.nesting_kinds, node.kind()) && !else_if;
        let child_nesting = if increases_nesting {
            let next = nesting.saturating_add(1);
            metrics.max_nesting = metrics.max_nesting.max(next);
            next
        } else {
            nesting
        };

        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i as u32) {
                stack.push((child, child_nesting));
            }
        }
    }

    metrics
}

fn collect_functions<'a>(
    root: Node<'a>,
    fc: FirstClass,
    cfg: &LangConfig,
    content: &str,
) -> Vec<FunctionComplexity> {
    let mut nodes = Vec::new();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        if is_function_node(node, cfg) {
            nodes.push(node);
        }
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i as u32) {
                stack.push(child);
            }
        }
    }

    nodes.sort_by_key(|node| node.start_byte());
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    nodes
        .into_iter()
        .map(|node| {
            let metrics = analyze_scope(node, fc, cfg, content, true);
            let name = function_name(node, content);
            let base_key = lexical_symbol_base(node, cfg, content, &name);
            let occurrence = occurrences.entry(base_key.clone()).or_default();
            *occurrence += 1;
            let direct_recursion = directly_calls(node, cfg, content, &name);
            FunctionComplexity {
                name,
                line: node.start_position().row + 1,
                end_line: node.end_position().row + 1,
                symbol_key: format!("{base_key}#{}", *occurrence),
                cyclomatic: 1u32.saturating_add(metrics.decision_points),
                cognitive: metrics
                    .cognitive
                    .saturating_add(u32::from(direct_recursion)),
                max_nesting: metrics.max_nesting,
            }
        })
        .collect()
}

fn directly_calls(root: Node<'_>, cfg: &LangConfig, content: &str, name: &str) -> bool {
    if name.starts_with("<anonymous:") {
        return false;
    }

    let root_start = root.start_byte();
    let root_end = root.end_byte();
    let root_kind = root.kind();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !same_node(node, root_start, root_end, root_kind) && is_function_node(node, cfg) {
            continue;
        }
        if recursive_call_matches(node, content, name) {
            return true;
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index as u32) {
                stack.push(child);
            }
        }
    }
    false
}

fn recursive_call_matches(node: Node<'_>, content: &str, name: &str) -> bool {
    match node.kind() {
        "member_call_expression" | "nullsafe_member_call_expression" => {
            node.child_by_field_name("object")
                .is_some_and(|object| node_text(object, content).trim() == "$this")
                && node
                    .child_by_field_name("name")
                    .is_some_and(|target| call_target_matches(node_text(target, content), name))
        }
        "scoped_call_expression" => {
            node.child_by_field_name("scope")
                .is_some_and(|scope| matches!(node_text(scope, content).trim(), "self" | "static"))
                && node
                    .child_by_field_name("name")
                    .is_some_and(|target| call_target_matches(node_text(target, content), name))
        }
        "call" | "call_expression" | "function_call_expression" => node
            .child_by_field_name("function")
            .or_else(|| node.named_child(0))
            .is_some_and(|target| call_target_matches(node_text(target, content), name)),
        _ => false,
    }
}

fn call_target_matches(target: &str, name: &str) -> bool {
    let target = target.trim();
    if target == name || target.starts_with(&format!("{name}::<")) {
        return true;
    }
    target
        .rsplit(['.', ':'])
        .find(|part| !part.is_empty())
        .is_some_and(|part| part.trim() == name)
}

fn lexical_symbol_base(node: Node<'_>, cfg: &LangConfig, content: &str, name: &str) -> String {
    let mut parts = vec![name.to_string()];
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if is_function_node(parent, cfg) {
            parts.push(function_name(parent, content));
        } else if is_symbol_container(parent.kind())
            && let Some(container) = parent
                .child_by_field_name("name")
                .or_else(|| parent.child_by_field_name("type"))
                .and_then(|child| child.utf8_text(content.as_bytes()).ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        {
            parts.push(container.to_string());
        }
        ancestor = parent.parent();
    }
    parts.reverse();
    parts.join("::")
}

fn is_symbol_container(kind: &str) -> bool {
    kind.contains("class")
        || kind.contains("interface")
        || matches!(
            kind,
            "impl_item"
                | "trait_item"
                | "trait_declaration"
                | "struct_item"
                | "enum_item"
                | "type_declaration"
        )
}

fn decision_increment(node: Node<'_>, fc: FirstClass, cfg: &LangConfig, content: &str) -> u32 {
    let mut increment = 0u32;
    if contains(cfg.decision_kinds, node.kind()) {
        increment = increment.saturating_add(1);
    }
    if contains(cfg.case_kinds, node.kind()) && !is_catch_all_case(node, fc, content) {
        increment = increment.saturating_add(1);
    }
    if is_cyclomatic_operator_token(node, fc, content) {
        increment = increment.saturating_add(1);
    }
    if is_short_circuit_assignment(node, fc, content) {
        increment = increment.saturating_add(1);
    }
    increment
}

fn is_cyclomatic_operator_token(node: Node<'_>, fc: FirstClass, content: &str) -> bool {
    if is_boolean_operator_token(node, fc, content) {
        return true;
    }
    matches!(
        fc,
        FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx | FirstClass::Php
    ) && node.child_count() == 0
        && node_text(node, content).trim() == "??"
}

fn is_short_circuit_assignment(node: Node<'_>, fc: FirstClass, content: &str) -> bool {
    if !matches!(
        fc,
        FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx
    ) || node.kind() != "augmented_assignment_expression"
    {
        return false;
    }
    node.child_by_field_name("operator")
        .is_some_and(|operator| {
            matches!(node_text(operator, content).trim(), "&&=" | "||=" | "??=")
        })
}

fn is_catch_all_case(node: Node<'_>, fc: FirstClass, content: &str) -> bool {
    if fc == FirstClass::Rust && node.kind() == "match_arm" {
        let Some(pattern) = node.child_by_field_name("pattern") else {
            return false;
        };
        if pattern.child_by_field_name("condition").is_some() {
            return false;
        }
        return is_rust_binding_pattern(node_text(pattern, content));
    }

    if fc == FirstClass::Python && node.kind() == "case_clause" {
        if node.child_by_field_name("guard").is_some() {
            return false;
        }
        let mut cursor = node.walk();
        let Some(pattern) = node
            .named_children(&mut cursor)
            .find(|child| child.kind() == "case_pattern")
        else {
            return false;
        };
        return is_python_capture_pattern(node_text(pattern, content));
    }

    let text = node_text(node, content).trim_start();
    if text.starts_with("default") {
        return true;
    }
    if let Some(rest) = text.strip_prefix("case") {
        let pattern = rest.trim_start();
        return pattern.starts_with('_') || pattern.starts_with("default");
    }
    if let Some((pattern, _)) = text.split_once("=>") {
        return pattern.trim() == "_";
    }
    false
}

fn is_rust_binding_pattern(pattern: &str) -> bool {
    let mut pattern = pattern.trim();
    loop {
        if let Some(rest) = pattern.strip_prefix('&') {
            pattern = rest.trim_start();
        } else if let Some(rest) = pattern.strip_prefix("ref ") {
            pattern = rest.trim_start();
        } else if let Some(rest) = pattern.strip_prefix("mut ") {
            pattern = rest.trim_start();
        } else {
            break;
        }
    }
    if pattern == "_" {
        return true;
    }
    let pattern = pattern.strip_prefix("r#").unwrap_or(pattern);
    is_identifier(pattern) && pattern.starts_with(|ch: char| ch == '_' || ch.is_lowercase())
}

fn is_python_capture_pattern(pattern: &str) -> bool {
    let pattern = pattern.trim();
    pattern == "_" || (!matches!(pattern, "True" | "False" | "None") && is_identifier(pattern))
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

fn is_top_level_boolean_expression(node: Node<'_>, fc: FirstClass, content: &str) -> bool {
    if !is_boolean_expression_kind(node.kind()) || !subtree_has_boolean_operator(node, fc, content)
    {
        return false;
    }
    !node.parent().is_some_and(|parent| {
        is_boolean_expression_kind(parent.kind())
            && subtree_has_boolean_operator(parent, fc, content)
    })
}

fn is_boolean_expression_kind(kind: &str) -> bool {
    matches!(kind, "binary_expression" | "boolean_operator")
}

fn subtree_has_boolean_operator(node: Node<'_>, fc: FirstClass, content: &str) -> bool {
    count_boolean_operators(node, fc, content) > 0
}

fn count_boolean_operators(node: Node<'_>, fc: FirstClass, content: &str) -> u32 {
    let mut count = 0u32;
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if is_boolean_operator_token(current, fc, content) {
            count = count.saturating_add(1);
        }
        for i in (0..current.child_count()).rev() {
            if let Some(child) = current.child(i as u32) {
                stack.push(child);
            }
        }
    }
    count
}

fn count_boolean_sequences(node: Node<'_>, fc: FirstClass, content: &str) -> u32 {
    let mut ops = Vec::new();
    collect_boolean_ops_in_order(node, fc, content, &mut ops);
    let mut sequences = 0u32;
    let mut previous: Option<&'static str> = None;
    for op in ops {
        if previous != Some(op) {
            sequences = sequences.saturating_add(1);
            previous = Some(op);
        }
    }
    sequences
}

fn collect_boolean_ops_in_order(
    node: Node<'_>,
    fc: FirstClass,
    content: &str,
    ops: &mut Vec<&'static str>,
) {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if let Some(op) = boolean_operator_kind(current, fc, content) {
            ops.push(op);
        }
        for i in (0..current.child_count()).rev() {
            if let Some(child) = current.child(i as u32) {
                stack.push(child);
            }
        }
    }
}

fn is_boolean_operator_token(node: Node<'_>, fc: FirstClass, content: &str) -> bool {
    boolean_operator_kind(node, fc, content).is_some()
}

fn boolean_operator_kind(node: Node<'_>, fc: FirstClass, content: &str) -> Option<&'static str> {
    let kind = node.kind();
    let text = node_text(node, content).trim();
    match text {
        "&&" => Some("&&"),
        "||" => Some("||"),
        "and" if fc == FirstClass::Python || kind == "and" || kind == "boolean_operator" => {
            Some("and")
        }
        "or" if fc == FirstClass::Python || kind == "or" || kind == "boolean_operator" => {
            Some("or")
        }
        "xor" if fc == FirstClass::Php => Some("xor"),
        _ => None,
    }
}

fn is_else_clause(node: Node<'_>, cfg: &LangConfig, content: &str) -> bool {
    if contains(cfg.else_clause_kinds, node.kind()) {
        return true;
    }
    if node.child_count() == 0 {
        let text = node_text(node, content).trim();
        return (text == "else" || text == "elif")
            && !node
                .parent()
                .is_some_and(|parent| contains(cfg.else_clause_kinds, parent.kind()));
    }
    false
}

fn is_else_if_node(node: Node<'_>, cfg: &LangConfig) -> bool {
    if node.kind() != "if_statement" && node.kind() != "if_expression" {
        return false;
    }
    node.parent()
        .is_some_and(|parent| contains(cfg.else_clause_kinds, parent.kind()))
}

fn is_labeled_jump(node: Node<'_>, fc: FirstClass, cfg: &LangConfig, content: &str) -> bool {
    if !contains(cfg.jump_kinds, node.kind()) {
        return false;
    }
    let text = node_text(node, content).trim();
    match fc {
        FirstClass::Rust => text.starts_with("break '") || text.starts_with("continue '"),
        FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx => {
            labeled_js_jump(text, "break") || labeled_js_jump(text, "continue")
        }
        FirstClass::Go => {
            text.starts_with("goto ")
                || text
                    .strip_prefix("break ")
                    .is_some_and(|label| is_identifier_like(label.trim()))
                || text
                    .strip_prefix("continue ")
                    .is_some_and(|label| is_identifier_like(label.trim()))
        }
        FirstClass::Python => false,
        FirstClass::Php => text
            .strip_prefix("break")
            .or_else(|| text.strip_prefix("continue"))
            .map(|level| level.trim().trim_end_matches(';').trim())
            .is_some_and(|level| !level.is_empty() && level != "1"),
    }
}

fn labeled_js_jump(text: &str, keyword: &str) -> bool {
    text.strip_prefix(keyword)
        .map(|rest| rest.trim().trim_end_matches(';').trim())
        .is_some_and(|label| !label.is_empty() && is_identifier_like(label))
}

fn function_name(node: Node<'_>, content: &str) -> String {
    if let Some(name) = named_field_text(node, "name", content) {
        return name;
    }

    // Anonymous callables usually inherit a useful name from their binding;
    // looking at their own identifiers first would report a parameter name.
    if is_anonymous_function_kind(node.kind())
        && let Some(name) = parent_binding_name(node, content)
    {
        return name.trim_start_matches('$').to_string();
    }
    if is_anonymous_function_kind(node.kind()) {
        return format!("<anonymous:{}>", node.start_position().row + 1);
    }

    for kind in [
        "identifier",
        "field_identifier",
        "property_identifier",
        "type_identifier",
    ] {
        if let Some(name) = first_child_text_of_kind(node, kind, content) {
            return name;
        }
    }

    if let Some(name) = parent_binding_name(node, content) {
        return name.trim_start_matches('$').to_string();
    }

    format!("<anonymous:{}>", node.start_position().row + 1)
}

fn is_anonymous_function_kind(kind: &str) -> bool {
    matches!(
        kind,
        "arrow_function"
            | "closure_expression"
            | "func_literal"
            | "function"
            | "function_expression"
            | "generator_function"
            | "anonymous_function"
            | "lambda"
    )
}

fn parent_binding_name(node: Node<'_>, content: &str) -> Option<String> {
    let mut ancestor = node.parent();
    for _ in 0..4 {
        let current = ancestor?;
        if is_callable_kind(current.kind()) {
            return None;
        }
        for field in ["name", "left", "key", "pattern"] {
            if let Some(name) = named_field_text(current, field, content) {
                return Some(name);
            }
        }
        if matches!(
            current.kind(),
            "assignment"
                | "assignment_expression"
                | "const_declaration"
                | "let_declaration"
                | "lexical_declaration"
                | "short_var_declaration"
                | "variable_declarator"
        ) {
            for kind in ["identifier", "field_identifier", "property_identifier"] {
                if let Some(name) = first_child_text_of_kind(current, kind, content) {
                    return Some(name);
                }
            }
        }
        ancestor = current.parent();
    }
    None
}

fn is_callable_kind(kind: &str) -> bool {
    RUST_FUNCTIONS.contains(&kind)
        || PY_FUNCTIONS.contains(&kind)
        || JS_FUNCTIONS.contains(&kind)
        || GO_FUNCTIONS.contains(&kind)
        || PHP_FUNCTIONS.contains(&kind)
}

fn named_field_text(node: Node<'_>, field: &str, content: &str) -> Option<String> {
    let child = node.child_by_field_name(field)?;
    let text = node_text(child, content).trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn first_child_text_of_kind(node: Node<'_>, kind: &str, content: &str) -> Option<String> {
    for i in 0..node.child_count() {
        let child = node.child(i as u32)?;
        if child.kind() == kind {
            let text = node_text(child, content).trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn halstead_from_ast(root: Node<'_>, content: &str) -> Halstead {
    let mut operators = HashSet::new();
    let mut operands = HashSet::new();
    let mut total_operators = 0usize;
    let mut total_operands = 0usize;
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        if is_comment_kind(node.kind()) {
            continue;
        }
        if node.child_count() == 0 {
            let text = node_text(node, content).trim();
            if let Some(class) = classify_token(node.kind(), text) {
                match class {
                    TokenClass::Operator => {
                        total_operators += 1;
                        operators.insert(text.to_string());
                    }
                    TokenClass::Operand => {
                        total_operands += 1;
                        operands.insert(text.to_string());
                    }
                }
            }
        } else {
            for i in (0..node.child_count()).rev() {
                if let Some(child) = node.child(i as u32) {
                    stack.push(child);
                }
            }
        }
    }

    finish_halstead(
        operators.len(),
        operands.len(),
        total_operators,
        total_operands,
    )
}

fn classify_token(kind: &str, text: &str) -> Option<TokenClass> {
    if text.is_empty() || kind == "ERROR" || is_comment_kind(kind) {
        return None;
    }

    let lower = kind.to_ascii_lowercase();
    if is_operand_kind(&lower, text) {
        Some(TokenClass::Operand)
    } else if is_operator_text(text) || is_keyword(text) || lower.contains("operator") {
        Some(TokenClass::Operator)
    } else if is_identifier_like(text) {
        Some(TokenClass::Operand)
    } else {
        Some(TokenClass::Operator)
    }
}

fn is_operand_kind(kind: &str, text: &str) -> bool {
    kind.contains("identifier")
        || kind.contains("literal")
        || kind.contains("string")
        || kind.contains("number")
        || kind.contains("integer")
        || kind.contains("float")
        || kind.contains("character")
        || kind.contains("rune")
        || matches!(
            kind,
            "self" | "super" | "this" | "none" | "nil" | "null" | "true" | "false"
        )
        || matches!(
            text,
            "None" | "nil" | "null" | "true" | "false" | "True" | "False"
        )
}

fn is_operator_text(text: &str) -> bool {
    text.chars()
        .any(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
}

fn is_keyword(text: &str) -> bool {
    matches!(
        text,
        "as" | "async"
            | "await"
            | "break"
            | "case"
            | "catch"
            | "chan"
            | "class"
            | "const"
            | "continue"
            | "crate"
            | "def"
            | "defer"
            | "do"
            | "else"
            | "enum"
            | "except"
            | "extends"
            | "fn"
            | "for"
            | "from"
            | "func"
            | "function"
            | "go"
            | "goto"
            | "if"
            | "impl"
            | "implements"
            | "import"
            | "in"
            | "interface"
            | "let"
            | "loop"
            | "map"
            | "match"
            | "mod"
            | "mut"
            | "new"
            | "of"
            | "package"
            | "pub"
            | "range"
            | "ref"
            | "return"
            | "select"
            | "struct"
            | "switch"
            | "trait"
            | "try"
            | "type"
            | "use"
            | "var"
            | "where"
            | "while"
            | "yield"
            | "and"
            | "or"
            | "not"
            | "is"
            | "lambda"
    )
}

fn analyze_heuristic(content: &str) -> Complexity {
    let cyclomatic = heuristic_cyclomatic(content);
    let halstead = halstead_heuristic(content);
    Complexity {
        cyclomatic,
        cognitive: cyclomatic.saturating_sub(1),
        max_nesting: heuristic_max_nesting(content),
        halstead,
        maintainability_index: 0.0,
        functions: Vec::new(),
    }
}

/// Language-agnostic cyclomatic approximation: 1 + number of decision points.
pub fn heuristic_cyclomatic(content: &str) -> u32 {
    const KEYWORDS: &[&str] = &[
        "if", "for", "while", "case", "catch", "elif", "when", "and", "or",
    ];
    let mut count = 1u32;
    for kw in KEYWORDS {
        count += count_word(content, kw) as u32;
    }
    for op in ["&&", "||", "?"] {
        count += content.matches(op).count() as u32;
    }
    count
}

fn heuristic_max_nesting(content: &str) -> u32 {
    let mut max_indent_depth = 0usize;
    let mut indent_stack: Vec<usize> = Vec::new();
    let mut brace_depth = 0u32;
    let mut max_brace_depth = 0u32;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            continue;
        }

        let indent = line
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .map(|ch| if ch == '\t' { 4 } else { 1 })
            .sum::<usize>();
        while indent_stack.last().is_some_and(|prev| indent <= *prev) {
            indent_stack.pop();
        }
        if indent_stack.last().is_none_or(|prev| indent > *prev) && indent > 0 {
            indent_stack.push(indent);
        }
        max_indent_depth = max_indent_depth.max(indent_stack.len());

        for ch in trimmed.chars() {
            match ch {
                '{' => {
                    brace_depth = brace_depth.saturating_add(1);
                    max_brace_depth = max_brace_depth.max(brace_depth);
                }
                '}' => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    (max_indent_depth as u32).max(max_brace_depth)
}

fn halstead_heuristic(content: &str) -> Halstead {
    let mut operators = HashSet::new();
    let mut operands = HashSet::new();
    let mut total_operators = 0usize;
    let mut total_operands = 0usize;
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_alphanumeric() || ch == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            total_operands += 1;
            operands.insert(token);
        } else {
            let start = i;
            i += 1;
            if i < chars.len() && is_operator_pair(ch, chars[i]) {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            total_operators += 1;
            operators.insert(token);
        }
    }

    finish_halstead(
        operators.len(),
        operands.len(),
        total_operators,
        total_operands,
    )
}

fn finish_halstead(
    distinct_operators: usize,
    distinct_operands: usize,
    total_operators: usize,
    total_operands: usize,
) -> Halstead {
    let vocabulary = distinct_operators + distinct_operands;
    let length = total_operators + total_operands;
    let volume = if vocabulary <= 1 || length == 0 {
        0.0
    } else {
        length as f64 * (vocabulary as f64).log2()
    };
    let difficulty = (distinct_operators as f64 / 2.0)
        * (total_operands as f64 / distinct_operands.max(1) as f64);
    let effort = difficulty * volume;

    Halstead {
        distinct_operators,
        distinct_operands,
        total_operators,
        total_operands,
        vocabulary,
        length,
        volume: finite_or_zero(volume),
        difficulty: finite_or_zero(difficulty),
        effort: finite_or_zero(effort),
    }
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn is_operator_pair(a: char, b: char) -> bool {
    matches!(
        (a, b),
        ('&', '&')
            | ('|', '|')
            | ('=', '=')
            | ('!', '=')
            | ('<', '=')
            | ('>', '=')
            | ('+', '=')
            | ('-', '=')
            | ('*', '=')
            | ('/', '=')
            | ('%', '=')
            | ('-', '>')
            | ('=', '>')
            | (':', '=')
            | (':', ':')
    )
}

fn count_word(content: &str, word: &str) -> usize {
    let bytes = content.as_bytes();
    content
        .match_indices(word)
        .filter(|(pos, _)| {
            let p = *pos;
            let before = p == 0 || !is_word_byte(bytes[p - 1]);
            let a = p + word.len();
            let after = a >= bytes.len() || !is_word_byte(bytes[a]);
            before && after
        })
        .count()
}

fn node_text<'a>(node: Node<'_>, content: &'a str) -> &'a str {
    node.utf8_text(content.as_bytes()).unwrap_or("")
}

fn is_function_node(node: Node<'_>, cfg: &LangConfig) -> bool {
    node.is_named() && contains(cfg.function_kinds, node.kind())
}

fn contains(items: &[&str], item: &str) -> bool {
    items.contains(&item)
}

fn same_node(node: Node<'_>, start: usize, end: usize, kind: &str) -> bool {
    node.start_byte() == start && node.end_byte() == end && node.kind() == kind
}

fn is_comment_kind(kind: &str) -> bool {
    kind.contains("comment")
}

fn is_identifier_like(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{FirstClass, detect};
    use crate::parse::parse;
    use std::path::Path;

    fn lines(src: &str) -> LineStats {
        LineStats {
            loc: src.lines().count(),
            sloc: src.lines().filter(|line| !line.trim().is_empty()).count(),
            comment_lines: 0,
            blank_lines: src.lines().filter(|line| line.trim().is_empty()).count(),
            approximate: false,
        }
    }

    fn analyze_first_class(path: &str, fc: FirstClass, src: &str) -> Complexity {
        let lang = detect(Path::new(path)).expect("language should be detected");
        let tree = parse(fc, src).expect("snippet should parse");
        let (complexity, approximate) = analyze(lang, src, Some(&tree), &lines(src));
        assert!(!approximate);
        complexity
    }

    #[test]
    fn rust_ast_complexity_counts_decisions_and_function() {
        let src = r#"
fn sample(a: bool, b: bool, xs: Vec<i32>) {
    if a && b {}
    if a {}
    for x in xs { if x > 1 {} }
}
"#;
        let complexity = analyze_first_class("x.rs", FirstClass::Rust, src);
        assert_eq!(complexity.cyclomatic, 6);
        assert_eq!(complexity.functions.len(), 1);
        assert_eq!(complexity.functions[0].name, "sample");
        assert_eq!(complexity.functions[0].cyclomatic, 6);
        assert!(complexity.functions[0].cognitive >= 5);
        assert_eq!(complexity.max_nesting, 2);
        assert!(complexity.halstead.length > 0);
        assert!(complexity.maintainability_index > 0.0);
    }

    #[test]
    fn python_ast_complexity_counts_elif_except_and_boolean_sequence() {
        let src = r#"
def sample(a, b, xs):
    if a and b:
        pass
    elif a:
        pass
    try:
        for x in xs:
            pass
    except ValueError:
        pass
"#;
        let complexity = analyze_first_class("x.py", FirstClass::Python, src);
        assert_eq!(complexity.cyclomatic, 6);
        assert_eq!(complexity.functions[0].name, "sample");
        assert_eq!(complexity.functions[0].cyclomatic, 6);
        assert!(complexity.functions[0].cognitive >= 5);
        assert_eq!(complexity.max_nesting, 1);
    }

    #[test]
    fn javascript_ast_complexity_counts_switch_case_ternary_and_arrow_function() {
        let src = r#"
const sample = (a, b, x) => {
  if (a || b) { return x ? 1 : 2; }
  switch (x) { case 1: return 1; default: return 0; }
};
"#;
        let complexity = analyze_first_class("x.js", FirstClass::JavaScript, src);
        assert_eq!(complexity.cyclomatic, 5);
        assert_eq!(complexity.functions.len(), 1);
        assert_eq!(complexity.functions[0].name, "sample");
        assert_eq!(complexity.functions[0].cyclomatic, 5);
        assert!(complexity.functions[0].cognitive >= 4);
    }

    #[test]
    fn go_ast_complexity_counts_for_switch_case_and_boolean_operator() {
        let src = r#"
package main
func sample(a bool, b bool, x int) int {
    if a && b { return 1 }
    for i := 0; i < x; i++ { }
    switch x { case 1: return 1; default: return 0 }
    return 0
}
"#;
        let complexity = analyze_first_class("x.go", FirstClass::Go, src);
        assert_eq!(complexity.cyclomatic, 5);
        assert_eq!(complexity.functions.len(), 1);
        assert_eq!(complexity.functions[0].name, "sample");
        assert_eq!(complexity.functions[0].cyclomatic, 5);
        assert!(complexity.functions[0].cognitive >= 4);
    }

    #[test]
    fn php_ast_complexity_counts_functions_methods_closures_and_modern_paths() {
        let src = r#"<?php
function classify(bool $a, bool $b, array $items): int {
    if ($a && $b) {
        foreach ($items as $item) {
            if ($item > 0) return $item;
        }
    } elseif ($a) {
        return 1;
    }
    return $a ?? false ? 1 : 0;
}

final class Service {
    public function run(int $value): int {
        return match ($value) {
            1, 2 => 1,
            default => 0,
        };
    }
}

$chooser = fn (bool $value) => $value ? 1 : 0;
$worker = function (bool $value): int {
    while ($value) break;
    return 0;
};
"#;
        let complexity = analyze_first_class("x.php", FirstClass::Php, src);

        assert_eq!(
            complexity
                .functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            ["classify", "run", "chooser", "worker"]
        );
        assert!(complexity.functions[0].cyclomatic >= 7);
        assert_eq!(complexity.functions[1].cyclomatic, 2);
        assert_eq!(complexity.functions[2].cyclomatic, 2);
        assert_eq!(complexity.functions[3].cyclomatic, 2);
        assert!(complexity.max_nesting >= 2);
    }

    #[test]
    fn rust_binding_match_arms_are_catch_alls() {
        let src = r#"
fn classify(value: Option<i32>) {
    match value {
        Some(1) => {},
        other => {},
    }
}
"#;
        let complexity = analyze_first_class("x.rs", FirstClass::Rust, src);
        assert_eq!(complexity.functions[0].cyclomatic, 2);
    }

    #[test]
    fn python_unguarded_capture_case_is_a_catch_all() {
        let src = r#"
def classify(value):
    match value:
        case 1:
            pass
        case other:
            pass
"#;
        let complexity = analyze_first_class("x.py", FirstClass::Python, src);
        assert_eq!(complexity.functions[0].cyclomatic, 2);
    }

    #[test]
    fn python_guarded_capture_case_is_a_decision() {
        let src = r#"
def classify(value):
    match value:
        case other if other > 0:
            pass
"#;
        let complexity = analyze_first_class("x.py", FirstClass::Python, src);
        // One path for the case and another for its guard.
        assert_eq!(complexity.functions[0].cyclomatic, 3);
    }

    #[test]
    fn anonymous_callable_scopes_are_separate_and_binding_named() {
        let cases = [
            (
                "x.rs",
                FirstClass::Rust,
                "fn outer() {\n    let chooser = |x: bool| if x { 1 } else { 0 };\n}\n",
            ),
            (
                "x.py",
                FirstClass::Python,
                "def outer():\n    chooser = lambda x: 1 if x else 0\n",
            ),
            (
                "x.go",
                FirstClass::Go,
                "package main\nfunc outer() {\n    chooser := func(x bool) int { if x { return 1 }; return 0 }\n    _ = chooser\n}\n",
            ),
            (
                "x.php",
                FirstClass::Php,
                "<?php\nfunction outer(): void {\n    $chooser = fn (bool $x) => $x ? 1 : 0;\n}\n",
            ),
        ];

        for (path, language, source) in cases {
            let complexity = analyze_first_class(path, language, source);
            assert_eq!(complexity.functions.len(), 2, "{path}");
            assert_eq!(complexity.functions[0].name, "outer", "{path}");
            assert_eq!(complexity.functions[0].cyclomatic, 1, "{path}");
            assert_eq!(complexity.functions[1].name, "chooser", "{path}");
            assert_eq!(complexity.functions[1].cyclomatic, 2, "{path}");
        }
    }

    #[test]
    fn file_cyclomatic_is_the_sum_of_independent_function_scopes() {
        let complexity =
            analyze_first_class("x.rs", FirstClass::Rust, "fn first() {}\nfn second() {}\n");

        assert_eq!(complexity.cyclomatic, 2);
        assert_eq!(
            complexity.cyclomatic,
            complexity
                .functions
                .iter()
                .map(|function| function.cyclomatic)
                .sum::<u32>()
        );
    }

    #[test]
    fn python_comprehension_clauses_create_control_flow_paths() {
        let complexity = analyze_first_class(
            "x.py",
            FirstClass::Python,
            "def positives(values):\n    return [value for value in values if value > 0]\n",
        );

        assert_eq!(complexity.functions[0].cyclomatic, 3);
        assert_eq!(complexity.functions[0].cognitive, 2);
    }

    #[test]
    fn javascript_modern_short_circuit_paths_match_eslint_rules() {
        let source = r#"
function sample(input = {}) {
  input ||= {};
  return input?.first?.second ?? null;
}
"#;
        let complexity = analyze_first_class("x.js", FirstClass::JavaScript, source);

        // Base path, default parameter, logical assignment, two optional
        // properties, and nullish coalescing.
        assert_eq!(complexity.functions[0].cyclomatic, 6);
    }

    #[test]
    fn direct_recursion_increments_cognitive_complexity() {
        let source = r#"
fn factorial(value: u32) -> u32 {
    if value == 0 { 1 } else { value * factorial(value - 1) }
}
"#;
        let complexity = analyze_first_class("x.rs", FirstClass::Rust, source);

        assert_eq!(complexity.functions[0].cyclomatic, 2);
        assert_eq!(complexity.functions[0].cognitive, 3);
    }

    #[test]
    fn php_self_method_recursion_is_cognitive_but_other_receivers_are_not() {
        let source = r#"<?php
final class Tree {
    public function walk(int $depth, Tree $other): int {
        if ($depth <= 0) return 0;
        $other->walk($depth - 1, $other);
        return $this->walk($depth - 1, $other);
    }
}
"#;
        let complexity = analyze_first_class("x.php", FirstClass::Php, source);

        assert_eq!(complexity.functions[0].cyclomatic, 2);
        assert_eq!(complexity.functions[0].cognitive, 2);
    }

    #[test]
    fn halstead_arithmetic_matches_reference_equations() {
        let halstead = finish_halstead(25, 12, 46, 26);

        assert_eq!(halstead.vocabulary, 37);
        assert_eq!(halstead.length, 72);
        assert!((halstead.volume - 375.080_642_325_284_43).abs() < 1e-9);
        assert!((halstead.difficulty - 27.083_333_333_333_332).abs() < 1e-9);
        assert!((halstead.effort - 10_158.434_062_976_452).abs() < 1e-9);
    }

    #[test]
    fn maintainability_index_matches_microsoft_normalized_formula() {
        let halstead = Halstead {
            volume: 100.0,
            ..Halstead::default()
        };
        let stats = LineStats {
            loc: 60,
            sloc: 50,
            comment_lines: 10,
            blank_lines: 0,
            approximate: false,
        };

        let value = maintainability_index(&halstead, 10, &stats);
        assert!((value - 47.589_673_885_921_606).abs() < 1e-12);

        let without_comments = LineStats {
            comment_lines: 0,
            ..stats
        };
        assert_eq!(
            value,
            maintainability_index(&halstead, 10, &without_comments)
        );
    }

    #[test]
    fn heuristic_fallback_is_marked_approximate_and_populates_halstead() {
        let lang = detect(Path::new("x.java")).expect("generic language should be detected");
        let src = "if (a && b) { for (x in y) { call(); } }";
        let (complexity, approximate) = analyze(lang, src, None, &lines(src));
        assert!(approximate);
        assert_eq!(complexity.cyclomatic, 4);
        assert_eq!(complexity.cognitive, 3);
        assert!(complexity.max_nesting > 0);
        assert!(complexity.halstead.length > 0);
        assert!(complexity.functions.is_empty());
    }
}
