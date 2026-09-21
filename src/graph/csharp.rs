use super::{BTreeMap, ImportResolution};
use crate::graph::source::CSharpFacts;
use std::collections::HashMap;
use tree_sitter::Node;

#[derive(Default)]
pub(super) struct CSharpResolver {
    // None records namespaces declared by more than one file.
    namespaces: BTreeMap<String, Option<String>>,
}

impl CSharpResolver {
    pub(super) fn add_source(&mut self, path: &str, facts: &CSharpFacts) {
        for namespace in &facts.namespaces {
            self.namespaces
                .entry(normalize(namespace))
                .and_modify(|candidate| {
                    if candidate.as_deref() != Some(path) {
                        *candidate = None;
                    }
                })
                .or_insert_with(|| Some(path.to_string()));
        }
    }

    pub(super) fn resolve(&self, importer: &str, namespace: &str) -> ImportResolution {
        let namespace = normalize(namespace);
        if namespace.is_empty() {
            return ImportResolution::NonGraph;
        }
        if let Some(candidate) = self.namespaces.get(&namespace) {
            return match candidate {
                Some(target) if target != importer => ImportResolution::Resolved {
                    target: target.clone(),
                    resolver: "csharp-namespace",
                },
                _ => ImportResolution::Local,
            };
        }
        if self.namespaces.keys().any(|local| {
            local
                .strip_prefix(&namespace)
                .is_some_and(|suffix| suffix.starts_with('.'))
                || namespace
                    .strip_prefix(local)
                    .is_some_and(|suffix| suffix.starts_with('.'))
        }) {
            ImportResolution::Local
        } else {
            ImportResolution::External
        }
    }
}

fn normalize(namespace: &str) -> String {
    namespace
        .trim()
        .trim_start_matches("global::")
        .replace("::", ".")
}

pub(super) fn file_namespace(root: Node<'_>, content: &str) -> String {
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .find(|node| node.kind() == "file_scoped_namespace_declaration")
        .and_then(|node| node.child_by_field_name("name"))
        .and_then(|name| name.utf8_text(content.as_bytes()).ok())
        .map(normalize)
        .unwrap_or_default()
}

pub(super) fn type_scope(node: Node<'_>, content: &str, file_namespace: &str) -> String {
    let mut parts = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(
            parent.kind(),
            "namespace_declaration"
                | "class_declaration"
                | "struct_declaration"
                | "record_declaration"
                | "interface_declaration"
        ) && let Some(name) = parent
            .child_by_field_name("name")
            .and_then(|name| name.utf8_text(content.as_bytes()).ok())
        {
            parts.push(normalize(name));
        }
        current = parent.parent();
    }
    if !file_namespace.is_empty() {
        parts.push(file_namespace.to_string());
    }
    parts.reverse();
    parts.join(".")
}

pub(super) fn scoped_aliases(node: Node<'_>, content: &str) -> HashMap<String, String> {
    let mut scopes = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "compilation_unit" {
            // The grammar keeps using directives after a file-scoped namespace
            // as compilation-unit siblings, not children of the namespace header.
            scopes.push(parent);
        } else if parent.kind() == "namespace_declaration"
            && let Some(body) = parent.child_by_field_name("body")
        {
            scopes.push(body);
        }
        current = parent.parent();
    }
    let mut aliases = HashMap::new();
    for scope in scopes.into_iter().rev() {
        let mut cursor = scope.walk();
        for directive in scope
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "using_directive")
        {
            let Some(alias) = directive.child_by_field_name("name") else {
                continue;
            };
            let mut cursor = directive.walk();
            let target = directive
                .named_children(&mut cursor)
                .find(|child| child.id() != alias.id() && child.kind() != "comment");
            if let (Ok(alias), Some(Ok(target))) = (
                alias.utf8_text(content.as_bytes()),
                target.map(|target| target.utf8_text(content.as_bytes())),
            ) {
                aliases.insert(normalize(alias), normalize(target));
            }
        }
    }
    aliases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csharp_namespaces_with_multiple_files_stay_ambiguous_including_the_importer() {
        let mut resolver = CSharpResolver::default();
        let facts = CSharpFacts {
            namespaces: vec!["App".into()],
        };
        resolver.add_source("a.cs", &facts);
        assert!(matches!(
            resolver.resolve("other.cs", "App"),
            ImportResolution::Resolved { .. }
        ));
        resolver.add_source("b.cs", &facts);
        for importer in ["a.cs", "b.cs", "other.cs"] {
            assert!(matches!(
                resolver.resolve(importer, "App"),
                ImportResolution::Local
            ));
        }
    }
}
