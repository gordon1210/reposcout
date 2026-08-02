use super::{Deserialize, FirstClass, Node, Serialize, StaticInclude, parse, php, symbols};

// ---------------------------------------------------------------------------
// Import specifier extractors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceFacts {
    pub(super) specifiers: Vec<ImportSpecifier>,
    pub(super) parse_errors: usize,
    pub(super) symbols: symbols::SourceFacts,
}

impl SourceFacts {
    pub(crate) fn parse_error() -> Self {
        Self {
            parse_errors: 1,
            ..Self::default()
        }
    }
}

#[derive(Default)]
pub(super) struct SpecifierExtraction {
    pub(super) specifiers: Vec<ImportSpecifier>,
    pub(super) parse_errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum ImportSpecifier {
    Module(String),
    PhpNamespace(String),
    PhpInclude(StaticInclude),
    Rust(RustImport),
    GoPackage(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum RustImport {
    Module {
        name: String,
        path: Option<String>,
        inline_modules: Vec<String>,
    },
    Use {
        path: String,
        inline_modules: Vec<String>,
    },
}

#[cfg(test)]
pub(super) fn extract_specifiers(fc: FirstClass, content: &str) -> SpecifierExtraction {
    let Some(tree) = parse::parse(fc, content) else {
        return SpecifierExtraction {
            parse_errors: 1,
            ..SpecifierExtraction::default()
        };
    };
    extract_specifiers_from_root(fc, content, tree.root_node())
}

pub(crate) fn extract_source_facts(fc: FirstClass, path: &str, content: &str) -> SourceFacts {
    let Some(tree) = parse::parse(fc, content) else {
        return SourceFacts::parse_error();
    };
    extract_source_facts_from_tree(fc, path, content, tree.root_node())
}

pub(crate) fn extract_source_facts_from_tree(
    fc: FirstClass,
    path: &str,
    content: &str,
    root: Node<'_>,
) -> SourceFacts {
    let extraction = extract_specifiers_from_root(fc, content, root);
    SourceFacts {
        specifiers: extraction.specifiers,
        parse_errors: extraction.parse_errors,
        symbols: symbols::Collector::source_facts(fc, path, content, root),
    }
}

pub(super) fn extract_specifiers_from_root(
    fc: FirstClass,
    content: &str,
    root: Node<'_>,
) -> SpecifierExtraction {
    let mut extraction = SpecifierExtraction {
        parse_errors: count_parse_errors(root),
        ..SpecifierExtraction::default()
    };
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match fc {
            FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx => {
                extract_js_node(node, content, &mut extraction.specifiers);
            }
            FirstClass::Python => extract_python_node(node, content, &mut extraction.specifiers),
            FirstClass::Php => extract_php_node(node, content, &mut extraction.specifiers),
            FirstClass::Rust => extract_rust_node(node, content, &mut extraction.specifiers),
            FirstClass::Go => extract_go_node(node, content, &mut extraction.specifiers),
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(crate::numeric::usize_to_u32(index)) {
                stack.push(child);
            }
        }
    }
    extraction
}

#[cfg(test)]
pub(crate) fn extract_js_specifiers(content: &str) -> Vec<String> {
    module_specifiers(extract_specifiers(FirstClass::JavaScript, content))
}

#[cfg(test)]
pub(crate) fn extract_py_specifiers(content: &str) -> Vec<String> {
    module_specifiers(extract_specifiers(FirstClass::Python, content))
}

#[cfg(test)]
pub(super) fn module_specifiers(extraction: SpecifierExtraction) -> Vec<String> {
    extraction
        .specifiers
        .into_iter()
        .filter_map(|specifier| match specifier {
            ImportSpecifier::Module(value) => Some(value),
            ImportSpecifier::PhpNamespace(_)
            | ImportSpecifier::PhpInclude(_)
            | ImportSpecifier::Rust(_)
            | ImportSpecifier::GoPackage(_) => None,
        })
        .collect()
}

pub(super) fn count_parse_errors(root: Node<'_>) -> usize {
    let mut errors = 0usize;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            errors = errors.saturating_add(1);
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(crate::numeric::usize_to_u32(index)) {
                stack.push(child);
            }
        }
    }
    errors
}

pub(super) fn extract_js_node(node: Node<'_>, content: &str, specs: &mut Vec<ImportSpecifier>) {
    match node.kind() {
        "import_statement" | "export_statement" => {
            if let Some(source) = node.child_by_field_name("source")
                && let Some(specifier) = string_literal(source, content)
            {
                specs.push(ImportSpecifier::Module(specifier));
            }
        }
        "call_expression" => {
            let Some(function) = node.child_by_field_name("function") else {
                return;
            };
            let is_supported = match function.kind() {
                "import" => true,
                "identifier" => function
                    .utf8_text(content.as_bytes())
                    .is_ok_and(|name| name == "require"),
                _ => false,
            };
            if !is_supported {
                return;
            }
            if let Some(arguments) = node.child_by_field_name("arguments") {
                let mut cursor = arguments.walk();
                if let Some(specifier) = arguments
                    .named_children(&mut cursor)
                    .next()
                    .and_then(|argument| string_literal(argument, content))
                {
                    specs.push(ImportSpecifier::Module(specifier));
                }
            }
        }
        _ => {}
    }
}

pub(super) fn extract_python_node(node: Node<'_>, content: &str, specs: &mut Vec<ImportSpecifier>) {
    match node.kind() {
        "import_statement" => {
            let mut cursor = node.walk();
            for imported in node.named_children(&mut cursor) {
                if let Some(module) = python_import_name(imported, content) {
                    specs.push(ImportSpecifier::Module(module));
                }
            }
        }
        "import_from_statement" => {
            let Some(module_node) = node.child_by_field_name("module_name") else {
                return;
            };
            let Ok(module) = module_node.utf8_text(content.as_bytes()) else {
                return;
            };
            if !module.chars().all(|ch| ch == '.') {
                specs.push(ImportSpecifier::Module(module.to_string()));
                return;
            }

            let mut found_name = false;
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.id() == module_node.id() {
                    continue;
                }
                if child.kind() == "wildcard_import" {
                    specs.push(ImportSpecifier::Module(module.to_string()));
                    found_name = true;
                } else if let Some(name) = python_import_name(child, content) {
                    specs.push(ImportSpecifier::Module(format!("{module}{name}")));
                    found_name = true;
                }
            }
            if !found_name {
                specs.push(ImportSpecifier::Module(module.to_string()));
            }
        }
        _ => {}
    }
}

pub(super) fn extract_php_node(node: Node<'_>, content: &str, specs: &mut Vec<ImportSpecifier>) {
    if node.kind() == "namespace_use_declaration" {
        specs.extend(
            php::use_namespaces(node, content)
                .into_iter()
                .map(ImportSpecifier::PhpNamespace),
        );
    } else if let Some(include) = php::static_include(node, content) {
        specs.push(ImportSpecifier::PhpInclude(include));
    }
}

pub(super) fn extract_rust_node(node: Node<'_>, content: &str, specs: &mut Vec<ImportSpecifier>) {
    match node.kind() {
        "mod_item" if node.child_by_field_name("body").is_none() => {
            let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(content.as_bytes()).ok())
            else {
                return;
            };
            specs.push(ImportSpecifier::Rust(RustImport::Module {
                name: name.to_string(),
                path: rust_path_attribute(node, content),
                inline_modules: enclosing_inline_rust_modules(node, content),
            }));
        }
        "use_declaration" => {
            let Some(argument) = node.child_by_field_name("argument") else {
                return;
            };
            let mut paths = Vec::new();
            expand_rust_use(argument, content, "", &mut paths);
            paths.sort();
            paths.dedup();
            let inline_modules = enclosing_inline_rust_modules(node, content);
            specs.extend(paths.into_iter().map(|path| {
                ImportSpecifier::Rust(RustImport::Use {
                    path,
                    inline_modules: inline_modules.clone(),
                })
            }));
        }
        _ => {}
    }
}

pub(super) fn expand_rust_use(
    node: Node<'_>,
    content: &str,
    prefix: &str,
    paths: &mut Vec<String>,
) {
    match node.kind() {
        "scoped_use_list" => {
            let next_prefix = node
                .child_by_field_name("path")
                .and_then(|path| rust_node_text(path, content))
                .map_or_else(|| prefix.to_string(), |path| join_rust_path(prefix, &path));
            if let Some(list) = node.child_by_field_name("list") {
                expand_rust_use(list, content, &next_prefix, paths);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                expand_rust_use(child, content, prefix, paths);
            }
        }
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                expand_rust_use(path, content, prefix, paths);
            }
        }
        "use_wildcard" => {
            if let Some(path) = node.child_by_field_name("path")
                && let Some(path) = rust_node_text(path, content)
            {
                paths.push(join_rust_path(prefix, &path));
            }
        }
        "identifier" | "scoped_identifier" | "crate" | "self" | "super" => {
            if let Some(path) = rust_node_text(node, content) {
                paths.push(join_rust_path(prefix, &path));
            }
        }
        _ => {}
    }
}

pub(super) fn rust_node_text(node: Node<'_>, content: &str) -> Option<String> {
    let text = node.utf8_text(content.as_bytes()).ok()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

pub(super) fn join_rust_path(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        suffix.to_string()
    } else if suffix == "self" {
        prefix.to_string()
    } else {
        format!("{prefix}::{suffix}")
    }
}

pub(super) fn enclosing_inline_rust_modules(node: Node<'_>, content: &str) -> Vec<String> {
    let mut modules = Vec::new();
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if parent.kind() == "mod_item"
            && parent.child_by_field_name("body").is_some()
            && let Some(name) = parent
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(content.as_bytes()).ok())
        {
            modules.push(name.to_string());
        }
        ancestor = parent.parent();
    }
    modules.reverse();
    modules
}

pub(super) fn rust_path_attribute(node: Node<'_>, content: &str) -> Option<String> {
    let mut sibling = node.prev_named_sibling();
    while let Some(attribute) = sibling {
        if attribute.kind() != "attribute_item" {
            break;
        }
        let text = attribute.utf8_text(content.as_bytes()).ok()?.trim();
        if text.starts_with("#[path") {
            let value = text.split_once('=')?.1.trim();
            let value = value.strip_suffix(']')?.trim();
            return strip_static_quotes(value).map(str::to_string);
        }
        sibling = attribute.prev_named_sibling();
    }
    None
}

pub(super) fn extract_go_node(node: Node<'_>, content: &str, specs: &mut Vec<ImportSpecifier>) {
    if node.kind() != "import_spec" {
        return;
    }
    let Some(path) = node.child_by_field_name("path") else {
        return;
    };
    let Some(path) = path
        .utf8_text(content.as_bytes())
        .ok()
        .map(str::trim)
        .and_then(strip_static_quotes)
    else {
        return;
    };
    specs.push(ImportSpecifier::GoPackage(path.to_string()));
}

pub(super) fn strip_static_quotes(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let quote = bytes[0];
    (matches!(quote, b'\'' | b'"' | b'`') && bytes.last().copied() == Some(quote))
        .then(|| &text[1..text.len() - 1])
}

pub(super) fn python_import_name(node: Node<'_>, content: &str) -> Option<String> {
    let node = if node.kind() == "aliased_import" {
        node.child_by_field_name("name")?
    } else {
        node
    };
    matches!(node.kind(), "dotted_name" | "identifier")
        .then(|| node.utf8_text(content.as_bytes()).ok().map(str::to_string))
        .flatten()
}

pub(super) fn string_literal(node: Node<'_>, content: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let text = node.utf8_text(content.as_bytes()).ok()?;
    let quote = text.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"' | b'`') || text.as_bytes().last().copied() != Some(quote) {
        return None;
    }
    Some(text[1..text.len().saturating_sub(1)].to_string())
}
