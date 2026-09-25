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
    assert!(!tree.root_node().has_error(), "invalid syntax in {path}");
    let (complexity, approximate) = analyze(lang, src, Some(&tree), &lines(src));
    assert!(!approximate);
    complexity
}

#[test]
fn rust_ast_complexity_counts_decisions_and_function() {
    let src = r"
fn sample(a: bool, b: bool, xs: Vec<i32>) {
    if a && b {}
    if a {}
    for x in xs { if x > 1 {} }
}
";
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
    let src = r"
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
";
    let complexity = analyze_first_class("x.py", FirstClass::Python, src);
    assert_eq!(complexity.cyclomatic, 6);
    assert_eq!(complexity.functions[0].name, "sample");
    assert_eq!(complexity.functions[0].cyclomatic, 6);
    assert!(complexity.functions[0].cognitive >= 5);
    assert_eq!(complexity.max_nesting, 1);
}

#[test]
fn javascript_ast_complexity_counts_switch_case_ternary_and_arrow_function() {
    let src = r"
const sample = (a, b, x) => {
  if (a || b) { return x ? 1 : 2; }
  switch (x) { case 1: return 1; default: return 0; }
};
";
    let complexity = analyze_first_class("x.js", FirstClass::JavaScript, src);
    assert_eq!(complexity.cyclomatic, 5);
    assert_eq!(complexity.functions.len(), 1);
    assert_eq!(complexity.functions[0].name, "sample");
    assert_eq!(complexity.functions[0].cyclomatic, 5);
    assert!(complexity.functions[0].cognitive >= 4);
}

#[test]
fn go_ast_complexity_counts_for_switch_case_and_boolean_operator() {
    let src = r"
package main
func sample(a bool, b bool, x int) int {
    if a && b { return 1 }
    for i := 0; i < x; i++ { }
    switch x { case 1: return 1; default: return 0 }
    return 0
}
";
    let complexity = analyze_first_class("x.go", FirstClass::Go, src);
    assert_eq!(complexity.cyclomatic, 5);
    assert_eq!(complexity.functions.len(), 1);
    assert_eq!(complexity.functions[0].name, "sample");
    assert_eq!(complexity.functions[0].cyclomatic, 5);
    assert!(complexity.functions[0].cognitive >= 4);
}

#[test]
fn php_ast_complexity_counts_functions_methods_closures_and_modern_paths() {
    let src = r"<?php
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
";
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
fn csharp_ast_complexity_counts_methods_local_functions_and_lambdas() {
    let src = r"
class Service
{
    public int Run(bool a, bool b, int[] values)
    {
        if (a && b) return 1;
        foreach (var value in values)
        {
            if (value > 0) return value;
        }
        int Local(int value) => value > 1 ? value : 0;
        System.Func<int, int> choose = value => value > 0 ? value : 0;
        return Local(choose(0));
    }
}
";
    let complexity = analyze_first_class("x.cs", FirstClass::CSharp, src);
    let functions = complexity
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function.cyclomatic))
        .collect::<Vec<_>>();

    assert!(functions.contains(&("Run", 5)), "{functions:?}");
    assert!(functions.contains(&("Local", 2)), "{functions:?}");
    assert!(functions.contains(&("choose", 2)), "{functions:?}");
    assert!(complexity.max_nesting >= 2);
}

#[test]
fn csharp_else_if_chains_do_not_add_nesting() {
    let src = "class C { int Run(int n) { if (n == 0) return 0; else if (n == 1) return 1; else if (n == 2) return 2; else return 3; } }";
    let complexity = analyze_first_class("x.cs", FirstClass::CSharp, src);
    assert_eq!(complexity.functions[0].cyclomatic, 4);
    assert_eq!(complexity.functions[0].cognitive, 4);
    assert_eq!(complexity.functions[0].max_nesting, 1);
}

#[test]
fn csharp_direct_recursion_adds_cognitive_complexity() {
    let src = "class C { int Run(int n) => n == 0 ? 0 : Run(n - 1); }";
    let complexity = analyze_first_class("x.cs", FirstClass::CSharp, src);
    assert_eq!(complexity.functions[0].cognitive, 2);
}

fn assert_function_metrics(
    complexity: &Complexity,
    name: &str,
    cyclomatic: u32,
    cognitive: u32,
    max_nesting: u32,
) {
    let function = complexity
        .functions
        .iter()
        .find(|function| function.name == name)
        .unwrap_or_else(|| panic!("missing function {name}"));
    assert_eq!(
        (
            function.cyclomatic,
            function.cognitive,
            function.max_nesting
        ),
        (cyclomatic, cognitive, max_nesting),
        "{name}"
    );
}

#[test]
fn rust_and_go_else_if_chains_keep_only_explicit_nesting() {
    let cases = [
        (
            "x.rs",
            FirstClass::Rust,
            "fn chain(n: i32) -> i32 { if n == 0 { 0 } else if n == 1 { 1 } else if n == 2 { 2 } else { 3 } }\nfn nested(n: i32) -> i32 { if n == 0 { 0 } else { if n == 1 { 1 } else { if n == 2 { 2 } else { 3 } } } }\n",
        ),
        (
            "x.go",
            FirstClass::Go,
            "package sample\nfunc chain(n int) int { if n == 0 { return 0 } else if n == 1 { return 1 } else if n == 2 { return 2 } else { return 3 } }\nfunc nested(n int) int { if n == 0 { return 0 } else { if n == 1 { return 1 } else { if n == 2 { return 2 } else { return 3 } } } }\n",
        ),
    ];

    for (path, language, source) in cases {
        let complexity = analyze_first_class(path, language, source);
        assert_function_metrics(&complexity, "chain", 4, 4, 1);
        assert_function_metrics(&complexity, "nested", 4, 9, 3);
    }
}

#[test]
fn rust_let_chains_count_one_boolean_sequence() {
    let source = "fn plain_and(a: Option<u8>, b: bool) -> u8 { if a.is_some() && b { 1 } else { 0 } }\nfn let_and(a: Option<u8>, b: bool) -> u8 { if let Some(x) = a && b { x } else { 0 } }\nfn let_and_and(a: Option<u8>, b: bool, c: bool) -> u8 { if let Some(x) = a && b && c { x } else { 0 } }\nfn while_let_and(mut a: Option<u8>, b: bool) { while let Some(_) = a && b { a = None; } }\nfn multi_let(a: Option<u8>, b: Option<u8>) -> u8 { if let Some(x) = a && let Some(y) = b { x + y } else { 0 } }\nfn let_group(a: Option<u8>, b: bool, c: bool) -> u8 { if let Some(x) = a && (b && c) { x } else { 0 } }\nfn let_mixed(a: Option<u8>, b: bool, c: bool) -> u8 { if let Some(x) = a && (b || c) { x } else { 0 } }\n";
    let complexity = analyze_first_class("x.rs", FirstClass::Rust, source);

    assert_function_metrics(&complexity, "plain_and", 3, 3, 1);
    assert_function_metrics(&complexity, "let_and", 3, 3, 1);
    assert_function_metrics(&complexity, "let_and_and", 4, 3, 1);
    assert_function_metrics(&complexity, "while_let_and", 3, 2, 1);
    assert_function_metrics(&complexity, "multi_let", 3, 3, 1);
    assert_function_metrics(&complexity, "let_group", 4, 3, 1);
    assert_function_metrics(&complexity, "let_mixed", 4, 4, 1);
}

#[test]
fn boolean_groups_count_each_sequence_once_across_languages() {
    let cases = [
        (
            "x.cs",
            FirstClass::CSharp,
            "class C { int mixedFlat(bool a, bool b, bool c) { if (a && b || c) return 1; return 0; } int mixedParen(bool a, bool b, bool c) { if (a && (b || c)) return 1; return 0; } int reversed(bool a, bool b, bool c) { if ((a || b) && c) return 1; return 0; } int doubled(bool a, bool b, bool c) { if (a && ((b || c))) return 1; return 0; } int sameParen(bool a, bool b, bool c) { if (a && (b && c)) return 1; return 0; } int negated(bool a, bool b, bool c) { if (a && !(b && c)) return 1; return 0; } }",
            0,
        ),
        (
            "x.go",
            FirstClass::Go,
            "package sample\nfunc mixedFlat(a, b, c bool) int { if a && b || c { return 1 }; return 0 }\nfunc mixedParen(a, b, c bool) int { if a && (b || c) { return 1 }; return 0 }\nfunc reversed(a, b, c bool) int { if (a || b) && c { return 1 }; return 0 }\nfunc doubled(a, b, c bool) int { if a && ((b || c)) { return 1 }; return 0 }\nfunc sameParen(a, b, c bool) int { if a && (b && c) { return 1 }; return 0 }\nfunc negated(a, b, c bool) int { if a && !(b && c) { return 1 }; return 0 }\n",
            0,
        ),
        (
            "x.js",
            FirstClass::JavaScript,
            "function mixedFlat(a,b,c){if(a&&b||c)return 1;return 0} function mixedParen(a,b,c){if(a&&(b||c))return 1;return 0} function reversed(a,b,c){if((a||b)&&c)return 1;return 0} function doubled(a,b,c){if(a&&((b||c)))return 1;return 0} function sameParen(a,b,c){if(a&&(b&&c))return 1;return 0} function negated(a,b,c){if(a&&!(b&&c))return 1;return 0}",
            0,
        ),
        (
            "x.ts",
            FirstClass::TypeScript,
            "function mixedFlat(a:boolean,b:boolean,c:boolean){if(a&&b||c)return 1;return 0} function mixedParen(a:boolean,b:boolean,c:boolean){if(a&&(b||c))return 1;return 0} function reversed(a:boolean,b:boolean,c:boolean){if((a||b)&&c)return 1;return 0} function doubled(a:boolean,b:boolean,c:boolean){if(a&&((b||c)))return 1;return 0} function sameParen(a:boolean,b:boolean,c:boolean){if(a&&(b&&c))return 1;return 0} function negated(a:boolean,b:boolean,c:boolean){if(a&&!(b&&c))return 1;return 0}",
            0,
        ),
        (
            "x.tsx",
            FirstClass::Tsx,
            "function mixedFlat(a:boolean,b:boolean,c:boolean){if(a&&b||c)return 1;return 0} function mixedParen(a:boolean,b:boolean,c:boolean){if(a&&(b||c))return 1;return 0} function reversed(a:boolean,b:boolean,c:boolean){if((a||b)&&c)return 1;return 0} function doubled(a:boolean,b:boolean,c:boolean){if(a&&((b||c)))return 1;return 0} function sameParen(a:boolean,b:boolean,c:boolean){if(a&&(b&&c))return 1;return 0} function negated(a:boolean,b:boolean,c:boolean){if(a&&!(b&&c))return 1;return 0}",
            0,
        ),
        (
            "x.php",
            FirstClass::Php,
            "<?php function mixedFlat($a,$b,$c){if($a&&$b||$c)return 1;return 0;} function mixedParen($a,$b,$c){if($a&&($b||$c))return 1;return 0;} function reversed($a,$b,$c){if(($a||$b)&&$c)return 1;return 0;} function doubled($a,$b,$c){if($a&&(($b||$c)))return 1;return 0;} function sameParen($a,$b,$c){if($a&&($b&&$c))return 1;return 0;} function negated($a,$b,$c){if($a&&!($b&&$c))return 1;return 0;}",
            0,
        ),
        (
            "x.py",
            FirstClass::Python,
            "def mixedFlat(a,b,c):\n    if a and b or c: return 1\n    return 0\ndef mixedParen(a,b,c):\n    if a and (b or c): return 1\n    return 0\ndef reversed(a,b,c):\n    if (a or b) and c: return 1\n    return 0\ndef doubled(a,b,c):\n    if a and ((b or c)): return 1\n    return 0\ndef sameParen(a,b,c):\n    if a and (b and c): return 1\n    return 0\ndef negated(a,b,c):\n    if a and not (b and c): return 1\n    return 0\n",
            0,
        ),
        (
            "x.rs",
            FirstClass::Rust,
            "fn mixedFlat(a:bool,b:bool,c:bool)->u8{if a&&b||c{1}else{0}} fn mixedParen(a:bool,b:bool,c:bool)->u8{if a&&(b||c){1}else{0}} fn reversed(a:bool,b:bool,c:bool)->u8{if (a||b)&&c{1}else{0}} fn doubled(a:bool,b:bool,c:bool)->u8{if a&&((b||c)){1}else{0}} fn sameParen(a:bool,b:bool,c:bool)->u8{if a&&(b&&c){1}else{0}} fn negated(a:bool,b:bool,c:bool)->u8{if a&&!(b&&c){1}else{0}}",
            1,
        ),
        (
            "x.gd",
            FirstClass::GdScript,
            "func mixedFlat(a: bool, b: bool, c: bool) -> int:\n    if a and b or c: return 1\n    return 0\nfunc mixedParen(a: bool, b: bool, c: bool) -> int:\n    if a and (b or c): return 1\n    return 0\nfunc reversed(a: bool, b: bool, c: bool) -> int:\n    if (a or b) and c: return 1\n    return 0\nfunc doubled(a: bool, b: bool, c: bool) -> int:\n    if a and ((b or c)): return 1\n    return 0\nfunc sameParen(a: bool, b: bool, c: bool) -> int:\n    if a and (b and c): return 1\n    return 0\nfunc negated(a: bool, b: bool, c: bool) -> int:\n    if a and not (b and c): return 1\n    return 0\n",
            0,
        ),
        (
            "x.gdshader",
            FirstClass::GdShader,
            "shader_type canvas_item;\nint mixedFlat(bool a, bool b, bool c) { if (a && b || c) return 1; return 0; }\nint mixedParen(bool a, bool b, bool c) { if (a && (b || c)) return 1; return 0; }\nint reversed(bool a, bool b, bool c) { if ((a || b) && c) return 1; return 0; }\nint doubled(bool a, bool b, bool c) { if (a && ((b || c))) return 1; return 0; }\nint sameParen(bool a, bool b, bool c) { if (a && (b && c)) return 1; return 0; }\nint negated(bool a, bool b, bool c) { if (a && !(b && c)) return 1; return 0; }\n",
            0,
        ),
    ];

    for (path, language, source, rust_else) in cases {
        let complexity = analyze_first_class(path, language, source);
        for name in ["mixedFlat", "mixedParen", "reversed", "doubled", "negated"] {
            assert_function_metrics(&complexity, name, 4, 3 + rust_else, 1);
        }
        assert_function_metrics(&complexity, "sameParen", 4, 2 + rust_else, 1);
    }
}

#[test]
fn boolean_operators_inside_calls_and_lambdas_have_separate_owners() {
    let source = "function check(a,b){return a||b} function called(a,b,c){if(a&&check(b||c))return 1;return 0} function enclosing(a,b,c){return a&&[b].some(x=>x||c)}";
    let complexity = analyze_first_class("x.js", FirstClass::JavaScript, source);

    assert_function_metrics(&complexity, "called", 4, 3, 1);
    assert_function_metrics(&complexity, "enclosing", 2, 1, 0);
    assert_eq!(
        complexity
            .functions
            .iter()
            .filter(|function| function.cognitive == 1 && function.cyclomatic == 2)
            .count(),
        3
    );
}

#[test]
fn rust_binding_match_arms_are_catch_alls() {
    let src = r"
fn classify(value: Option<i32>) {
    match value {
        Some(1) => {},
        other => {},
    }
}
";
    let complexity = analyze_first_class("x.rs", FirstClass::Rust, src);
    assert_eq!(complexity.functions[0].cyclomatic, 2);
}

#[test]
fn python_unguarded_capture_case_is_a_catch_all() {
    let src = r"
def classify(value):
    match value:
        case 1:
            pass
        case other:
            pass
";
    let complexity = analyze_first_class("x.py", FirstClass::Python, src);
    assert_eq!(complexity.functions[0].cyclomatic, 2);
}

#[test]
fn python_guarded_capture_case_is_a_decision() {
    let src = r"
def classify(value):
    match value:
        case other if other > 0:
            pass
";
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
    let source = r"
function sample(input = {}) {
  input ||= {};
  return input?.first?.second ?? null;
}
";
    let complexity = analyze_first_class("x.js", FirstClass::JavaScript, source);

    // Base path, default parameter, logical assignment, two optional
    // properties, and nullish coalescing.
    assert_eq!(complexity.functions[0].cyclomatic, 6);
}

#[test]
fn direct_recursion_increments_cognitive_complexity() {
    let source = r"
fn factorial(value: u32) -> u32 {
    if value == 0 { 1 } else { value * factorial(value - 1) }
}
";
    let complexity = analyze_first_class("x.rs", FirstClass::Rust, source);

    assert_eq!(complexity.functions[0].cyclomatic, 2);
    assert_eq!(complexity.functions[0].cognitive, 3);
}

#[test]
fn php_self_method_recursion_is_cognitive_but_other_receivers_are_not() {
    let source = r"<?php
final class Tree {
    public function walk(int $depth, Tree $other): int {
        if ($depth <= 0) return 0;
        $other->walk($depth - 1, $other);
        return $this->walk($depth - 1, $other);
    }
}
";
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
    assert!((value - maintainability_index(&halstead, 10, &without_comments)).abs() < f64::EPSILON);
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
