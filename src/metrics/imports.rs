//! Import / dependency extraction for first-class languages.
//!
//! ## Contract (frozen)
//! `extract(fc, content, tree) -> Vec<String>`
//! Returns the list of imported modules/packages (deduplicated, order-stable),
//! e.g. Rust `use`/`extern crate`, Python `import`/`from`, JS/TS `import`/
//! `require`, Go `import`, PHP namespace `use`. `tree` is the parsed tree-sitter
//! tree for `fc`.
//!
//! Implemented with small manual tree-sitter walks to avoid depending on query
//! node names beyond the import-related node kinds.

use crate::{lang::FirstClass, php};
use std::collections::HashSet;
use tree_sitter::{Node, Tree};

#[must_use]
pub fn extract(fc: FirstClass, content: &str, tree: &Tree) -> Vec<String> {
    let mut imports = Vec::new();
    let mut seen = HashSet::new();
    let mut add = |value: String| {
        if !value.is_empty() && seen.insert(value.clone()) {
            imports.push(value);
        }
    };

    walk(tree.root_node(), &mut |node| match fc {
        FirstClass::Rust => extract_rust(node, content, &mut add),
        FirstClass::Python => extract_python(node, content, &mut add),
        FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx => {
            extract_javascript(node, content, &mut add);
        }
        FirstClass::Go => extract_go(node, content, &mut add),
        FirstClass::Php => extract_php(node, content, &mut add),
    });

    imports
}

fn extract_php<F>(node: Node<'_>, content: &str, add: &mut F)
where
    F: FnMut(String),
{
    for namespace in php::use_namespaces(node, content) {
        if let Some(root) = php::namespace_root(&namespace) {
            add(root);
        }
    }
}

fn walk<F>(node: Node<'_>, visit: &mut F)
where
    F: FnMut(Node<'_>),
{
    visit(node);

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, visit);
    }
}

fn extract_rust<F>(node: Node<'_>, content: &str, add: &mut F)
where
    F: FnMut(String),
{
    if matches!(node.kind(), "use_declaration" | "extern_crate_declaration")
        && let Some(segment) = first_rust_segment(node, content)
    {
        add(segment);
    }
}

fn first_rust_segment(node: Node<'_>, content: &str) -> Option<String> {
    if matches!(node.kind(), "identifier" | "crate" | "self" | "super") {
        let text = node.utf8_text(content.as_bytes()).ok()?.trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(segment) = first_rust_segment(child, content) {
            return Some(segment);
        }
    }

    None
}

fn extract_python<F>(node: Node<'_>, content: &str, add: &mut F)
where
    F: FnMut(String),
{
    match node.kind() {
        "import_statement" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                match child.kind() {
                    "dotted_name" => {
                        if let Some(module) = python_root_from_node(child, content) {
                            add(module);
                        }
                    }
                    "aliased_import" => {
                        if let Some(module) = first_python_module_name(child, content)
                            && let Some(root) = python_root(&module)
                        {
                            add(root);
                        }
                    }
                    _ => {}
                }
            }
        }
        "import_from_statement" => {
            if let Some(module) = node.child_by_field_name("module_name")
                && let Some(root) = python_root_from_node(module, content)
            {
                add(root);
            }
        }
        _ => {}
    }
}

fn python_root_from_node(node: Node<'_>, content: &str) -> Option<String> {
    let module = node.utf8_text(content.as_bytes()).ok()?;
    python_root(module)
}

fn python_root(module: &str) -> Option<String> {
    let module = module.trim();
    if module.starts_with('.') {
        return None;
    }

    module
        .split('.')
        .next()
        .filter(|root| !root.is_empty())
        .map(str::to_string)
}

fn first_python_module_name(node: Node<'_>, content: &str) -> Option<String> {
    if matches!(node.kind(), "dotted_name" | "identifier") {
        let text = node.utf8_text(content.as_bytes()).ok()?.trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(module) = first_python_module_name(child, content) {
            return Some(module);
        }
    }

    None
}

fn extract_javascript<F>(node: Node<'_>, content: &str, add: &mut F)
where
    F: FnMut(String),
{
    match node.kind() {
        "import_statement" | "export_statement" => {
            if let Some(source) = node
                .child_by_field_name("source")
                .and_then(|source| string_text(source, content))
                && let Some(root) = javascript_package_root(&source)
            {
                add(root);
            }
        }
        "call_expression" => {
            if is_require_or_import_call(node, content)
                && let Some(args) = node
                    .child_by_field_name("arguments")
                    .or_else(|| named_child_of_kind(node, "arguments"))
                && let Some(source) = first_direct_string_argument(args, content)
                && let Some(root) = javascript_package_root(&source)
            {
                add(root);
            }
        }
        _ => {}
    }
}

fn javascript_package_root(specifier: &str) -> Option<String> {
    let specifier = specifier.trim();
    if specifier.is_empty() || specifier.starts_with('.') {
        return None;
    }

    let mut segments = specifier.split('/');
    let first = segments.next()?;
    if first.starts_with('@') {
        let package = segments.next()?;
        if first.len() == 1 || package.is_empty() {
            return None;
        }
        Some(format!("{first}/{package}"))
    } else {
        Some(first.to_string())
    }
}

fn is_require_or_import_call(node: Node<'_>, content: &str) -> bool {
    node.child_by_field_name("function")
        .or_else(|| node.child(0))
        .and_then(|function| function.utf8_text(content.as_bytes()).ok())
        .is_some_and(|text| matches!(text.trim(), "require" | "import"))
}

fn first_direct_string_argument(node: Node<'_>, content: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if is_string_node(child) {
            return string_text(child, content);
        }
    }

    None
}

fn extract_go<F>(node: Node<'_>, content: &str, add: &mut F)
where
    F: FnMut(String),
{
    if node.kind() != "import_declaration" {
        return;
    }

    let mut import_specs = Vec::new();
    collect_kind(node, "import_spec", &mut import_specs);
    for import_spec in import_specs {
        if let Some(source) = first_string_literal(import_spec, content) {
            add(source);
        }
    }
}

fn collect_kind<'a>(node: Node<'a>, kind: &str, out: &mut Vec<Node<'a>>) {
    if node.kind() == kind {
        out.push(node);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_kind(child, kind, out);
    }
}

fn first_string_literal(node: Node<'_>, content: &str) -> Option<String> {
    if is_string_node(node) {
        return string_text(node, content);
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(source) = first_string_literal(child, content) {
            return Some(source);
        }
    }

    None
}

fn is_string_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "string" | "string_literal" | "interpreted_string_literal" | "raw_string_literal"
    )
}

fn string_text(node: Node<'_>, content: &str) -> Option<String> {
    let text = node.utf8_text(content.as_bytes()).ok()?.trim();
    Some(strip_quotes(text).to_string())
}

fn strip_quotes(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if matches!(first, b'\'' | b'"' | b'`') && first == last {
            return &text[1..text.len() - 1];
        }
    }

    text
}

fn named_child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

#[cfg(test)]
mod tests {
    use super::extract;
    use crate::{lang::FirstClass, parse};

    #[test]
    fn extracts_rust_import_roots() {
        let src = "use std::collections::HashMap;\nuse serde::Serialize;\nuse std::fmt;\n";
        let tree = parse::parse(FirstClass::Rust, src).unwrap();

        assert_eq!(extract(FirstClass::Rust, src, &tree), vec!["std", "serde"]);
    }

    #[test]
    fn extracts_python_imports() {
        let src = "import os\nimport sys as system\nfrom collections import OrderedDict\n";
        let tree = parse::parse(FirstClass::Python, src).unwrap();

        assert_eq!(
            extract(FirstClass::Python, src, &tree),
            vec!["os", "sys", "collections"]
        );
    }

    #[test]
    fn extracts_python_root_imports_and_skips_relative_modules() {
        let src = concat!(
            "import os.path\n",
            "import numpy.linalg as linear_algebra\n",
            "from collections.abc import Iterable\n",
            "from . import sibling\n",
            "from .local import helper\n",
            "from ..parent import value\n",
        );
        let tree = parse::parse(FirstClass::Python, src).unwrap();

        assert_eq!(
            extract(FirstClass::Python, src, &tree),
            vec!["os", "numpy", "collections"]
        );
    }

    #[test]
    fn extracts_javascript_imports() {
        let src = "import React from 'react';\nexport { x } from 'shared/x';\nconst fs = require('fs');\nconst lazy = import(\"lazy\");\n";
        let tree = parse::parse(FirstClass::JavaScript, src).unwrap();

        assert_eq!(
            extract(FirstClass::JavaScript, src, &tree),
            vec!["react", "shared", "fs", "lazy"]
        );
    }

    #[test]
    fn extracts_javascript_package_roots_and_skips_relative_modules() {
        let src = concat!(
            "import map from 'lodash/fp';\n",
            "import thing from '@scope/package/subpath';\n",
            "const read = require('node:fs/promises');\n",
            "const sibling = require('./sibling');\n",
            "const parent = import('../parent');\n",
        );
        let tree = parse::parse(FirstClass::JavaScript, src).unwrap();

        assert_eq!(
            extract(FirstClass::JavaScript, src, &tree),
            vec!["lodash", "@scope/package", "node:fs"]
        );
    }

    #[test]
    fn extracts_typescript_imports() {
        let src = "import type { Foo } from '@scope/pkg';\nconst mod = require('mod');\n";
        let tree = parse::parse(FirstClass::TypeScript, src).unwrap();

        assert_eq!(
            extract(FirstClass::TypeScript, src, &tree),
            vec!["@scope/pkg", "mod"]
        );
    }

    #[test]
    fn extracts_tsx_imports() {
        let src = "import React from 'react';\nexport const el = <div />;\n";
        let tree = parse::parse(FirstClass::Tsx, src).unwrap();

        assert_eq!(extract(FirstClass::Tsx, src, &tree), vec!["react"]);
    }

    #[test]
    fn extracts_go_imports() {
        let src = "package main\nimport (\n  \"fmt\"\n  \"github.com/x/y\"\n)\n";
        let tree = parse::parse(FirstClass::Go, src).unwrap();

        assert_eq!(
            extract(FirstClass::Go, src, &tree),
            vec!["fmt", "github.com/x/y"]
        );
    }

    #[test]
    fn extracts_php_namespace_roots() {
        let src = concat!(
            "<?php\n",
            "use Symfony\\Component\\HttpFoundation\\Request;\n",
            "use Psr\\Log\\{LoggerInterface, NullLogger as Logger};\n",
            "use function App\\Support\\helper;\n",
            "use Symfony\\Component\\Console\\Application;\n",
        );
        let tree = parse::parse(FirstClass::Php, src).unwrap();

        assert_eq!(
            extract(FirstClass::Php, src, &tree),
            vec!["Symfony", "Psr", "App"]
        );
    }
}
