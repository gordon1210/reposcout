//! Symbol / public-API counts derived from the tree-sitter AST.
//!
//! Counts three categories for each first-class language:
//! - `functions`: named callable definitions
//! - `types`:     type/class/interface/enum/struct declarations
//! - `exports`:   publicly accessible symbols (language-specific heuristic)

use crate::lang::FirstClass;
use crate::model::{
    DefinitionFact, DefinitionFacts, DefinitionStatus, SourceSpan, SymbolCounts, SymbolOutline,
};
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
    Signal => "signal",
    Constant => "constant",
    Property => "property",
    Node => "node",
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SymbolAnalysis {
    pub(crate) counts: SymbolCounts,
    pub(crate) outlines: Vec<SymbolOutline>,
    pub(crate) definitions: DefinitionFacts,
}

/// Analyze structural declarations once, retaining the historical aggregate
/// counts plus compact declaration headers for context-plan projection.
pub(crate) fn analyze(fc: FirstClass, content: &str, tree: &Tree) -> SymbolAnalysis {
    let (outlines, definitions) = declarations(fc, content, tree);
    SymbolAnalysis {
        counts: count(fc, content, tree),
        outlines,
        definitions,
    }
}

/// Count structural symbols in `tree` for `fc`.  `content` is the source text
/// used to resolve identifier names for export detection.
#[must_use]
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
        FirstClass::GdScript | FirstClass::GdShader => {
            let mut counts = SymbolCounts::default();
            walk(root, |node| {
                if let Some((kind, _)) = declaration_kind(fc, node) {
                    counts.functions +=
                        usize::from(matches!(kind, OutlineKind::Function | OutlineKind::Method));
                    counts.types +=
                        usize::from(matches!(kind, OutlineKind::Class | OutlineKind::Enum));
                    counts.exports += usize::from(
                        declaration_name(node, src).is_some_and(|name| !name.starts_with('_')),
                    );
                }
            });
            counts
        }
        FirstClass::GodotResource => SymbolCounts::default(),
    }
}

fn declarations(
    fc: FirstClass,
    content: &str,
    tree: &Tree,
) -> (Vec<SymbolOutline>, DefinitionFacts) {
    if fc == FirstClass::GodotResource {
        return (
            godot_nodes(content, tree),
            DefinitionFacts {
                status: DefinitionStatus::Unsupported,
                definitions: Vec::new(),
            },
        );
    }
    let src = content.as_bytes();
    let named_exports = named_module_exports(fc, tree.root_node(), src);
    let mut outlines = Vec::new();
    let mut seen = HashSet::new();
    let mut definition_seen = HashSet::new();
    let mut definitions = Vec::new();
    walk(tree.root_node(), |node| {
        let Some((kind, declaration)) = declaration_kind(fc, node) else {
            return;
        };
        let Some(base_name) = declaration_name(node, src) else {
            return;
        };
        let name = qualified_name(fc, node, &base_name, src);
        let line = node.start_position().row + 1;
        let include_outline = seen.insert((line, name.clone(), kind));
        let exported = declaration_exported(fc, node, &base_name, src, &named_exports);
        let reason = if exported {
            if matches!(fc, FirstClass::Python | FirstClass::GdScript) {
                "public-name heuristic"
            } else {
                "exported/public declaration"
            }
        } else {
            "representative file-local declaration"
        };
        let symbol = SymbolOutline {
            name,
            kind: kind.as_str().to_string(),
            signature: signature_text(fc, declaration, content),
            line,
            exported,
            reasons: vec![reason.to_string()],
        };
        if definition_seen.insert((
            node.start_byte(),
            node.end_byte(),
            symbol.name.clone(),
            kind,
        )) {
            let (declaration_span, source_span) = definition_spans(fc, node, declaration, content);
            definitions.push(DefinitionFact {
                symbol: symbol.clone(),
                declaration_span,
                source_span,
            });
        }
        if include_outline {
            outlines.push(symbol);
        }
    });
    outlines.sort_by(|left, right| {
        left.line
            .cmp(&right.line)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.name.cmp(&right.name))
    });
    definitions.sort_by(|left, right| {
        left.declaration_span
            .start_byte
            .cmp(&right.declaration_span.start_byte)
            .then_with(|| left.symbol.kind.cmp(&right.symbol.kind))
            .then_with(|| left.symbol.name.cmp(&right.symbol.name))
    });
    (
        outlines,
        DefinitionFacts {
            status: if tree.root_node().has_error() {
                DefinitionStatus::ParseErrors
            } else {
                DefinitionStatus::Available
            },
            definitions,
        },
    )
}

/// Return the canonical declaration kinds supported for precise retrieval in this language.
#[must_use]
pub fn definition_kinds(fc: FirstClass) -> &'static [&'static str] {
    match fc {
        FirstClass::Rust => &["enum", "function", "method", "trait", "type"],
        FirstClass::Python | FirstClass::JavaScript => &["class", "function", "method"],
        FirstClass::TypeScript | FirstClass::Tsx => {
            &["class", "enum", "function", "interface", "method", "type"]
        }
        FirstClass::Go => &["function", "method", "type"],
        FirstClass::Php => &["class", "enum", "function", "interface", "method", "trait"],
        FirstClass::GdScript => &["class", "constant", "enum", "method", "property", "signal"],
        FirstClass::GdShader => &["function"],
        FirstClass::GodotResource => &[],
    }
}

fn node_span(node: Node<'_>) -> SourceSpan {
    SourceSpan {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: node.start_position().row + 1,
        end_line: (node.end_position().row + usize::from(node.end_position().column != 0))
            .max(node.start_position().row + 1),
    }
}

fn definition_spans(
    fc: FirstClass,
    node: Node<'_>,
    declaration: Node<'_>,
    content: &str,
) -> (SourceSpan, Option<SourceSpan>) {
    let mut declaration_span = node_span(declaration);
    let mut source = declaration;
    if matches!(
        fc,
        FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx
    ) {
        if node.kind() == "variable_declarator" {
            let Some(parent) = node.parent().filter(|parent| {
                matches!(
                    parent.kind(),
                    "lexical_declaration" | "variable_declaration" | "using_declaration"
                )
            }) else {
                return (declaration_span, None);
            };
            source = parent;
        }
        if let Some(export) = source
            .parent()
            .filter(|parent| parent.kind() == "export_statement")
        {
            source = export;
        }
    } else if fc == FirstClass::Go && matches!(node.kind(), "type_spec" | "type_alias") {
        let Some(parent) = node
            .parent()
            .filter(|parent| parent.kind() == "type_declaration")
        else {
            return (declaration_span, None);
        };
        source = parent;
    }
    let mut span = node_span(source);
    let mut valid = !source.has_error() && !source.is_missing();
    if fc == FirstClass::Rust {
        let mut previous = source.prev_named_sibling();
        let mut adjacent_start = source.start_byte();
        while let Some(prefix) = previous {
            if !content
                .get(prefix.end_byte()..adjacent_start)
                .is_some_and(|gap| gap.chars().all(char::is_whitespace))
            {
                break;
            }
            let raw = content.get(prefix.byte_range()).unwrap_or_default();
            let is_outer_doc = (raw.starts_with("///") && !raw.starts_with("////"))
                || (raw.starts_with("/**") && !raw.starts_with("/***"));
            if prefix.kind() == "attribute_item" || is_outer_doc {
                span.start_byte = prefix.start_byte();
                span.start_line = prefix.start_position().row + 1;
                valid &= !prefix.has_error() && !prefix.is_missing();
            } else if !matches!(prefix.kind(), "line_comment" | "block_comment")
                || raw.starts_with("//!")
                || raw.starts_with("/*!")
            {
                break;
            }
            adjacent_start = prefix.start_byte();
            previous = prefix.prev_named_sibling();
        }
        declaration_span.start_byte = span.start_byte;
        declaration_span.start_line = span.start_line;
    } else if fc == FirstClass::GdScript {
        let mut previous = source.prev_named_sibling();
        let mut adjacent_start = source.start_byte();
        while let Some(prefix) = previous {
            if !content
                .get(prefix.end_byte()..adjacent_start)
                .is_some_and(|gap| gap.chars().all(char::is_whitespace))
            {
                break;
            }
            if matches!(prefix.kind(), "annotation" | "annotations") {
                span.start_byte = prefix.start_byte();
                span.start_line = prefix.start_position().row + 1;
                valid &= !prefix.has_error() && !prefix.is_missing();
            } else if prefix.kind() != "comment" {
                break;
            }
            adjacent_start = prefix.start_byte();
            previous = prefix.prev_named_sibling();
        }
        declaration_span.start_byte = span.start_byte;
        declaration_span.start_line = span.start_line;
    } else if source.id() != declaration.id()
        && node.kind() != "variable_declarator"
        && !matches!(node.kind(), "type_spec" | "type_alias")
    {
        declaration_span = span;
    }
    valid &=
        span.start_byte < span.end_byte && content.get(span.start_byte..span.end_byte).is_some();
    (declaration_span, valid.then_some(span))
}

fn declaration_kind(fc: FirstClass, node: Node<'_>) -> Option<(OutlineKind, Node<'_>)> {
    let kind = match fc {
        FirstClass::Rust => rust_declaration_kind(node),
        FirstClass::Python => python_declaration_kind(node),
        FirstClass::JavaScript => javascript_declaration_kind(node),
        FirstClass::TypeScript | FirstClass::Tsx => typescript_declaration_kind(node),
        FirstClass::Go => go_declaration_kind(node),
        FirstClass::Php => php_declaration_kind(node),
        FirstClass::GdScript => match node.kind() {
            "function_definition" | "constructor_definition" => Some(OutlineKind::Method),
            "class_definition" | "class_name_statement" => Some(OutlineKind::Class),
            "enum_definition" => Some(OutlineKind::Enum),
            "signal_statement" => Some(OutlineKind::Signal),
            "const_statement" if !ancestor(node, |parent| is_callable_scope(parent.kind())) => {
                Some(OutlineKind::Constant)
            }
            "variable_statement" if !ancestor(node, |parent| is_callable_scope(parent.kind())) => {
                Some(OutlineKind::Property)
            }
            _ => None,
        },
        FirstClass::GdShader => {
            (node.kind() == "function_definition").then_some(OutlineKind::Function)
        }
        FirstClass::GodotResource => None,
    }?;
    let declaration = if fc == FirstClass::Python {
        node.parent()
            .filter(|parent| parent.kind() == "decorated_definition")
            .unwrap_or(node)
    } else {
        node
    };
    Some((kind, declaration))
}

fn rust_declaration_kind(node: Node<'_>) -> Option<OutlineKind> {
    match node.kind() {
        "function_item" | "function_signature_item" => Some(callable_kind(node)),
        "struct_item" | "union_item" | "type_item" => Some(OutlineKind::Type),
        "enum_item" => Some(OutlineKind::Enum),
        "trait_item" => Some(OutlineKind::Trait),
        _ => None,
    }
}

fn python_declaration_kind(node: Node<'_>) -> Option<OutlineKind> {
    match node.kind() {
        "function_definition" => Some(callable_kind(node)),
        "class_definition" => Some(OutlineKind::Class),
        _ => None,
    }
}

fn javascript_declaration_kind(node: Node<'_>) -> Option<OutlineKind> {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" => Some(OutlineKind::Function),
        "method_definition" => Some(OutlineKind::Method),
        "class_declaration" => Some(OutlineKind::Class),
        "variable_declarator" if variable_callable(node) => Some(OutlineKind::Function),
        _ => None,
    }
}

fn typescript_declaration_kind(node: Node<'_>) -> Option<OutlineKind> {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" => Some(OutlineKind::Function),
        "method_definition" | "method_signature" => Some(OutlineKind::Method),
        "class_declaration" => Some(OutlineKind::Class),
        "interface_declaration" => Some(OutlineKind::Interface),
        "type_alias_declaration" => Some(OutlineKind::Type),
        "enum_declaration" => Some(OutlineKind::Enum),
        "variable_declarator" if variable_callable(node) => Some(OutlineKind::Function),
        _ => None,
    }
}

fn go_declaration_kind(node: Node<'_>) -> Option<OutlineKind> {
    match node.kind() {
        "function_declaration" => Some(OutlineKind::Function),
        "method_declaration" => Some(OutlineKind::Method),
        "type_spec" | "type_alias" => Some(OutlineKind::Type),
        _ => None,
    }
}

fn php_declaration_kind(node: Node<'_>) -> Option<OutlineKind> {
    match node.kind() {
        "function_definition" => Some(OutlineKind::Function),
        "method_declaration" => Some(OutlineKind::Method),
        "class_declaration" => Some(OutlineKind::Class),
        "interface_declaration" => Some(OutlineKind::Interface),
        "trait_declaration" => Some(OutlineKind::Trait),
        "enum_declaration" => Some(OutlineKind::Enum),
        _ => None,
    }
}

fn godot_nodes(content: &str, tree: &Tree) -> Vec<SymbolOutline> {
    let mut outlines = Vec::new();
    let root = tree.root_node();
    let mut cursor = root.walk();
    for section in root.named_children(&mut cursor) {
        if section.kind() != "section" || crate::godot::section_name(section, content) != "node" {
            continue;
        }
        let Some(name) = crate::godot::attribute(section, "name", content) else {
            continue;
        };
        let parent = crate::godot::attribute(section, "parent", content);
        let name = parent
            .filter(|parent| parent != ".")
            .map_or_else(|| name.clone(), |parent| format!("{parent}/{name}"));
        let mut cursor = section.walk();
        let end = section
            .children(&mut cursor)
            .find(|child| child.kind() == "]")
            .map_or(section.start_byte(), |close| close.end_byte());
        let signature = content[section.start_byte()..end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        outlines.push(SymbolOutline {
            name,
            kind: OutlineKind::Node.as_str().to_string(),
            signature: signature.chars().take(MAX_SIGNATURE_CHARS).collect(),
            line: section.start_position().row + 1,
            exported: false,
            reasons: vec!["scene node declaration; not a filesystem path".to_string()],
        });
    }
    outlines
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
    if node.kind() == "constructor_definition" {
        return Some("_init".to_string());
    }
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
            FirstClass::Python | FirstClass::GdScript => candidate.kind() == "class_definition",
            FirstClass::GdShader | FirstClass::GodotResource | FirstClass::Go => false,
            FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx => matches!(
                candidate.kind(),
                "class_declaration" | "interface_declaration"
            ),
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
            | "constructor_definition"
            | "get_body"
            | "set_body"
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
        FirstClass::GdScript => {
            !name.starts_with('_') && !ancestor(node, |parent| is_callable_scope(parent.kind()))
        }
        FirstClass::GdShader => true,
        FirstClass::GodotResource => false,
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
            | "internal_module"
            | "program" => return false,
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
    if let Some(body) = header
        .child_by_field_name("body")
        .or_else(|| header.child_by_field_name("block"))
        .or_else(|| header.child_by_field_name("setget"))
        .or_else(|| {
            header
                .child_by_field_name("value")
                .and_then(|value| value.child_by_field_name("body"))
        })
    {
        end = if fc == FirstClass::Python {
            let mut cursor = header.walk();
            header
                .children(&mut cursor)
                .filter(|child| child.kind() == ":")
                .last()
                .map_or_else(|| body.start_byte(), |colon| colon.end_byte())
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
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
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
            if name_text(node, src).is_some_and(|n| !n.starts_with('_')) {
                counts.exports += 1;
            }
        }
        "class_definition" => {
            counts.types += 1;
            if name_text(node, src).is_some_and(|n| !n.starts_with('_')) {
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
                .is_some_and(char::is_uppercase)
            {
                counts.exports += 1;
            }
        }
        "type_spec" | "type_alias" => {
            counts.types += 1;
            if name_text(node, src)
                .and_then(|n| n.chars().next())
                .is_some_and(char::is_uppercase)
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

    fn extracted(fc: FirstClass, source: &str) -> DefinitionFacts {
        analyze(fc, source, &parse::parse(fc, source).unwrap()).definitions
    }

    fn source_of<'a>(source: &'a str, definition: &DefinitionFact) -> &'a str {
        let span = definition.source_span.unwrap();
        &source[span.start_byte..span.end_byte]
    }

    type ExpectedDefinition = (&'static str, &'static str, &'static str);
    type DefinitionFixture = (FirstClass, &'static str, &'static [ExpectedDefinition]);
    const DEFINITION_FIXTURES: &[DefinitionFixture] = &[
        (
            FirstClass::Rust,
            "struct Item;\nenum Mode { Ready }\ntrait Runner { fn run(&self); }\nfn execute() {}\n",
            &[
                ("Item", "type", "struct Item;"),
                ("Mode", "enum", "enum Mode { Ready }"),
                ("Runner", "trait", "trait Runner { fn run(&self); }"),
                ("Runner.run", "method", "fn run(&self);"),
                ("execute", "function", "fn execute() {}"),
            ],
        ),
        (
            FirstClass::Python,
            "class Item:\n    def run(self):\n        return 1\n\ndef execute():\n    return 2\n",
            &[
                (
                    "Item",
                    "class",
                    "class Item:\n    def run(self):\n        return 1",
                ),
                ("Item.run", "method", "def run(self):\n        return 1"),
                ("execute", "function", "def execute():\n    return 2"),
            ],
        ),
        (
            FirstClass::JavaScript,
            "class Item { run() { return 1; } }\nfunction execute() { return 2; }\n",
            &[
                ("Item", "class", "class Item { run() { return 1; } }"),
                ("Item.run", "method", "run() { return 1; }"),
                ("execute", "function", "function execute() { return 2; }"),
            ],
        ),
        (
            FirstClass::TypeScript,
            "class Item { run(): number { return 1; } }\ninterface Runner { run(): void; }\ntype Name = string;\nenum Mode { Ready }\nfunction execute(): void {}\n",
            &[
                (
                    "Item",
                    "class",
                    "class Item { run(): number { return 1; } }",
                ),
                ("Item.run", "method", "run(): number { return 1; }"),
                ("Runner", "interface", "interface Runner { run(): void; }"),
                ("Runner.run", "method", "run(): void"),
                ("Name", "type", "type Name = string;"),
                ("Mode", "enum", "enum Mode { Ready }"),
                ("execute", "function", "function execute(): void {}"),
            ],
        ),
        (
            FirstClass::Tsx,
            "class Item { run(): number { return 1; } }\ninterface Runner { run(): void; }\ntype Name = string;\nenum Mode { Ready }\nconst View = () => <div />;\n",
            &[
                (
                    "Item",
                    "class",
                    "class Item { run(): number { return 1; } }",
                ),
                ("Item.run", "method", "run(): number { return 1; }"),
                ("Runner", "interface", "interface Runner { run(): void; }"),
                ("Runner.run", "method", "run(): void"),
                ("Name", "type", "type Name = string;"),
                ("Mode", "enum", "enum Mode { Ready }"),
                ("View", "function", "const View = () => <div />;"),
            ],
        ),
        (
            FirstClass::Go,
            "package sample\ntype Item struct{}\nfunc (item Item) Run() {}\nfunc Execute() {}\n",
            &[
                ("Item", "type", "type Item struct{}"),
                ("Run", "method", "func (item Item) Run() {}"),
                ("Execute", "function", "func Execute() {}"),
            ],
        ),
        (
            FirstClass::Php,
            "<?php\nclass Item { public function run(): void {} }\ninterface Runner {}\ntrait Shared {}\nenum Mode { case Ready; }\nfunction execute(): void {}\n",
            &[
                (
                    "Item",
                    "class",
                    "class Item { public function run(): void {} }",
                ),
                ("Item.run", "method", "public function run(): void {}"),
                ("Runner", "interface", "interface Runner {}"),
                ("Shared", "trait", "trait Shared {}"),
                ("Mode", "enum", "enum Mode { case Ready; }"),
                ("execute", "function", "function execute(): void {}"),
            ],
        ),
        (
            FirstClass::GdScript,
            "class_name Item\nsignal moved\nconst SPEED = 1\nvar position = 0\nenum Mode { READY }\nfunc run():\n    return 1\n",
            &[
                ("Item", "class", "class_name Item"),
                ("moved", "signal", "signal moved"),
                ("SPEED", "constant", "const SPEED = 1"),
                ("position", "property", "var position = 0"),
                ("Mode", "enum", "enum Mode { READY }"),
                ("run", "method", "func run():\n    return 1"),
            ],
        ),
        (
            FirstClass::GdShader,
            "shader_type spatial;\nvoid fragment() { ALBEDO = vec3(1.0); }\n",
            &[(
                "fragment",
                "function",
                "void fragment() { ALBEDO = vec3(1.0); }",
            )],
        ),
    ];

    #[test]
    fn every_advertised_language_kind_has_an_exact_source_span() {
        for &(language, source, expected) in DEFINITION_FIXTURES {
            let facts = extracted(language, source);
            assert_eq!(facts.status, DefinitionStatus::Available, "{language:?}");
            assert_eq!(facts.definitions.len(), expected.len(), "{language:?}");
            let observed_kinds = facts
                .definitions
                .iter()
                .map(|definition| definition.symbol.kind.as_str())
                .collect::<BTreeSet<_>>();
            let advertised_kinds = definition_kinds(language)
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            assert_eq!(observed_kinds, advertised_kinds, "{language:?}");
            for &(name, kind, exact_source) in expected {
                let definition = facts
                    .definitions
                    .iter()
                    .find(|definition| {
                        definition.symbol.name == name && definition.symbol.kind == kind
                    })
                    .unwrap_or_else(|| panic!("missing {language:?} {kind} {name}"));
                assert_eq!(
                    source_of(source, definition),
                    exact_source,
                    "{language:?} {kind} {name}"
                );
                let span = definition.source_span.unwrap();
                let preceding = &source[..span.start_byte];
                let start_line = preceding.bytes().filter(|byte| *byte == b'\n').count() + 1;
                let end_line = start_line
                    + exact_source
                        .trim_end_matches('\n')
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .count();
                assert_eq!(
                    (span.start_line, span.end_line),
                    (start_line, end_line),
                    "{language:?} {kind} {name}"
                );
            }
        }
    }

    #[test]
    fn definition_spans_preserve_complete_source_across_code_languages() {
        for (language, source, name, expected) in [
            (
                FirstClass::Rust,
                "#[inline]\n// attached\n#[must_use]\npub fn run() -> u32 { 42 }\n",
                "run",
                "#[inline]\n// attached\n#[must_use]\npub fn run() -> u32 { 42 }",
            ),
            (
                FirstClass::Python,
                "@first\n@second(1)\ndef run():\n    return 42\n",
                "run",
                "@first\n@second(1)\ndef run():\n    return 42",
            ),
            (
                FirstClass::JavaScript,
                "export function run() { return 42; }\n",
                "run",
                "export function run() { return 42; }",
            ),
            (
                FirstClass::TypeScript,
                "export const run = (): number => 42;\n",
                "run",
                "export const run = (): number => 42;",
            ),
            (
                FirstClass::Tsx,
                "export const View = () => <div>hello</div>;\n",
                "View",
                "export const View = () => <div>hello</div>;",
            ),
            (
                FirstClass::Go,
                "package sample\nfunc Run() int { return 42 }\n",
                "Run",
                "func Run() int { return 42 }",
            ),
            (
                FirstClass::Php,
                "<?php\n#[Example]\nfunction run(): int { return 42; }\n",
                "run",
                "#[Example]\nfunction run(): int { return 42; }",
            ),
            (
                FirstClass::GdScript,
                "@rpc\nfunc run():\n    return 42\n",
                "run",
                "@rpc\nfunc run():\n    return 42",
            ),
            (
                FirstClass::GdShader,
                "shader_type spatial;\nvoid fragment() { ALBEDO = vec3(1.0); }\n",
                "fragment",
                "void fragment() { ALBEDO = vec3(1.0); }",
            ),
        ] {
            let facts = extracted(language, source);
            assert_eq!(facts.status, DefinitionStatus::Available, "{language:?}");
            let definition = facts
                .definitions
                .iter()
                .find(|definition| definition.symbol.name == name)
                .unwrap();
            assert_eq!(source_of(source, definition), expected, "{language:?}");
            assert!(definition_kinds(language).contains(&definition.symbol.kind.as_str()));
        }
    }

    #[test]
    fn grouped_declarations_share_source_but_retain_identity_spans() {
        for (language, source) in [
            (
                FirstClass::JavaScript,
                "export const one = () => 1, two = () => 2;",
            ),
            (
                FirstClass::Go,
                "package sample\ntype (\nOne int\nTwo string\n)\n",
            ),
        ] {
            let facts = extracted(language, source);
            assert_eq!(facts.status, DefinitionStatus::Available, "{language:?}");
            assert_eq!(facts.definitions.len(), 2);
            assert_eq!(
                facts.definitions[0].source_span,
                facts.definitions[1].source_span
            );
            assert_ne!(
                facts.definitions[0].declaration_span,
                facts.definitions[1].declaration_span
            );
        }
    }

    #[test]
    fn bodyless_declarations_retain_terminators_and_no_neighbour_body() {
        for (language, source, name, expected) in [
            (
                FirstClass::Rust,
                "trait Run { fn run(&self); fn other(&self) {} }",
                "Run.run",
                "fn run(&self);",
            ),
            (
                FirstClass::TypeScript,
                "interface Run { run(): void; other(): void; }",
                "Run.run",
                "run(): void",
            ),
            (
                FirstClass::Php,
                "<?php interface Run { public function run(): void; }",
                "Run.run",
                "public function run(): void;",
            ),
            (
                FirstClass::GdScript,
                "@abstract\nfunc run()\n",
                "run",
                "@abstract\nfunc run()",
            ),
        ] {
            let facts = extracted(language, source);
            assert_eq!(facts.status, DefinitionStatus::Available, "{language:?}");
            let definition = facts
                .definitions
                .iter()
                .find(|definition| definition.symbol.name == name)
                .unwrap();
            assert_eq!(source_of(source, definition), expected, "{language:?}");
        }
    }

    #[test]
    fn definitions_are_not_deduplicated_by_same_line_name() {
        let source = "fn one() { fn nested() {} } fn two() { fn nested() {} }";
        let analysis = analyze(
            FirstClass::Rust,
            source,
            &parse::parse(FirstClass::Rust, source).unwrap(),
        );
        assert_eq!(
            analysis
                .definitions
                .definitions
                .iter()
                .filter(|definition| definition.symbol.name == "nested")
                .count(),
            2
        );
        assert_eq!(
            analysis
                .outlines
                .iter()
                .filter(|outline| outline.name == "nested")
                .count(),
            1
        );
    }

    #[test]
    fn parse_errors_preserve_clean_siblings_without_trusting_broken_source() {
        let source = "fn good() {}\nfn broken() { let value = ; }";
        let facts = extracted(FirstClass::Rust, source);
        assert_eq!(facts.status, DefinitionStatus::ParseErrors);
        let good = facts
            .definitions
            .iter()
            .find(|definition| definition.symbol.name == "good")
            .unwrap();
        assert_eq!(source_of(source, good), "fn good() {}");
        let broken = facts
            .definitions
            .iter()
            .find(|definition| definition.symbol.name == "broken")
            .unwrap();
        assert!(broken.source_span.is_none());
    }

    #[test]
    fn byte_spans_preserve_unicode_crlf_and_final_line_without_newline() {
        let source = "// ü\r\nfn café() { let value = \"😀\"; }";
        let facts = extracted(FirstClass::Rust, source);
        assert_eq!(facts.status, DefinitionStatus::Available);
        let definition = &facts.definitions[0];
        let span = definition.source_span.unwrap();
        assert_eq!(
            source_of(source, definition),
            "fn café() { let value = \"😀\"; }"
        );
        assert_eq!((span.start_line, span.end_line), (2, 2));
        assert_eq!(span.end_byte, source.len());
    }

    #[test]
    fn resource_outlines_do_not_claim_code_definition_support() {
        let facts = extracted(
            FirstClass::GodotResource,
            "[gd_scene format=3]\n[node name=\"Root\" type=\"Node\"]\n",
        );
        assert_eq!(facts.status, DefinitionStatus::Unsupported);
        assert!(facts.definitions.is_empty());
        assert!(definition_kinds(FirstClass::GodotResource).is_empty());
    }

    #[test]
    fn source_spans_are_independent_of_signature_caps() {
        let source = format!("fn run() {{ let body = \"{}\"; }}", "x".repeat(600));
        let facts = extracted(FirstClass::Rust, &source);
        let definition = &facts.definitions[0];
        assert_eq!(source_of(&source, definition), source);
        assert!(definition.symbol.signature.len() < 280);
    }

    #[test]
    fn outline_kinds_are_a_closed_capability_set() {
        let samples = [
            (
                FirstClass::GdScript,
                "class_name Actor\nextends Node\nsignal moved\nconst SPEED = 1\nvar position = 0\n",
            ),
            (
                FirstClass::GodotResource,
                "[gd_scene format=3]\n[node name=\"Main\" type=\"Node\"]\n",
            ),
            (
                FirstClass::Rust,
                r"
pub fn run() {}
pub struct Service;
pub enum State { Ready }
pub trait Runner { fn execute(&self); }
impl Runner for Service { fn execute(&self) {} }
",
            ),
            (
                FirstClass::TypeScript,
                r"
export class Client { request(): void {} }
export interface Transport { send(): void; }
export type Identifier = string;
export enum Mode { Fast }
export function connect(): void {}
",
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
        let src = r"
pub fn public_func() {}
fn private_func() {}
pub struct MyStruct {}
";
        let tree = parse::parse(FirstClass::Rust, src).unwrap();
        let counts = count(FirstClass::Rust, src, &tree);
        assert_eq!(counts.functions, 2, "two functions");
        assert_eq!(counts.types, 1, "one struct");
        assert_eq!(counts.exports, 2, "pub fn + pub struct");
    }

    #[test]
    fn typescript_interface_and_export() {
        let src = r"
export interface Foo {
    bar: string;
}
function baz() {}
";
        let tree = parse::parse(FirstClass::TypeScript, src).unwrap();
        let counts = count(FirstClass::TypeScript, src, &tree);
        assert!(counts.types >= 1, "interface should count as a type");
        assert!(counts.exports >= 1, "export_statement should count");
    }

    #[test]
    fn go_grouped_types_are_counted_individually() {
        let src = r"
package sample
type (
    Exported struct{}
    internal int
    Alias = string
)
";
        let tree = parse::parse(FirstClass::Go, src).unwrap();
        let counts = count(FirstClass::Go, src, &tree);

        assert_eq!(counts.types, 3);
        assert_eq!(counts.exports, 2);
    }

    #[test]
    fn php_counts_named_symbols_and_public_api() {
        let src = r"<?php
function helper(): void {}

interface Runner { public function run(): void; }
trait Logs { protected function log(): void {} }
enum Status { case Ready; }
final class Service implements Runner {
    public function run(): void {}
    private function secret(): void {}
}
";
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
        let src = r"
pub struct Request {
    pub value: String,
}

pub fn execute(request: Request) -> usize {
    let secret_body = request.value.len();
    secret_body
}
";
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
