//! Explicit type-relationship extraction for the on-demand repository graph.
//!
//! This module deliberately keeps a small interface: callers feed each parsed
//! first-class source file into [`Collector`], then consume one deterministic
//! [`SymbolTopology`]. Resolution is conservative; ambiguous names remain
//! diagnostics rather than becoming invented architecture.

use crate::lang::FirstClass;
use crate::model::{GraphSymbol, GraphSymbolEdge};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use tree_sitter::Node;

#[derive(Default)]
pub(super) struct Collector {
    declarations: Vec<Declaration>,
    relations: Vec<UnresolvedRelation>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct SourceFacts {
    declarations: Vec<Declaration>,
    relations: Vec<UnresolvedRelation>,
}

#[derive(Default)]
pub(super) struct SymbolTopology {
    pub symbols: Vec<GraphSymbol>,
    pub edges: Vec<GraphSymbolEdge>,
    pub unresolved_relations: usize,
    pub unresolved_by_path: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Declaration {
    id: String,
    name: String,
    qualified_name: String,
    kind: String,
    path: String,
    language: String,
    family: String,
    line: usize,
    scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum RelationSource {
    Declaration(String),
    Reference(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UnresolvedRelation {
    source: RelationSource,
    target: String,
    relation: String,
    path: String,
    family: String,
    scope: String,
    namespace: String,
    aliases: HashMap<String, String>,
}

struct SourceContext {
    path: String,
    language: String,
    family: &'static str,
    scope: String,
    namespace: String,
    aliases: HashMap<String, String>,
}

impl Collector {
    pub fn source_facts(
        language: FirstClass,
        path: &str,
        content: &str,
        root: Node<'_>,
    ) -> SourceFacts {
        let mut collector = Self::default();
        collector.add_source(language, path, content, root);
        SourceFacts {
            declarations: collector.declarations,
            relations: collector.relations,
        }
    }

    pub fn add_facts(&mut self, facts: SourceFacts) {
        self.declarations.extend(facts.declarations);
        self.relations.extend(facts.relations);
    }

    pub fn add_source(&mut self, language: FirstClass, path: &str, content: &str, root: Node<'_>) {
        let context = SourceContext::new(language, path, content, root);
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            match language {
                FirstClass::Php => self.extract_php(node, content, &context),
                FirstClass::JavaScript => self.extract_javascript(node, content, &context),
                FirstClass::TypeScript | FirstClass::Tsx => {
                    self.extract_typescript(node, content, &context);
                }
                FirstClass::Python => self.extract_python(node, content, &context),
                FirstClass::Rust => self.extract_rust(node, content, &context),
                FirstClass::Go => self.extract_go(node, content, &context),
            }
            for index in (0..node.named_child_count()).rev() {
                if let Some(child) = node.named_child(index as u32) {
                    stack.push(child);
                }
            }
        }
    }

    pub fn finish(self) -> SymbolTopology {
        let mut qualified = HashMap::<(&str, String), Vec<usize>>::new();
        let mut simple = HashMap::<(&str, String), Vec<usize>>::new();
        let mut by_id = HashMap::<String, usize>::new();
        for (index, declaration) in self.declarations.iter().enumerate() {
            qualified
                .entry((
                    declaration.family.as_str(),
                    normalize_name(&declaration.family, &declaration.qualified_name),
                ))
                .or_default()
                .push(index);
            simple
                .entry((
                    declaration.family.as_str(),
                    normalize_name(&declaration.family, &declaration.name),
                ))
                .or_default()
                .push(index);
            by_id.insert(declaration.id.clone(), index);
        }

        let mut unresolved_relations = 0usize;
        let mut unresolved_by_path = HashMap::<String, usize>::new();
        let mut resolved = BTreeMap::<(String, String, String), String>::new();
        for relation in self.relations {
            let source = match &relation.source {
                RelationSource::Declaration(id) => {
                    by_id.get(id).copied().map(|index| (index, "declaration"))
                }
                RelationSource::Reference(reference) => resolve_reference(
                    reference,
                    &relation,
                    &self.declarations,
                    &qualified,
                    &simple,
                ),
            };
            let target = resolve_reference(
                &relation.target,
                &relation,
                &self.declarations,
                &qualified,
                &simple,
            );
            let (Some((source, _)), Some((target, resolver))) = (source, target) else {
                unresolved_relations = unresolved_relations.saturating_add(1);
                *unresolved_by_path.entry(relation.path.clone()).or_default() += 1;
                continue;
            };
            if source == target {
                continue;
            }
            let source_id = self.declarations[source].id.clone();
            let target_id = self.declarations[target].id.clone();
            resolved
                .entry((source_id, target_id, relation.relation))
                .and_modify(|current| {
                    if resolver_rank(resolver) < resolver_rank(current) {
                        *current = resolver.to_string();
                    }
                })
                .or_insert_with(|| resolver.to_string());
        }

        let mut fan_in = HashMap::<String, usize>::new();
        let mut fan_out = HashMap::<String, usize>::new();
        let mut retained = HashSet::<String>::new();
        let edges = resolved
            .into_iter()
            .map(|((source, target, relation), resolver)| {
                *fan_out.entry(source.clone()).or_default() += 1;
                *fan_in.entry(target.clone()).or_default() += 1;
                retained.insert(source.clone());
                retained.insert(target.clone());
                GraphSymbolEdge {
                    source,
                    target,
                    relation,
                    resolver,
                }
            })
            .collect::<Vec<_>>();

        let mut symbols = self
            .declarations
            .into_iter()
            .filter(|declaration| retained.contains(&declaration.id))
            .map(|declaration| GraphSymbol {
                fan_in: fan_in.get(&declaration.id).copied().unwrap_or_default(),
                fan_out: fan_out.get(&declaration.id).copied().unwrap_or_default(),
                id: declaration.id,
                name: declaration.name,
                qualified_name: declaration.qualified_name,
                kind: declaration.kind,
                path: declaration.path,
                language: declaration.language,
                line: declaration.line,
            })
            .collect::<Vec<_>>();
        symbols.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.qualified_name.cmp(&right.qualified_name))
        });

        SymbolTopology {
            symbols,
            edges,
            unresolved_relations,
            unresolved_by_path,
        }
    }

    fn add_declaration(
        &mut self,
        node: Node<'_>,
        content: &str,
        context: &SourceContext,
        kind: &str,
    ) -> Option<String> {
        let name = node_text(node.child_by_field_name("name")?, content)?;
        let qualified_name = qualify_declaration(&name, context);
        let line = node.start_position().row + 1;
        let id = format!("{}#L{}:{}:{}", context.path, line, kind, qualified_name);
        self.declarations.push(Declaration {
            id: id.clone(),
            name,
            qualified_name,
            kind: kind.to_string(),
            path: context.path.clone(),
            language: context.language.clone(),
            family: context.family.to_string(),
            line,
            scope: context.scope.clone(),
        });
        Some(id)
    }

    fn add_relation(
        &mut self,
        source: RelationSource,
        target: String,
        relation: &str,
        context: &SourceContext,
    ) {
        if clean_reference(&target).is_empty() {
            return;
        }
        self.relations.push(UnresolvedRelation {
            source,
            target,
            relation: relation.to_string(),
            path: context.path.clone(),
            family: context.family.to_string(),
            scope: context.scope.clone(),
            namespace: context.namespace.clone(),
            aliases: context.aliases.clone(),
        });
    }

    fn extract_php(&mut self, node: Node<'_>, content: &str, context: &SourceContext) {
        let (kind, base_relation, interface_relation) = match node.kind() {
            "class_declaration" => ("class", Some("extends"), Some("implements")),
            "interface_declaration" => ("interface", Some("extends"), None),
            "trait_declaration" => ("trait", None, None),
            "enum_declaration" => ("enum", None, Some("implements")),
            _ => return,
        };
        let Some(id) = self.add_declaration(node, content, context, kind) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let relation = match child.kind() {
                "base_clause" => base_relation,
                "class_interface_clause" => interface_relation,
                _ => None,
            };
            if let Some(relation) = relation {
                for target in named_reference_children(child, content) {
                    self.add_relation(
                        RelationSource::Declaration(id.clone()),
                        target,
                        relation,
                        context,
                    );
                }
            }
        }
    }

    fn extract_javascript(&mut self, node: Node<'_>, content: &str, context: &SourceContext) {
        if node.kind() != "class_declaration" {
            return;
        }
        let Some(id) = self.add_declaration(node, content, context, "class") else {
            return;
        };
        if let Some(heritage) = child_of_kind(node, "class_heritage") {
            for target in named_reference_children(heritage, content) {
                self.add_relation(
                    RelationSource::Declaration(id.clone()),
                    target,
                    "extends",
                    context,
                );
            }
        }
    }

    fn extract_typescript(&mut self, node: Node<'_>, content: &str, context: &SourceContext) {
        let kind = match node.kind() {
            "class_declaration" => "class",
            "interface_declaration" => "interface",
            _ => return,
        };
        let Some(id) = self.add_declaration(node, content, context, kind) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "class_heritage" => {
                    let mut heritage_cursor = child.walk();
                    for clause in child.named_children(&mut heritage_cursor) {
                        let relation = match clause.kind() {
                            "extends_clause" => Some("extends"),
                            "implements_clause" => Some("implements"),
                            _ => None,
                        };
                        if let Some(relation) = relation {
                            for target in named_reference_children(clause, content) {
                                self.add_relation(
                                    RelationSource::Declaration(id.clone()),
                                    target,
                                    relation,
                                    context,
                                );
                            }
                        }
                    }
                }
                "extends_type_clause" => {
                    for target in named_reference_children(child, content) {
                        self.add_relation(
                            RelationSource::Declaration(id.clone()),
                            target,
                            "extends",
                            context,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_python(&mut self, node: Node<'_>, content: &str, context: &SourceContext) {
        if node.kind() != "class_definition" {
            return;
        }
        let Some(id) = self.add_declaration(node, content, context, "class") else {
            return;
        };
        let Some(superclasses) = node.child_by_field_name("superclasses") else {
            return;
        };
        for target in named_reference_children(superclasses, content) {
            self.add_relation(
                RelationSource::Declaration(id.clone()),
                target,
                "extends",
                context,
            );
        }
    }

    fn extract_rust(&mut self, node: Node<'_>, content: &str, context: &SourceContext) {
        let kind = match node.kind() {
            "struct_item" => Some("struct"),
            "enum_item" => Some("enum"),
            "trait_item" => Some("trait"),
            "type_item" => Some("type"),
            _ => None,
        };
        if let Some(kind) = kind {
            let Some(id) = self.add_declaration(node, content, context, kind) else {
                return;
            };
            if node.kind() == "trait_item"
                && let Some(bounds) = node.child_by_field_name("bounds")
            {
                for target in named_reference_children(bounds, content) {
                    self.add_relation(
                        RelationSource::Declaration(id.clone()),
                        target,
                        "extends",
                        context,
                    );
                }
            }
            return;
        }
        if node.kind() != "impl_item" {
            return;
        }
        let Some(trait_node) = node.child_by_field_name("trait") else {
            return;
        };
        let (Some(source), Some(target)) = (
            node.child_by_field_name("type")
                .and_then(|value| node_text(value, content)),
            node_text(trait_node, content),
        ) else {
            return;
        };
        self.add_relation(
            RelationSource::Reference(source),
            target,
            "implements",
            context,
        );
    }

    fn extract_go(&mut self, node: Node<'_>, content: &str, context: &SourceContext) {
        if node.kind() != "type_spec" {
            return;
        }
        let Some(type_node) = node.child_by_field_name("type") else {
            return;
        };
        let kind = match type_node.kind() {
            "interface_type" => "interface",
            "struct_type" => "struct",
            _ => "type",
        };
        let Some(id) = self.add_declaration(node, content, context, kind) else {
            return;
        };
        let mut stack = vec![type_node];
        while let Some(candidate) = stack.pop() {
            let target = if candidate.kind() == "type_elem" {
                node_text(candidate, content)
            } else if candidate.kind() == "field_declaration"
                && candidate.child_by_field_name("name").is_none()
            {
                candidate
                    .child_by_field_name("type")
                    .and_then(|value| node_text(value, content))
            } else {
                None
            };
            if let Some(target) = target {
                self.add_relation(
                    RelationSource::Declaration(id.clone()),
                    target,
                    "embeds",
                    context,
                );
                continue;
            }
            for index in (0..candidate.named_child_count()).rev() {
                if let Some(child) = candidate.named_child(index as u32) {
                    stack.push(child);
                }
            }
        }
    }
}

impl SourceContext {
    fn new(language: FirstClass, path: &str, content: &str, root: Node<'_>) -> Self {
        let family = language_family(language);
        let namespace = if language == FirstClass::Php {
            php_namespace(root, content)
        } else {
            String::new()
        };
        Self {
            path: path.to_string(),
            language: language_name(language).to_string(),
            family,
            scope: symbol_scope(language, path, &namespace),
            namespace,
            aliases: source_aliases(language, root, content),
        }
    }
}

fn resolve_reference(
    reference: &str,
    relation: &UnresolvedRelation,
    declarations: &[Declaration],
    qualified: &HashMap<(&str, String), Vec<usize>>,
    simple: &HashMap<(&str, String), Vec<usize>>,
) -> Option<(usize, &'static str)> {
    let cleaned = clean_reference(reference);
    if cleaned.is_empty() {
        return None;
    }
    let family = relation.family.as_str();
    let normalized = normalize_name(family, &cleaned);
    let first = first_segment(&normalized);
    let aliased = relation.aliases.get(&first).map(|target| {
        let suffix = normalized
            .strip_prefix(&first)
            .unwrap_or_default()
            .trim_start_matches(['\\', ':', '.', '/']);
        if suffix.is_empty() {
            target.clone()
        } else {
            join_qualified(family, target, suffix)
        }
    });
    let namespaced = (!relation.namespace.is_empty() && !is_qualified(reference))
        .then(|| join_qualified(family, &relation.namespace, &normalized));
    for candidate in aliased
        .iter()
        .chain(namespaced.iter())
        .chain(std::iter::once(&normalized))
    {
        let candidate = normalize_name(family, candidate);
        if let Some(indices) = qualified.get(&(family, candidate))
            && indices.len() == 1
        {
            return Some((indices[0], "qualified"));
        }
    }

    let simple_name = normalize_name(family, simple_name(&normalized));
    let candidates = simple.get(&(family, simple_name))?;
    let same_file = candidates
        .iter()
        .copied()
        .filter(|&index| declarations[index].path == relation.path)
        .collect::<Vec<_>>();
    if same_file.len() == 1 {
        return Some((same_file[0], "same-file"));
    }
    let same_scope = candidates
        .iter()
        .copied()
        .filter(|&index| declarations[index].scope == relation.scope)
        .collect::<Vec<_>>();
    if same_scope.len() == 1 {
        return Some((same_scope[0], "same-scope"));
    }
    (candidates.len() == 1).then_some((candidates[0], "unique-name"))
}

fn source_aliases(language: FirstClass, root: Node<'_>, content: &str) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let pair = match (language, node.kind()) {
            (FirstClass::Php, "namespace_use_clause") => {
                let alias_node = node.child_by_field_name("alias");
                let mut cursor = node.walk();
                let imported = node
                    .named_children(&mut cursor)
                    .find(|child| alias_node.is_none_or(|alias| alias.id() != child.id()))
                    .and_then(|value| node_text(value, content));
                let alias = node
                    .child_by_field_name("alias")
                    .and_then(|value| node_text(value, content))
                    .or_else(|| imported.as_deref().map(simple_name).map(str::to_string));
                imported.zip(alias)
            }
            (
                FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx,
                "import_specifier",
            )
            | (FirstClass::Python, "aliased_import")
            | (FirstClass::Rust, "use_as_clause") => {
                let imported = node
                    .child_by_field_name(if language == FirstClass::Rust {
                        "path"
                    } else {
                        "name"
                    })
                    .and_then(|value| node_text(value, content));
                let alias = node
                    .child_by_field_name("alias")
                    .and_then(|value| node_text(value, content));
                imported.zip(alias)
            }
            _ => None,
        };
        if let Some((imported, alias)) = pair {
            aliases.insert(
                normalize_name(language_family(language), &alias),
                normalize_name(language_family(language), &imported),
            );
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(index as u32) {
                stack.push(child);
            }
        }
    }
    aliases
}

fn php_namespace(root: Node<'_>, content: &str) -> String {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "namespace_definition"
            && let Some(name) = node
                .child_by_field_name("name")
                .and_then(|value| node_text(value, content))
        {
            return clean_reference(&name);
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(index as u32) {
                stack.push(child);
            }
        }
    }
    String::new()
}

fn qualify_declaration(name: &str, context: &SourceContext) -> String {
    if !context.namespace.is_empty() {
        return join_qualified(context.family, &context.namespace, name);
    }
    match context.family {
        "python" => join_qualified("python", &python_module(&context.path), name),
        "rust" => join_qualified("rust", &context.scope.replace('/', "::"), name),
        "go" => join_qualified("go", &context.scope, name),
        _ => name.to_string(),
    }
}

fn symbol_scope(language: FirstClass, path: &str, namespace: &str) -> String {
    if language == FirstClass::Php && !namespace.is_empty() {
        return normalize_name("php", namespace);
    }
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn python_module(path: &str) -> String {
    let without_extension = path
        .strip_suffix(".py")
        .or_else(|| path.strip_suffix(".pyi"))
        .or_else(|| path.strip_suffix(".pyw"))
        .unwrap_or(path);
    without_extension
        .strip_suffix("/__init__")
        .unwrap_or(without_extension)
        .replace('/', ".")
}

fn named_reference_children(node: Node<'_>, content: &str) -> Vec<String> {
    let mut references = BTreeSet::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if is_reference_node(child) {
            if let Some(value) = node_text(child, content) {
                references.insert(value);
            }
        } else if matches!(
            child.kind(),
            "type_arguments" | "type_parameters" | "keyword_argument" | "comment"
        ) {
            continue;
        } else if matches!(
            child.kind(),
            "argument_list"
                | "base_clause"
                | "class_heritage"
                | "class_interface_clause"
                | "extends_clause"
                | "extends_type_clause"
                | "implements_clause"
                | "parenthesized_type"
                | "trait_bounds"
        ) {
            let nested = named_reference_children(child, content);
            references.extend(nested);
        }
    }
    references.into_iter().collect()
}

fn is_reference_node(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "name"
            | "qualified_name"
            | "relative_name"
            | "identifier"
            | "type_identifier"
            | "nested_type_identifier"
            | "scoped_type_identifier"
            | "generic_type"
            | "dotted_name"
            | "attribute"
            | "member_expression"
            | "qualified_type"
            | "subscript"
    )
}

fn child_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn node_text(node: Node<'_>, content: &str) -> Option<String> {
    let text = node.utf8_text(content.as_bytes()).ok()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn clean_reference(reference: &str) -> String {
    let mut value = reference.trim();
    for prefix in ["dyn ", "impl ", "&mut ", "&", "*", "?"] {
        value = value.strip_prefix(prefix).unwrap_or(value).trim();
    }
    if let Some(index) = value.find(['<', '[']) {
        value = &value[..index];
    }
    value
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '(' | ')' | '\'' | '"')
        })
        .to_string()
}

fn normalize_name(family: &str, value: &str) -> String {
    let value = clean_reference(value)
        .trim_start_matches(['\\', ':', '.', '/'])
        .replace("\\\\", "\\");
    if family == "php" {
        value.to_lowercase()
    } else {
        value
    }
}

fn simple_name(value: &str) -> &str {
    value
        .rsplit(['\\', ':', '.', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(value)
}

fn first_segment(value: &str) -> String {
    value
        .split(['\\', ':', '.', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(value)
        .to_string()
}

fn join_qualified(family: &str, prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        return suffix.to_string();
    }
    if suffix.is_empty() {
        return prefix.to_string();
    }
    let separator = match family {
        "php" => "\\",
        "rust" => "::",
        "go" => "/",
        _ => ".",
    };
    format!(
        "{}{}{}",
        prefix.trim_end_matches(['\\', ':', '.', '/']),
        separator,
        suffix.trim_start_matches(['\\', ':', '.', '/'])
    )
}

fn is_qualified(value: &str) -> bool {
    value.starts_with(['\\', ':', '.', '/'])
        || value.contains('\\')
        || value.contains("::")
        || value.contains('.')
        || value.contains('/')
}

fn resolver_rank(resolver: &str) -> usize {
    match resolver {
        "qualified" => 0,
        "same-file" => 1,
        "same-scope" => 2,
        "unique-name" => 3,
        _ => 4,
    }
}

fn language_family(language: FirstClass) -> &'static str {
    match language {
        FirstClass::Rust => "rust",
        FirstClass::Python => "python",
        FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx => "javascript",
        FirstClass::Go => "go",
        FirstClass::Php => "php",
    }
}

fn language_name(language: FirstClass) -> &'static str {
    match language {
        FirstClass::Rust => "Rust",
        FirstClass::Python => "Python",
        FirstClass::JavaScript => "JavaScript",
        FirstClass::TypeScript => "TypeScript",
        FirstClass::Tsx => "TSX",
        FirstClass::Go => "Go",
        FirstClass::Php => "PHP",
    }
}

#[cfg(test)]
mod tests {
    use super::Collector;
    use crate::lang::FirstClass;
    use crate::parse;

    fn topology(sources: &[(FirstClass, &str, &str)]) -> super::SymbolTopology {
        let mut collector = Collector::default();
        for &(language, path, content) in sources {
            let tree = parse::parse(language, content).expect("fixture parses");
            collector.add_source(language, path, content, tree.root_node());
        }
        collector.finish()
    }

    #[test]
    fn resolves_explicit_relationships_for_every_first_class_language() {
        let sources = [
            (
                FirstClass::Php,
                "php/Base.php",
                "<?php namespace App; interface Port {} abstract class Base {}",
            ),
            (
                FirstClass::Php,
                "php/Child.php",
                "<?php namespace App; class Child extends Base implements Port {}",
            ),
            (
                FirstClass::TypeScript,
                "ts/types.ts",
                "interface Port {}\nclass Base {}\nclass Child extends Base implements Port {}",
            ),
            (
                FirstClass::JavaScript,
                "js/types.js",
                "class Base {}\nclass Child extends Base {}",
            ),
            (
                FirstClass::Python,
                "py/types.py",
                "class Base:\n    pass\nclass Child(Base):\n    pass\n",
            ),
            (
                FirstClass::Rust,
                "rust/types.rs",
                "trait Port {}\nstruct Worker;\nimpl Port for Worker {}",
            ),
            (
                FirstClass::Go,
                "go/types.go",
                "package graph\ntype Reader interface { Read() }\ntype Buffered interface { Reader }",
            ),
        ];

        let result = topology(&sources);
        let relations = result
            .edges
            .iter()
            .map(|edge| edge.relation.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            relations
                .iter()
                .filter(|&&value| value == "extends")
                .count(),
            4
        );
        assert_eq!(
            relations
                .iter()
                .filter(|&&value| value == "implements")
                .count(),
            3
        );
        assert_eq!(
            relations.iter().filter(|&&value| value == "embeds").count(),
            1
        );
        assert_eq!(result.unresolved_relations, 0);
        assert!(
            result
                .symbols
                .iter()
                .any(|symbol| { symbol.qualified_name == "App\\Base" && symbol.fan_in == 1 })
        );
    }

    #[test]
    fn rejects_ambiguous_short_names_instead_of_inventing_an_edge() {
        let result = topology(&[
            (
                FirstClass::Php,
                "a/Base.php",
                "<?php namespace A; class Base {}",
            ),
            (
                FirstClass::Php,
                "b/Base.php",
                "<?php namespace B; class Base {}",
            ),
            (
                FirstClass::Php,
                "c/Child.php",
                "<?php class Child extends Base {}",
            ),
        ]);

        assert!(result.edges.is_empty());
        assert_eq!(result.unresolved_relations, 1);
    }
}
