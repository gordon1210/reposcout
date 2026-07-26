//! Symbol / public-API counts derived from the tree-sitter AST.
//!
//! Counts three categories for each first-class language:
//! - `functions`: named callable definitions
//! - `types`:     type/class/interface/enum/struct declarations
//! - `exports`:   publicly accessible symbols (language-specific heuristic)

use crate::lang::FirstClass;
use crate::model::{SymbolCounts, SymbolOutline};
use std::collections::HashSet;
use tree_sitter::{Node, Tree};

const MAX_SIGNATURE_CHARS: usize = 280;

macro_rules! define_outline_kinds {
    ($($variant:ident => $label:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        enum OutlineKind {
            $($variant),+
        }

        impl OutlineKind {
            fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $label),+
                }
            }
        }

        /// Declaration kinds produced by first-class symbol outlines and accepted by
        /// `reposcout locate --kind`. Capability discovery reuses this source of truth.
        pub const OUTLINE_KINDS: &[&str] = &[$($label),+];
    };
}

define_outline_kinds! {
    Class => "class",
    Enum => "enum",
    Function => "function",
    Interface => "interface",
    Method => "method",
    Trait => "trait",
    Type => "type",
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SymbolAnalysis {
    pub(crate) counts: SymbolCounts,
    pub(crate) outlines: Vec<SymbolOutline>,
}

/// Analyze structural declarations once, retaining the historical aggregate
/// counts plus compact declaration headers for context-plan projection.
pub(crate) fn analyze(fc: FirstClass, content: &str, tree: &Tree) -> SymbolAnalysis {
    SymbolAnalysis {
        counts: count(fc, content, tree),
        outlines: outline(fc, content, tree),
    }
}

/// Count structural symbols in `tree` for `fc`.  `content` is the source text
/// used to resolve identifier names for export detection.
pub fn count(fc: FirstClass, content: &str, tree: &Tree) -> SymbolCounts {
    let src = content.as_bytes();
    let root = tree.root_node();
    match fc {
        FirstClass::Rust => count_rust(root),
        FirstClass::Python => count_python(root, src),
        FirstClass::JavaScript => count_javascript(root),
        FirstClass::TypeScript | FirstClass::Tsx => count_typescript(root),
        FirstClass::Go => count_go(root, src),
        FirstClass::Php => count_php(root, src),
    }
}

fn outline(fc: FirstClass, content: &str, tree: &Tree) -> Vec<SymbolOutline> {
    let src = content.as_bytes();
    let named_exports = named_module_exports(fc, tree.root_node(), src);
    let mut outlines = Vec::new();
    let mut seen = HashSet::new();
    walk(tree.root_node(), |node| {
        let Some((kind, declaration)) = declaration_kind(fc, node) else {
            return;
        };
        let Some(base_name) = declaration_name(node, src) else {
            return;
        };
        let name = qualified_name(fc, node, &base_name, src);
        let line = node.start_position().row + 1;
        if !seen.insert((line, name.clone(), kind)) {
            return;
        }
        let exported = declaration_exported(fc, node, &base_name, src, &named_exports);
        let reason = if exported {
            if fc == FirstClass::Python {
                "public-name heuristic"
            } else {
                "exported/public declaration"
            }
        } else {
            "representative file-local declaration"
        };
        outlines.push(SymbolOutline {
            name,
            kind: kind.as_str().to_string(),
            signature: signature_text(fc, declaration, content),
            line,
            exported,
            reasons: vec![reason.to_string()],
        });
    });
    outlines.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.name.cmp(&right.name))
    });
    outlines
}

fn declaration_kind(fc: FirstClass, node: Node<'_>) -> Option<(OutlineKind, Node<'_>)> {
    let kind = match fc {
        FirstClass::Rust => match node.kind() {
            "function_item" | "function_signature_item" => callable_kind(node),
            "struct_item" | "union_item" | "type_item" => OutlineKind::Type,
            "enum_item" => OutlineKind::Enum,
            "trait_item" => OutlineKind::Trait,
            _ => return None,
        },
        FirstClass::Python => match node.kind() {
            "function_definition" => callable_kind(node),
            "class_definition" => OutlineKind::Class,
            _ => return None,
        },
        FirstClass::JavaScript => match node.kind() {
            "function_declaration" | "generator_function_declaration" => OutlineKind::Function,
            "method_definition" => OutlineKind::Method,
            "class_declaration" => OutlineKind::Class,
            "variable_declarator" if variable_callable(node) => OutlineKind::Function,
            _ => return None,
        },
        FirstClass::TypeScript | FirstClass::Tsx => match node.kind() {
            "function_declaration" | "generator_function_declaration" => OutlineKind::Function,
            "method_definition" | "method_signature" => OutlineKind::Method,
            "class_declaration" => OutlineKind::Class,
            "interface_declaration" => OutlineKind::Interface,
            "type_alias_declaration" => OutlineKind::Type,
            "enum_declaration" => OutlineKind::Enum,
            "variable_declarator" if variable_callable(node) => OutlineKind::Function,
            _ => return None,
        },
        FirstClass::Go => match node.kind() {
            "function_declaration" => OutlineKind::Function,
            "method_declaration" => OutlineKind::Method,
            "type_spec" | "type_alias" => OutlineKind::Type,
            _ => return None,
        },
        FirstClass::Php => match node.kind() {
            "function_definition" => OutlineKind::Function,
            "method_declaration" => OutlineKind::Method,
            "class_declaration" => OutlineKind::Class,
            "interface_declaration" => OutlineKind::Interface,
            "trait_declaration" => OutlineKind::Trait,
            "enum_declaration" => OutlineKind::Enum,
            _ => return None,
        },
    };
    let declaration = if fc == FirstClass::Python {
        node.parent()
            .filter(|parent| parent.kind() == "decorated_definition")
            .unwrap_or(node)
    } else {
        node
    };
    Some((kind, declaration))
}

fn variable_callable(node: Node<'_>) -> bool {
    node.child_by_field_name("value").is_some_and(|value| {
        matches!(
            value.kind(),
            "arrow_function" | "function_expression" | "generator_function"
        )
    })
}

fn callable_kind(node: Node<'_>) -> OutlineKind {
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        if is_callable_scope(candidate.kind()) {
            return OutlineKind::Function;
        }
        if matches!(
            candidate.kind(),
            "class_definition"
                | "class_declaration"
                | "impl_item"
                | "trait_item"
                | "trait_declaration"
                | "interface_declaration"
                | "enum_declaration"
        ) {
            return OutlineKind::Method;
        }
        if matches!(candidate.kind(), "source_file" | "program" | "module") {
            break;
        }
        parent = candidate.parent();
    }
    OutlineKind::Function
}

fn declaration_name(node: Node<'_>, src: &[u8]) -> Option<String> {
    name_text(node, src).map(str::to_string).or_else(|| {
        node.child_by_field_name("declarator")
            .and_then(|name| name.utf8_text(src).ok())
            .map(str::to_string)
    })
}

fn qualified_name(fc: FirstClass, node: Node<'_>, name: &str, src: &[u8]) -> String {
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        if is_callable_scope(candidate.kind()) {
            break;
        }
        let is_container = match fc {
            FirstClass::Rust => matches!(candidate.kind(), "impl_item" | "trait_item"),
            FirstClass::Python => candidate.kind() == "class_definition",
            FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx => matches!(
                candidate.kind(),
                "class_declaration" | "interface_declaration"
            ),
            FirstClass::Go => false,
            FirstClass::Php => matches!(
                candidate.kind(),
                "class_declaration"
                    | "interface_declaration"
                    | "trait_declaration"
                    | "enum_declaration"
            ),
        };
        if is_container {
            let container = name_text(candidate, src).or_else(|| {
                candidate
                    .child_by_field_name("type")
                    .and_then(|value| value.utf8_text(src).ok())
            });
            if let Some(container) = container {
                return format!("{container}.{name}");
            }
        }
        parent = candidate.parent();
    }
    name.to_string()
}

fn is_callable_scope(kind: &str) -> bool {
    matches!(
        kind,
        "function_item"
            | "closure_expression"
            | "function_definition"
            | "lambda"
            | "function_declaration"
            | "generator_function_declaration"
            | "function_expression"
            | "generator_function"
            | "arrow_function"
            | "method_definition"
            | "method_declaration"
            | "anonymous_function"
    )
}

fn declaration_exported(
    fc: FirstClass,
    node: Node<'_>,
    name: &str,
    src: &[u8],
    named_exports: &HashSet<String>,
) -> bool {
    match fc {
        FirstClass::Rust => {
            if has_visibility_modifier(node) {
                return true;
            }
            ancestor(node, |candidate| {
                matches!(candidate.kind(), "trait_item") && has_visibility_modifier(candidate)
            })
        }
        FirstClass::Python => python_declaration_is_public(node, name, src),
        FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx => {
            javascript_declaration_is_public(node, name, src, named_exports)
        }
        FirstClass::Go => name.chars().next().is_some_and(char::is_uppercase),
        FirstClass::Php => php_declaration_is_public(node, src),
    }
}

fn php_declaration_is_public(node: Node<'_>, src: &[u8]) -> bool {
    if node.kind() == "method_declaration" {
        return !php_has_non_public_visibility(node, src);
    }

    let mut parent = node.parent();
    while let Some(candidate) = parent {
        match candidate.kind() {
            "function_definition" | "method_declaration" | "anonymous_function" => return false,
            "program" => return true,
            _ => parent = candidate.parent(),
        }
    }
    false
}

fn php_has_non_public_visibility(node: Node<'_>, src: &[u8]) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| {
        child.kind() == "visibility_modifier"
            && child
                .utf8_text(src)
                .is_ok_and(|modifier| matches!(modifier, "private" | "protected"))
    })
}

fn python_declaration_is_public(node: Node<'_>, name: &str, src: &[u8]) -> bool {
    if name.starts_with('_') {
        return false;
    }
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        match candidate.kind() {
            "function_definition" | "lambda" => return false,
            "class_definition" => {
                return name_text(candidate, src).is_some_and(|name| !name.starts_with('_'));
            }
            "module" => return true,
            _ => parent = candidate.parent(),
        }
    }
    false
}

fn javascript_declaration_is_public(
    node: Node<'_>,
    name: &str,
    src: &[u8],
    named_exports: &HashSet<String>,
) -> bool {
    if javascript_member_is_private(node, name, src) {
        return false;
    }
    if matches!(node.kind(), "method_definition" | "method_signature") {
        return exported_javascript_container(node, src, named_exports);
    }
    if named_exports.contains(name) && javascript_declaration_is_module_level(node) {
        return true;
    }

    let mut parent = node.parent();
    while let Some(candidate) = parent {
        match candidate.kind() {
            "export_statement" => return true,
            "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "class_declaration"
            | "internal_module" => return false,
            "program" => return false,
            _ => parent = candidate.parent(),
        }
    }
    false
}

fn javascript_declaration_is_module_level(node: Node<'_>) -> bool {
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        match candidate.kind() {
            "function_declaration"
            | "generator_function_declaration"
            | "method_definition"
            | "class_declaration"
            | "internal_module" => return false,
            "program" => return true,
            _ => parent = candidate.parent(),
        }
    }
    false
}

fn javascript_member_is_private(node: Node<'_>, name: &str, src: &[u8]) -> bool {
    if !matches!(node.kind(), "method_definition" | "method_signature") {
        return false;
    }
    if name.starts_with('#') {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| {
        child.kind() == "accessibility_modifier"
            && child
                .utf8_text(src)
                .is_ok_and(|modifier| matches!(modifier, "private" | "protected"))
    })
}

fn exported_javascript_container(
    node: Node<'_>,
    src: &[u8],
    named_exports: &HashSet<String>,
) -> bool {
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        if matches!(
            candidate.kind(),
            "class_declaration" | "interface_declaration"
        ) {
            return ancestor(candidate, |parent| parent.kind() == "export_statement")
                || name_text(candidate, src).is_some_and(|name| named_exports.contains(name));
        }
        if matches!(
            candidate.kind(),
            "function_declaration" | "generator_function_declaration" | "method_definition"
        ) {
            return false;
        }
        parent = candidate.parent();
    }
    false
}

fn ancestor(mut node: Node<'_>, predicate: impl Fn(Node<'_>) -> bool) -> bool {
    while let Some(parent) = node.parent() {
        if predicate(parent) {
            return true;
        }
        node = parent;
    }
    false
}

fn named_module_exports(fc: FirstClass, root: Node<'_>, src: &[u8]) -> HashSet<String> {
    if !matches!(
        fc,
        FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx
    ) {
        return HashSet::new();
    }
    let mut exported = HashSet::new();
    walk(root, |candidate| {
        if candidate.kind() != "export_specifier" {
            return;
        }
        if let Some(name) = candidate
            .child_by_field_name("name")
            .or_else(|| candidate.named_child(0))
            .and_then(|value| value.utf8_text(src).ok())
        {
            exported.insert(name.to_string());
        }
    });
    exported
}

fn signature_text(fc: FirstClass, node: Node<'_>, content: &str) -> String {
    let header = if fc == FirstClass::Python && node.kind() == "decorated_definition" {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|child| matches!(child.kind(), "function_definition" | "class_definition"))
            .unwrap_or(node)
    } else {
        node
    };
    let mut end = node.end_byte();
    let mut omitted_body = false;
    if let Some(body) = header.child_by_field_name("body").or_else(|| {
        header
            .child_by_field_name("value")
            .and_then(|value| value.child_by_field_name("body"))
    }) {
        end = if fc == FirstClass::Python {
            let mut cursor = header.walk();
            header
                .children(&mut cursor)
                .filter(|child| child.kind() == ":")
                .last()
                .map(|colon| colon.end_byte())
                .unwrap_or_else(|| body.start_byte())
        } else {
            body.start_byte()
        };
        omitted_body = true;
    }
    let raw = content.get(node.start_byte()..end).unwrap_or_default();
    let mut signature = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if signature.chars().count() > MAX_SIGNATURE_CHARS {
        signature = signature.chars().take(MAX_SIGNATURE_CHARS).collect();
        signature.push('…');
    } else if omitted_body {
        signature.push_str(" …");
    }
    signature
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn walk<F: FnMut(Node<'_>)>(root: Node<'_>, mut visit: F) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        visit(node);
        for i in (0..node.child_count()).rev() {
            if let Some(child) = node.child(i as u32) {
                stack.push(child);
            }
        }
    }
}

/// True if `node` has a direct child whose kind is `visibility_modifier`
/// (Rust's `pub` / `pub(crate)` / …).
fn has_visibility_modifier(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|c| c.kind() == "visibility_modifier")
}

/// Retrieve the `name` field text of a node, returning `None` on failure.
fn name_text<'a>(node: Node<'_>, src: &'a [u8]) -> Option<&'a str> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())
}

// ── per-language counters ─────────────────────────────────────────────────────

fn count_rust(root: Node<'_>) -> SymbolCounts {
    let mut counts = SymbolCounts::default();
    walk(root, |node| {
        let kind = node.kind();
        let is_fn = kind == "function_item";
        let is_ty = matches!(
            kind,
            "struct_item" | "enum_item" | "trait_item" | "type_item" | "union_item"
        );
        if is_fn {
            counts.functions += 1;
        }
        if is_ty {
            counts.types += 1;
        }
        if (is_fn || is_ty) && has_visibility_modifier(node) {
            counts.exports += 1;
        }
    });
    counts
}

fn count_python(root: Node<'_>, src: &[u8]) -> SymbolCounts {
    let mut counts = SymbolCounts::default();
    walk(root, |node| match node.kind() {
        "function_definition" => {
            counts.functions += 1;
            if name_text(node, src)
                .map(|n| !n.starts_with('_'))
                .unwrap_or(false)
            {
                counts.exports += 1;
            }
        }
        "class_definition" => {
            counts.types += 1;
            if name_text(node, src)
                .map(|n| !n.starts_with('_'))
                .unwrap_or(false)
            {
                counts.exports += 1;
            }
        }
        _ => {}
    });
    counts
}

fn count_javascript(root: Node<'_>) -> SymbolCounts {
    let mut counts = SymbolCounts::default();
    walk(root, |node| match node.kind() {
        "function_declaration" | "method_definition" | "generator_function_declaration" => {
            counts.functions += 1;
        }
        "class_declaration" => {
            counts.types += 1;
        }
        "export_statement" => {
            counts.exports += 1;
        }
        _ => {}
    });
    counts
}

fn count_typescript(root: Node<'_>) -> SymbolCounts {
    let mut counts = SymbolCounts::default();
    walk(root, |node| match node.kind() {
        "function_declaration" | "method_definition" => {
            counts.functions += 1;
        }
        "class_declaration"
        | "interface_declaration"
        | "type_alias_declaration"
        | "enum_declaration" => {
            counts.types += 1;
        }
        "export_statement" => {
            counts.exports += 1;
        }
        _ => {}
    });
    counts
}

fn count_go(root: Node<'_>, src: &[u8]) -> SymbolCounts {
    let mut counts = SymbolCounts::default();
    walk(root, |node| match node.kind() {
        "function_declaration" | "method_declaration" => {
            counts.functions += 1;
            if name_text(node, src)
                .and_then(|n| n.chars().next())
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
            {
                counts.exports += 1;
            }
        }
        "type_spec" | "type_alias" => {
            counts.types += 1;
            if name_text(node, src)
                .and_then(|n| n.chars().next())
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
            {
                counts.exports += 1;
            }
        }
        _ => {}
    });
    counts
}

fn count_php(root: Node<'_>, src: &[u8]) -> SymbolCounts {
    let mut counts = SymbolCounts::default();
    walk(root, |node| {
        let is_function = matches!(node.kind(), "function_definition" | "method_declaration");
        let is_type = matches!(
            node.kind(),
            "class_declaration"
                | "interface_declaration"
                | "trait_declaration"
                | "enum_declaration"
        );
        if is_function {
            counts.functions += 1;
        }
        if is_type {
            counts.types += 1;
        }
        if (is_function || is_type) && php_declaration_is_public(node, src) {
            counts.exports += 1;
        }
    });
    counts
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use std::collections::BTreeSet;

    #[test]
    fn outline_kinds_are_a_closed_capability_set() {
        let samples = [
            (
                FirstClass::Rust,
                r#"
pub fn run() {}
pub struct Service;
pub enum State { Ready }
pub trait Runner { fn execute(&self); }
impl Runner for Service { fn execute(&self) {} }
"#,
            ),
            (
                FirstClass::TypeScript,
                r#"
export class Client { request(): void {} }
export interface Transport { send(): void; }
export type Identifier = string;
export enum Mode { Fast }
export function connect(): void {}
"#,
            ),
        ];
        let mut actual = BTreeSet::new();
        for (language, source) in samples {
            let tree = parse::parse(language, source).unwrap();
            actual.extend(
                analyze(language, source, &tree)
                    .outlines
                    .into_iter()
                    .map(|outline| outline.kind),
            );
        }
        let expected = OUTLINE_KINDS
            .iter()
            .map(|kind| (*kind).to_string())
            .collect::<BTreeSet<_>>();

        assert_eq!(
            expected.len(),
            OUTLINE_KINDS.len(),
            "duplicate outline kind"
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn rust_basic_counts() {
        let src = r#"
pub fn public_func() {}
fn private_func() {}
pub struct MyStruct {}
"#;
        let tree = parse::parse(FirstClass::Rust, src).unwrap();
        let counts = count(FirstClass::Rust, src, &tree);
        assert_eq!(counts.functions, 2, "two functions");
        assert_eq!(counts.types, 1, "one struct");
        assert_eq!(counts.exports, 2, "pub fn + pub struct");
    }

    #[test]
    fn typescript_interface_and_export() {
        let src = r#"
export interface Foo {
    bar: string;
}
function baz() {}
"#;
        let tree = parse::parse(FirstClass::TypeScript, src).unwrap();
        let counts = count(FirstClass::TypeScript, src, &tree);
        assert!(counts.types >= 1, "interface should count as a type");
        assert!(counts.exports >= 1, "export_statement should count");
    }

    #[test]
    fn go_grouped_types_are_counted_individually() {
        let src = r#"
package sample
type (
    Exported struct{}
    internal int
    Alias = string
)
"#;
        let tree = parse::parse(FirstClass::Go, src).unwrap();
        let counts = count(FirstClass::Go, src, &tree);

        assert_eq!(counts.types, 3);
        assert_eq!(counts.exports, 2);
    }

    #[test]
    fn php_counts_named_symbols_and_public_api() {
        let src = r#"<?php
function helper(): void {}

interface Runner { public function run(): void; }
trait Logs { protected function log(): void {} }
enum Status { case Ready; }
final class Service implements Runner {
    public function run(): void {}
    private function secret(): void {}
}
"#;
        let tree = parse::parse(FirstClass::Php, src).unwrap();
        let analysis = analyze(FirstClass::Php, src, &tree);

        assert_eq!(analysis.counts.functions, 5);
        assert_eq!(analysis.counts.types, 4);
        assert_eq!(analysis.counts.exports, 7);
        assert!(analysis.outlines.iter().any(|symbol| {
            symbol.name == "Service.run" && symbol.kind == "method" && symbol.exported
        }));
        assert!(analysis.outlines.iter().any(|symbol| {
            symbol.name == "Service.secret" && symbol.kind == "method" && !symbol.exported
        }));
        assert!(
            analysis
                .outlines
                .iter()
                .all(|symbol| !symbol.signature.contains("secret body"))
        );
    }

    #[test]
    fn rust_outlines_keep_headers_and_drop_bodies() {
        let src = r#"
pub struct Request {
    pub value: String,
}

pub fn execute(request: Request) -> usize {
    let secret_body = request.value.len();
    secret_body
}
"#;
        let tree = parse::parse(FirstClass::Rust, src).unwrap();
        let analysis = analyze(FirstClass::Rust, src, &tree);

        let function = analysis
            .outlines
            .iter()
            .find(|symbol| symbol.name == "execute")
            .unwrap();
        assert!(function.exported);
        assert!(function.signature.contains("pub fn execute"));
        assert!(!function.signature.contains("secret_body"));
        assert!(
            analysis
                .outlines
                .iter()
                .any(|symbol| symbol.name == "Request" && symbol.kind == "type")
        );
    }

    #[test]
    fn first_class_outlines_mark_public_declarations() {
        for (fc, src, expected, body_marker) in [
            (
                FirstClass::Python,
                "@trace\ndef public(value: int) -> str:\n    # secret body comment\n    return str(value)\n",
                "public",
                "secret body comment",
            ),
            (
                FirstClass::JavaScript,
                "export function publicValue(value) { const secretBody = value; return secretBody }\n",
                "publicValue",
                "secretBody",
            ),
            (
                FirstClass::TypeScript,
                "export class PublicValue { run(): number { const secretBody = 1; return secretBody } }\n",
                "PublicValue",
                "secretBody",
            ),
            (
                FirstClass::Tsx,
                "export const PublicView = () => <div>secret-body</div>;\n",
                "PublicView",
                "secret-body",
            ),
            (
                FirstClass::Go,
                "package sample\nfunc PublicValue(value int) int { secretBody := value; return secretBody }\n",
                "PublicValue",
                "secretBody",
            ),
            (
                FirstClass::Php,
                "<?php\nfinal class PublicValue { public function run(): int { $secretBody = 1; return $secretBody; } }\n",
                "PublicValue",
                "secretBody",
            ),
        ] {
            let tree = parse::parse(fc, src).unwrap();
            let analysis = analyze(fc, src, &tree);
            let outline = analysis
                .outlines
                .iter()
                .find(|symbol| symbol.name == expected)
                .unwrap_or_else(|| panic!("missing {expected} outline for {fc:?}"));
            assert!(outline.exported, "{expected} was not public for {fc:?}");
            assert!(!outline.signature.contains(body_marker));
            if fc == FirstClass::Python {
                assert!(outline.signature.contains("@trace"));
            }
        }
    }

    #[test]
    fn named_javascript_exports_are_projected() {
        let src = "function helper(value) { return value }\nexport { helper };\n";
        let tree = parse::parse(FirstClass::JavaScript, src).unwrap();
        let analysis = analyze(FirstClass::JavaScript, src, &tree);

        assert!(
            analysis
                .outlines
                .iter()
                .any(|symbol| symbol.name == "helper" && symbol.exported)
        );
    }

    #[test]
    fn exported_types_expose_public_but_not_private_members() {
        let src = concat!(
            "class Service {\n",
            "  run(): void { function nested(): void {} }\n",
            "  private hidden(): void {}\n",
            "}\n",
            "export { Service };\n",
        );
        let tree = parse::parse(FirstClass::TypeScript, src).unwrap();
        let analysis = analyze(FirstClass::TypeScript, src, &tree);

        assert!(
            analysis
                .outlines
                .iter()
                .any(|symbol| symbol.name == "Service.run" && symbol.exported)
        );
        assert!(
            analysis
                .outlines
                .iter()
                .any(|symbol| symbol.name == "Service.hidden" && !symbol.exported)
        );
        assert!(analysis.outlines.iter().any(|symbol| {
            symbol.name == "nested" && symbol.kind == "function" && !symbol.exported
        }));
    }

    #[test]
    fn python_nested_declarations_and_private_class_members_are_not_public() {
        let src = concat!(
            "def public():\n",
            "    def nested():\n",
            "        return 1\n",
            "    return nested()\n",
            "\n",
            "class _Private:\n",
            "    def visible_name(self):\n",
            "        return 1\n",
        );
        let tree = parse::parse(FirstClass::Python, src).unwrap();
        let analysis = analyze(FirstClass::Python, src, &tree);

        assert!(
            analysis
                .outlines
                .iter()
                .any(|symbol| symbol.name == "public" && symbol.exported)
        );
        assert!(
            analysis
                .outlines
                .iter()
                .any(|symbol| symbol.name == "nested" && !symbol.exported)
        );
        assert!(
            analysis
                .outlines
                .iter()
                .any(|symbol| symbol.name == "_Private.visible_name" && !symbol.exported)
        );
    }
}
