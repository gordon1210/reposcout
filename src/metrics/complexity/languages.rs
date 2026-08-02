use super::FirstClass;

#[derive(Clone, Copy)]
#[expect(
    clippy::struct_field_names,
    reason = "the repeated suffix makes each grammar-node category explicit at every call site"
)]
pub(super) struct LangConfig {
    pub(super) function_kinds: &'static [&'static str],
    pub(super) decision_kinds: &'static [&'static str],
    pub(super) case_kinds: &'static [&'static str],
    pub(super) cognitive_structure_kinds: &'static [&'static str],
    pub(super) nesting_kinds: &'static [&'static str],
    pub(super) else_clause_kinds: &'static [&'static str],
    pub(super) jump_kinds: &'static [&'static str],
}

#[derive(Default)]
pub(super) struct ScopeMetrics {
    pub(super) decision_points: u32,
    pub(super) cognitive: u32,
    pub(super) max_nesting: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenClass {
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

pub(super) fn config(fc: FirstClass) -> LangConfig {
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

pub(super) fn is_function_kind(kind: &str) -> bool {
    RUST_FUNCTIONS.contains(&kind)
        || PY_FUNCTIONS.contains(&kind)
        || JS_FUNCTIONS.contains(&kind)
        || GO_FUNCTIONS.contains(&kind)
        || PHP_FUNCTIONS.contains(&kind)
}
