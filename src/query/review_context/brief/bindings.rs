//! Where a file binds the names it uses: declared in the file, imported from an in-tree module,
//! or bound outside the tree (external imports, ambient, foreign and exported ABI declarations).

use crate::lang::FirstClass;
use rustc_hash::FxHashSet;
use tree_sitter::{Node, Tree};

/// The name is declared in a non-import binding position or as the local alias of an import.
pub(super) const DECLARED: u8 = 1;
/// The name is imported by its original name from an in-tree module.
pub(super) const IMPORTED: u8 = 2;

/// Names a file binds, by how.
#[derive(Debug, Default)]
pub(super) struct Bindings {
    declared: FxHashSet<String>,
    imported: FxHashSet<String>,
    /// Bound outside the tree in this file; such a name proves no rename.
    external: FxHashSet<String>,
}

impl Bindings {
    /// `DECLARED` and `IMPORTED` flags of `name`; none for a name also bound outside the tree.
    pub(super) fn of(&self, name: &str) -> u8 {
        if self.external.contains(name) {
            return 0;
        }
        let flag = |set: &FxHashSet<String>, flag: u8| if set.contains(name) { flag } else { 0 };
        flag(&self.declared, DECLARED) | flag(&self.imported, IMPORTED)
    }
}

/// The bindings of a parsed file of a reviewed grammar, or `None` when some identifier of the file
/// contains a non-ASCII character. `module` tells whether an absolute Python module path names a
/// captured file.
pub(super) fn bindings(
    grammar: FirstClass,
    tree: &Tree,
    content: &str,
    module: &dyn Fn(&str) -> bool,
) -> Option<Bindings> {
    let text = |node: Node<'_>| content.get(node.byte_range()).unwrap_or_default();
    let mut out = Bindings::default();
    // Node, parent, field within the parent, and whether declarations there bind outside the tree.
    let mut stack = vec![(tree.root_node(), None, None, false)];
    while let Some((node, parent, field, outside)) = stack.pop() {
        if node.kind().contains("identifier") && !text(node).is_ascii() {
            return None;
        }
        let outside = outside || external_scope(grammar, node);
        if let Some(parent) = parent
            && declares(grammar, parent, field, node)
        {
            let exported = grammar == FirstClass::Rust && abi_symbol(parent, content);
            let mut names = Vec::new();
            pattern(grammar, node, &mut names);
            let target = if outside || exported {
                &mut out.external
            } else {
                &mut out.declared
            };
            target.extend(names.into_iter().map(|name| text(name).to_string()));
        }
        imports(grammar, node, content, module, &mut out);
        let mut cursor = node.walk();
        let children: Vec<_> = node
            .children(&mut cursor)
            .enumerate()
            .filter(|(_, child)| child.is_named())
            .map(|(index, child)| {
                let field = u32::try_from(index)
                    .ok()
                    .and_then(|index| node.field_name_for_child(index));
                (child, Some(node), field, outside)
            })
            .collect();
        stack.extend(children.into_iter().rev());
    }
    Some(out)
}

/// Declarations inside `node` bind outside the tree: TypeScript ambient declarations and Rust
/// extern blocks.
fn external_scope(grammar: FirstClass, node: Node<'_>) -> bool {
    match grammar {
        FirstClass::TypeScript | FirstClass::Tsx | FirstClass::JavaScript => {
            node.kind() == "ambient_declaration"
        }
        FirstClass::Rust => node.kind() == "foreign_mod_item",
        _ => false,
    }
}

/// A Rust item carrying `#[no_mangle]` or `#[export_name]`.
fn abi_symbol(item: Node<'_>, content: &str) -> bool {
    let mut current = item.prev_named_sibling();
    while let Some(attribute) = current.filter(|node| node.kind() == "attribute_item") {
        let text = content.get(attribute.byte_range()).unwrap_or_default();
        if text.contains("no_mangle") || text.contains("export_name") {
            return true;
        }
        current = attribute.prev_named_sibling();
    }
    false
}

/// `node`, the `field` child of `parent`, holds names that `parent` declares.
fn declares(grammar: FirstClass, parent: Node<'_>, field: Option<&str>, node: Node<'_>) -> bool {
    let parent = parent.kind();
    match grammar {
        FirstClass::TypeScript | FirstClass::Tsx | FirstClass::JavaScript => match field {
            Some("name") => matches!(
                parent,
                "variable_declarator"
                    | "function_declaration"
                    | "generator_function_declaration"
                    | "function_expression"
                    | "generator_function"
                    | "function_signature"
                    | "class_declaration"
                    | "class"
                    | "abstract_class_declaration"
                    | "interface_declaration"
                    | "type_alias_declaration"
                    | "enum_declaration"
                    | "internal_module"
                    | "type_parameter"
            ),
            Some("parameter") => matches!(parent, "arrow_function" | "catch_clause"),
            Some("parameters") => true,
            Some("left") => parent == "for_in_statement",
            Some("alias") => parent == "import_specifier",
            None => {
                matches!(parent, "import_clause" | "namespace_import")
                    && node.kind() == "identifier"
            }
            _ => false,
        },
        FirstClass::Python => match field {
            Some("name") => matches!(
                parent,
                "function_definition" | "class_definition" | "named_expression"
            ),
            Some("parameters") => matches!(parent, "function_definition" | "lambda"),
            Some("left") => matches!(
                parent,
                "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause"
            ),
            Some("alias") => matches!(parent, "as_pattern" | "aliased_import"),
            _ => false,
        },
        FirstClass::Rust => match field {
            Some("name") => matches!(
                parent,
                "function_item"
                    | "function_signature_item"
                    | "struct_item"
                    | "enum_item"
                    | "union_item"
                    | "trait_item"
                    | "type_item"
                    | "const_item"
                    | "static_item"
                    | "mod_item"
                    | "macro_definition"
                    | "type_parameter"
            ),
            Some("pattern") => matches!(
                parent,
                "let_declaration" | "for_expression" | "match_arm" | "let_condition"
            ),
            Some("parameters") => matches!(
                parent,
                "function_item" | "function_signature_item" | "closure_expression"
            ),
            Some("left") => parent == "constrained_type_parameter",
            Some("alias") => parent == "use_as_clause",
            _ => false,
        },
        FirstClass::Go => matches!(
            (parent, field),
            (
                "function_declaration"
                    | "type_spec"
                    | "type_alias"
                    | "var_spec"
                    | "const_spec"
                    | "type_parameter_declaration",
                Some("name")
            ) | (
                "function_declaration" | "method_declaration" | "func_literal",
                Some("parameters" | "result")
            ) | ("method_declaration", Some("receiver"))
                | ("short_var_declaration" | "range_clause", Some("left"))
        ),
        _ => false,
    }
}

/// Identifier nodes a declaring position binds, through destructuring and parameter lists but
/// never into keys, types, default values or other expressions.
fn pattern<'t>(grammar: FirstClass, node: Node<'t>, out: &mut Vec<Node<'t>>) {
    let kind = node.kind();
    let named = |out: &mut Vec<Node<'t>>, skip: &[&str]| {
        let mut cursor = node.walk();
        for (index, child) in node.children(&mut cursor).enumerate() {
            let field = u32::try_from(index)
                .ok()
                .and_then(|index| node.field_name_for_child(index));
            if child.is_named() && !field.is_some_and(|field| skip.contains(&field)) {
                pattern(grammar, child, out);
            }
        }
    };
    let field = |out: &mut Vec<Node<'t>>, name: &str| {
        let mut cursor = node.walk();
        for child in node.children_by_field_name(name, &mut cursor) {
            pattern(grammar, child, out);
        }
    };
    match grammar {
        FirstClass::TypeScript | FirstClass::Tsx | FirstClass::JavaScript => match kind {
            "identifier" | "type_identifier" | "shorthand_property_identifier_pattern" => {
                out.push(node);
            }
            "object_pattern" | "array_pattern" | "rest_pattern" | "formal_parameters" => {
                named(out, &["key", "value", "right", "type"]);
            }
            "pair_pattern" => field(out, "value"),
            "object_assignment_pattern" | "assignment_pattern" => field(out, "left"),
            "required_parameter" | "optional_parameter" => field(out, "pattern"),
            _ => {}
        },
        FirstClass::Python => match kind {
            "identifier" => out.push(node),
            "pattern_list"
            | "tuple_pattern"
            | "list_pattern"
            | "list_splat_pattern"
            | "dictionary_splat_pattern"
            | "as_pattern_target"
            | "parameters"
            | "lambda_parameters"
            | "typed_parameter" => named(out, &["type", "value"]),
            "default_parameter" | "typed_default_parameter" => field(out, "name"),
            _ => {}
        },
        FirstClass::Rust => match kind {
            "identifier" | "type_identifier" | "shorthand_field_identifier" => out.push(node),
            "tuple_pattern"
            | "slice_pattern"
            | "ref_pattern"
            | "mut_pattern"
            | "reference_pattern"
            | "or_pattern"
            | "parameters"
            | "closure_parameters"
            | "captured_pattern"
            | "match_pattern"
            | "tuple_struct_pattern"
            | "struct_pattern" => named(out, &["type", "condition"]),
            "field_pattern" => {
                if node.child_by_field_name("pattern").is_some() {
                    field(out, "pattern");
                } else {
                    field(out, "name");
                }
            }
            "parameter" => field(out, "pattern"),
            _ => {}
        },
        FirstClass::Go => match kind {
            "identifier" | "type_identifier" => out.push(node),
            "expression_list" | "parameter_list" => named(out, &[]),
            "parameter_declaration" | "variadic_parameter_declaration" => field(out, "name"),
            _ => {}
        },
        _ => {}
    }
}

/// Records the names an import statement binds by their original names, as imported from an
/// in-tree module or bound outside the tree.
fn imports(
    grammar: FirstClass,
    node: Node<'_>,
    content: &str,
    module: &dyn Fn(&str) -> bool,
    out: &mut Bindings,
) {
    let text = |node: Node<'_>| content.get(node.byte_range()).unwrap_or_default();
    let mut names = Vec::new();
    let in_tree = match (grammar, node.kind()) {
        (
            FirstClass::TypeScript | FirstClass::Tsx | FirstClass::JavaScript,
            "import_specifier" | "export_specifier",
        ) => {
            let Some(source) = script_source(node, content) else {
                return;
            };
            names.extend(node.child_by_field_name("name"));
            source
        }
        (FirstClass::Python, "import_from_statement") => {
            let mut cursor = node.walk();
            for name in node.children_by_field_name("name", &mut cursor) {
                let original = if name.kind() == "aliased_import" {
                    name.child_by_field_name("name")
                } else {
                    Some(name)
                };
                names.extend(original);
            }
            python_from(node, content, module)
        }
        (FirstClass::Python, "import_statement") => {
            let mut cursor = node.walk();
            let mut in_tree = true;
            for name in node.children_by_field_name("name", &mut cursor) {
                if name.kind() == "dotted_name" {
                    in_tree &= module(text(name));
                    names.extend(name.named_child(0));
                }
            }
            in_tree
        }
        (FirstClass::Rust, "use_declaration") => {
            let Some(argument) = node.child_by_field_name("argument") else {
                return;
            };
            use_names(argument, &mut names);
            rust_in_tree(argument)
        }
        _ => return,
    };
    let target = if in_tree {
        &mut out.imported
    } else {
        &mut out.external
    };
    target.extend(names.into_iter().map(|name| text(name).to_string()));
}

/// Whether the specifier's statement imports or re-exports from a relative module; `None` for a
/// specifier without a source module.
fn script_source(specifier: Node<'_>, content: &str) -> Option<bool> {
    let mut current = specifier.parent();
    while let Some(node) = current {
        if matches!(node.kind(), "import_statement" | "export_statement") {
            let source = node.child_by_field_name("source")?;
            let text = content.get(source.byte_range()).unwrap_or_default();
            return Some(text.get(1..).is_some_and(|path| path.starts_with('.')));
        }
        current = node.parent();
    }
    None
}

/// A Python `from` import names a relative module or an absolute module of a captured file.
fn python_from(statement: Node<'_>, content: &str, module: &dyn Fn(&str) -> bool) -> bool {
    statement
        .child_by_field_name("module_name")
        .is_some_and(|name| {
            name.kind() == "relative_import"
                || module(content.get(name.byte_range()).unwrap_or_default())
        })
}

/// Original names a Rust use tree imports.
fn use_names<'t>(node: Node<'t>, out: &mut Vec<Node<'t>>) {
    match node.kind() {
        "identifier" => out.push(node),
        "scoped_identifier" => out.extend(node.child_by_field_name("name")),
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                use_names(path, out);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                use_names(child, out);
            }
        }
        "scoped_use_list" => {
            if let Some(list) = node.child_by_field_name("list") {
                use_names(list, out);
            }
        }
        _ => {}
    }
}

/// A Rust use tree rooted at `crate`, `self` or `super`.
fn rust_in_tree(argument: Node<'_>) -> bool {
    let mut current = argument;
    loop {
        match current.kind() {
            "crate" | "self" | "super" => return true,
            "scoped_identifier" | "scoped_use_list" | "use_as_clause" => {
                match current.child_by_field_name("path") {
                    Some(path) => current = path,
                    None => return false,
                }
            }
            "use_wildcard" => match current.named_child(0) {
                Some(path) => current = path,
                None => return false,
            },
            _ => return false,
        }
    }
}

/// For a token on the original-name side of an import or re-export specifier, whether that
/// specifier names an in-tree module; `None` for any other position.
pub(super) fn specifier(
    grammar: FirstClass,
    node: Node<'_>,
    content: &str,
    module: &dyn Fn(&str) -> bool,
) -> Option<bool> {
    let parent = node.parent()?;
    match grammar {
        FirstClass::TypeScript | FirstClass::Tsx | FirstClass::JavaScript => {
            let named = parent
                .child_by_field_name("name")
                .is_some_and(|name| name.id() == node.id());
            if matches!(parent.kind(), "import_specifier" | "export_specifier") && named {
                script_source(parent, content)
            } else {
                None
            }
        }
        FirstClass::Python => {
            let mut current = node;
            while let Some(outer) = current.parent() {
                match outer.kind() {
                    "dotted_name" => current = outer,
                    "aliased_import" => {
                        let original = outer
                            .child_by_field_name("name")
                            .is_some_and(|name| name.id() == current.id());
                        let statement = outer
                            .parent()
                            .filter(|statement| statement.kind() == "import_from_statement")?;
                        return original.then(|| python_from(statement, content, module));
                    }
                    "import_from_statement" => {
                        let mut cursor = outer.walk();
                        let named = outer
                            .children_by_field_name("name", &mut cursor)
                            .any(|name| name.id() == current.id());
                        return named.then(|| python_from(outer, content, module));
                    }
                    _ => return None,
                }
            }
            None
        }
        FirstClass::Rust => {
            if parent.kind() == "use_as_clause"
                && parent
                    .child_by_field_name("alias")
                    .is_some_and(|alias| alias.id() == node.id())
            {
                return None;
            }
            let mut current = parent;
            loop {
                if current.kind() == "use_declaration" {
                    return Some(
                        current
                            .child_by_field_name("argument")
                            .is_some_and(rust_in_tree),
                    );
                }
                current = current.parent()?;
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(grammar: FirstClass, source: &str, names: &[&str]) -> Vec<u8> {
        let tree = crate::parse::parse(grammar, source).unwrap();
        let module = |path: &str| path == "pkg.core";
        let bindings = bindings(grammar, &tree, source, &module).unwrap();
        names.iter().map(|name| bindings.of(name)).collect()
    }

    #[test]
    fn script_bindings_come_from_declarations_and_relative_imports() {
        let source = "import { user as u, other } from 'ext-lib';\n\
            import { loadUser, kept as alias } from './lib';\n\
            import def, * as ns from 'ext';\n\
            export { shared as current } from 'ext-lib';\n\
            export { local } from './local';\n\
            declare global { var global_user: string }\n\
            declare function ambient(): void;\n\
            const { a, b: c, d = 1, ...e } = obj;\n\
            function f(p, q = 1) { try {} catch (err) {} }\n\
            class K {}\n";
        for grammar in [FirstClass::TypeScript, FirstClass::Tsx] {
            assert_eq!(
                of(
                    grammar,
                    source,
                    &[
                        "user",
                        "u",
                        "other",
                        "loadUser",
                        "kept",
                        "alias",
                        "def",
                        "ns",
                        "shared",
                        "current",
                        "local",
                        "global_user",
                        "ambient",
                        "a",
                        "b",
                        "c",
                        "d",
                        "e",
                        "f",
                        "p",
                        "q",
                        "err",
                        "K",
                        "obj",
                    ]
                ),
                [
                    0, 1, 0, 2, 2, 1, 1, 1, 0, 0, 2, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0
                ],
                "{grammar:?}"
            );
        }
    }

    #[test]
    fn python_bindings_come_from_declarations_and_in_tree_imports() {
        let source = "from extlib import user as u, other\n\
            from .rel import near\n\
            from pkg.core import charge\n\
            import os\n\
            import pkg.core as pc\n\
            def k(p, q=1, *args, r: int = 2, s: int, **kw):\n    \
                for i, (j, l) in x: pass\n    \
                w = (n := 1)\n    \
                self.attr = 1\n\
            class C(Base): pass\n";
        assert_eq!(
            of(
                FirstClass::Python,
                source,
                &[
                    "user", "u", "other", "near", "charge", "os", "pkg", "pc", "k", "p", "q",
                    "args", "r", "s", "kw", "i", "j", "l", "w", "n", "attr", "self", "C", "Base",
                    "int",
                ]
            ),
            [
                0, 1, 0, 2, 2, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0
            ]
        );
    }

    #[test]
    fn rust_bindings_exclude_external_uses_and_abi_symbols() {
        let source = "use extcrate::{user as u, other};\n\
            use crate::m::{inner::{deep as dd}, near};\n\
            extern \"C\" { fn cfn(id: u32) -> u32; }\n\
            #[no_mangle] pub extern \"C\" fn exported() {}\n\
            fn f(p: u32, S { fx, fy: fz }: S) { let Some(w) = o else { return }; }\n\
            struct St;\n";
        assert_eq!(
            of(
                FirstClass::Rust,
                source,
                &[
                    "user", "u", "other", "deep", "dd", "near", "cfn", "exported", "f", "p", "fx",
                    "fz", "fy", "w", "Some", "St",
                ]
            ),
            [0, 1, 0, 2, 1, 2, 0, 0, 1, 1, 1, 1, 0, 1, 0, 1]
        );
    }

    #[test]
    fn go_bindings_are_file_declarations() {
        let source = "package a\n\
            import u \"ext/user\"\n\
            func F(p int, q, r string) (res int) { v := 1; for i := range s {}; return }\n\
            type T struct{}\n";
        assert_eq!(
            of(
                FirstClass::Go,
                source,
                &["u", "F", "p", "q", "r", "res", "v", "i", "T", "s"]
            ),
            [0, 1, 1, 1, 1, 1, 1, 1, 1, 0]
        );
    }

    #[test]
    fn files_with_non_ascii_identifiers_have_no_bindings() {
        let source = "user = 1\nprint(\u{ff55}ser)\n";
        let tree = crate::parse::parse(FirstClass::Python, source).unwrap();
        assert!(bindings(FirstClass::Python, &tree, source, &|_| false).is_none());
    }

    /// `specifier` for the first identifier of the first occurrence of `needle`.
    fn specifier_at(grammar: FirstClass, source: &str, needle: &str) -> Option<bool> {
        let tree = crate::parse::parse(grammar, source).unwrap();
        let start = source.find(needle).unwrap();
        let length = needle
            .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .unwrap_or(needle.len());
        let node = tree
            .root_node()
            .descendant_for_byte_range(start, start + length)
            .unwrap();
        specifier(grammar, node, source, &|_| false)
    }

    #[test]
    fn specifiers_name_their_module() {
        let script = "import { user as u } from 'ext';\nimport { near } from './near';\n\
            export { far } from 'ext';\nexport { u };\n";
        let at = |needle: &str| specifier_at(FirstClass::TypeScript, script, needle);
        assert_eq!(at("user"), Some(false));
        assert_eq!(at("u }"), None);
        assert_eq!(at("near"), Some(true));
        assert_eq!(at("far"), Some(false));
        assert_eq!(at("u };"), None);
        let rust = "use crate::m::{user as u};\nuse ext::{other};\n";
        let at = |needle: &str| specifier_at(FirstClass::Rust, rust, needle);
        assert_eq!(at("user"), Some(true));
        assert_eq!(at("u}"), None);
        assert_eq!(at("other"), Some(false));
        let python = "from ext import user as u\nfrom .near import near\n";
        let at = |needle: &str| specifier_at(FirstClass::Python, python, needle);
        assert_eq!(at("user"), Some(false));
        assert_eq!(at("near\n"), Some(true));
    }
}
