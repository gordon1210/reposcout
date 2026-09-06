//! Godot resource relationships, scoped to the nearest project.godot.
//! Syntax facts are cacheable; project/UID reads use the shared snapshot budget.
use super::{
    BTreeMap, BTreeSet, ConfigAccess, FirstClass, HashSet, ImportResolution, Node, Path,
    SourceFacts, detect, join_graph_path, parse, path_in_scope, path_parent,
};
use crate::godot::{attribute, section_name, string, text};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct Facts {
    pub(super) class_name: Option<String>,
    pub(super) uid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Reference {
    Resource {
        path: Option<String>,
        uid: Option<String>,
    },
    Name(String),
    Dynamic,
}

pub(super) fn extract(fc: FirstClass, content: &str, root: Node<'_>) -> (Facts, Vec<Reference>) {
    let mut facts = Facts::default();
    let mut references = Vec::new();
    let mut shadowed = HashSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match fc {
            FirstClass::GdScript => {
                if node.kind() == "class_name_statement" {
                    facts.class_name = node
                        .child_by_field_name("name")
                        .map(|name| text(name, content).to_string());
                }
                collect_shadowed(node, content, &mut shadowed);
                script_reference(node, content, &mut references);
            }
            FirstClass::GdShader if node.kind() == "preproc_include" => {
                push_path(node.child_by_field_name("path"), content, &mut references);
            }
            FirstClass::GodotResource if node.kind() == "section" => {
                let name = section_name(node, content);
                if matches!(name, "gd_scene" | "gd_resource") {
                    facts.uid = attribute(node, "uid", content);
                } else if name == "ext_resource" {
                    references.push(Reference::Resource {
                        path: attribute(node, "path", content),
                        uid: attribute(node, "uid", content),
                    });
                }
            }
            FirstClass::GodotResource if node.kind() == "property" => {
                property_reference(node, content, &mut references);
            }
            _ => {}
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(crate::numeric::usize_to_u32(index)) {
                stack.push(child);
            }
        }
    }
    // A local binding wins over an autoload/global name. Conservatively suppress
    // it file-wide instead of inventing scope-sensitive type inference.
    references
        .retain(|reference| !matches!(reference, Reference::Name(name) if shadowed.contains(name)));
    (facts, references)
}

fn collect_shadowed(node: Node<'_>, content: &str, names: &mut HashSet<String>) {
    if matches!(
        node.kind(),
        "variable_statement" | "const_statement" | "class_definition" | "function_definition"
    ) {
        if let Some(name) = node.child_by_field_name("name") {
            names.insert(text(name, content).to_string());
        }
    } else if node.kind() == "parameters" {
        let mut cursor = node.walk();
        for parameter in node.named_children(&mut cursor) {
            let name = parameter
                .child_by_field_name("name")
                .or_else(|| parameter.named_child(0))
                .unwrap_or(parameter);
            names.insert(text(name, content).to_string());
        }
    } else if node.kind() == "for_statement"
        && let Some(name) = node.child_by_field_name("left")
    {
        names.insert(text(name, content).to_string());
    }
}

fn script_reference(node: Node<'_>, content: &str, references: &mut Vec<Reference>) {
    match node.kind() {
        "extends_statement" => {
            if let Some(base) = node.named_child(0)
                && base.kind() == "string"
            {
                push_path(Some(base), content, references);
            }
        }
        "call" => {
            let callable = node
                .named_child(0)
                .map_or("", |callee| text(callee, content));
            if matches!(
                callable,
                "load" | "preload" | "ResourceLoader.load" | "ResourceLoader.load_threaded_request"
            ) {
                push_path(
                    node.child_by_field_name("arguments")
                        .and_then(|args| args.named_child(0)),
                    content,
                    references,
                );
            }
        }
        "attribute" => {
            if let Some(receiver) = node.named_child(0)
                && receiver.kind() == "identifier"
            {
                references.push(Reference::Name(text(receiver, content).to_string()));
            }
            // ResourceLoader.load is represented as an attribute_call by the grammar.
            if node
                .named_child(0)
                .is_some_and(|receiver| text(receiver, content) == "ResourceLoader")
            {
                let mut cursor = node.walk();
                for call in node
                    .named_children(&mut cursor)
                    .filter(|child| child.kind() == "attribute_call")
                {
                    if call.named_child(0).is_some_and(|name| {
                        matches!(text(name, content), "load" | "load_threaded_request")
                    }) {
                        push_path(
                            call.child_by_field_name("arguments")
                                .and_then(|args| args.named_child(0)),
                            content,
                            references,
                        );
                    }
                }
            }
        }
        "type" => {
            let mut stack = vec![node];
            while let Some(ty) = stack.pop() {
                if ty.kind() == "identifier" {
                    references.push(Reference::Name(text(ty, content).to_string()));
                }
                for index in 0..ty.named_child_count() {
                    if let Some(child) = ty.named_child(crate::numeric::usize_to_u32(index)) {
                        stack.push(child);
                    }
                }
            }
        }
        _ => {}
    }
}

fn push_path(node: Option<Node<'_>>, content: &str, references: &mut Vec<Reference>) {
    references.push(node.and_then(|node| string(node, content)).map_or(
        Reference::Dynamic,
        |path| Reference::Resource {
            path: Some(path),
            uid: None,
        },
    ));
}

fn property_reference(node: Node<'_>, content: &str, references: &mut Vec<Reference>) {
    let Some(parent) = node.parent().filter(|parent| parent.kind() == "section") else {
        return;
    };
    let section = section_name(parent, content);
    let key = node.named_child(0).map_or("", |key| text(key, content));
    let Some(value) = node.named_child(1) else {
        return;
    };
    if section == "autoload"
        || (section == "application" && key == "run/main_scene")
        || (section == "plugin" && key == "script")
    {
        let path =
            string(value, content).map(|path| path.strip_prefix('*').unwrap_or(&path).to_string());
        references.push(Reference::Resource { path, uid: None });
    } else if section == "editor_plugins" && key == "enabled" {
        let mut stack = vec![value];
        while let Some(node) = stack.pop() {
            if node.kind() == "string" {
                push_path(Some(node), content, references);
            }
            for index in 0..node.named_child_count() {
                if let Some(child) = node.named_child(crate::numeric::usize_to_u32(index)) {
                    stack.push(child);
                }
            }
        }
    }
}

#[derive(Default)]
pub(super) struct GodotResolver {
    projects: BTreeSet<String>,
    names: BTreeMap<(String, String), BTreeSet<String>>,
    uids: BTreeMap<(String, String), BTreeSet<String>>,
    pub(super) config_files: Vec<String>,
    pub(super) config_errors_by_path: BTreeMap<String, usize>,
}

impl GodotResolver {
    pub(super) fn discover(files: &[String], access: &mut ConfigAccess<'_>) -> Self {
        let mut resolver = Self::default();
        let known_projects = files
            .iter()
            .filter(|path| {
                Path::new(path)
                    .file_name()
                    .is_some_and(|name| name == "project.godot")
            })
            .collect::<HashSet<_>>();
        let mut candidates = BTreeSet::new();
        for path in files.iter().filter(|path| is_godot(path)) {
            let mut directory = path_parent(path);
            loop {
                candidates.insert(join_graph_path(&directory, "project.godot"));
                if directory.is_empty() {
                    break;
                }
                directory = path_parent(&directory);
            }
        }
        for config in candidates {
            if !known_projects.contains(&config) && !access.exists(&config) {
                continue;
            }
            let project = path_parent(&config);
            resolver.projects.insert(project.clone());
            resolver.config_files.push(config.clone());
            let Some(content) = access.read(&config) else {
                resolver.config_errors_by_path.insert(config, 1);
                continue;
            };
            let Some(tree) = parse::parse(FirstClass::GodotResource, &content) else {
                resolver.config_errors_by_path.insert(config, 1);
                continue;
            };
            let errors = super::source::count_parse_errors(tree.root_node());
            if errors > 0 {
                resolver.config_errors_by_path.insert(config, errors);
            }
            resolver.add_autoloads(&project, &content, tree.root_node());
        }
        for path in files.iter().filter(|path| has_uid_sidecar(path)) {
            let sidecar = format!("{path}.uid");
            if !access.exists(&sidecar) {
                continue;
            }
            resolver.config_files.push(sidecar.clone());
            match access.read(&sidecar) {
                Some(uid) if valid_uid(uid.trim()) => resolver.index_uid(path, uid.trim()),
                _ => {
                    resolver.config_errors_by_path.insert(sidecar, 1);
                }
            }
        }
        resolver
    }

    fn add_autoloads(&mut self, project: &str, content: &str, root: Node<'_>) {
        let mut cursor = root.walk();
        for section in root
            .named_children(&mut cursor)
            .filter(|node| node.kind() == "section" && section_name(*node, content) == "autoload")
        {
            let mut cursor = section.walk();
            for property in section
                .named_children(&mut cursor)
                .filter(|node| node.kind() == "property")
            {
                let Some(key) = property.named_child(0) else {
                    continue;
                };
                let Some(value) = property
                    .named_child(1)
                    .and_then(|value| string(value, content))
                else {
                    continue;
                };
                // Only autoloads with the singleton marker are globally named.
                let Some(path) = value
                    .strip_prefix('*')
                    .and_then(|path| path.strip_prefix("res://"))
                else {
                    continue;
                };
                let Some(target) = safe_path(project, path) else {
                    continue;
                };
                self.names
                    .entry((project.to_string(), text(key, content).to_string()))
                    .or_default()
                    .insert(target);
            }
        }
    }

    pub(super) fn add_source(&mut self, path: &str, facts: &SourceFacts) {
        if let Some(name) = &facts.godot.class_name
            && let Some(project) = self.project(path)
        {
            self.names
                .entry((project.to_string(), name.clone()))
                .or_default()
                .insert(path.to_string());
        }
        if let Some(uid) = &facts.godot.uid {
            self.index_uid(path, uid);
        }
    }

    fn index_uid(&mut self, path: &str, uid: &str) {
        if valid_uid(uid)
            && let Some(project) = self.project(path)
        {
            self.uids
                .entry((project.to_string(), uid.to_string()))
                .or_default()
                .insert(path.to_string());
        }
    }

    fn project(&self, path: &str) -> Option<&str> {
        self.projects
            .iter()
            .filter(|root| path_in_scope(path, root))
            .max_by_key(|root| root.len())
            .map(String::as_str)
    }

    pub(super) fn resolve(
        &self,
        importer: &str,
        reference: Reference,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        match reference {
            Reference::Dynamic => ImportResolution::Unresolved,
            Reference::Name(name) => {
                let Some(project) = self.project(importer) else {
                    return ImportResolution::External;
                };
                let Some(targets) = self.names.get(&(project.to_string(), name)) else {
                    return ImportResolution::External;
                };
                if targets.len() == 1 && targets.contains(importer) {
                    return ImportResolution::Local;
                }
                Self::unique_target(targets, "godot-global", nodes)
            }
            Reference::Resource { path, uid } => {
                self.resolve_resource(importer, path.as_deref(), uid.as_deref(), nodes)
            }
        }
    }

    fn resolve_resource(
        &self,
        importer: &str,
        path: Option<&str>,
        uid: Option<&str>,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        let uid = uid.or_else(|| path.filter(|path| path.starts_with("uid://")));
        if let Some(uid) = uid
            && let Some(project) = self.project(importer)
            && let Some(targets) = self.uids.get(&(project.to_string(), uid.to_string()))
        {
            return Self::unique_target(targets, "godot-uid", nodes);
        }
        let Some(path) = path.filter(|path| !path.starts_with("uid://")) else {
            return ImportResolution::Unresolved;
        };
        let project = self.project(importer);
        let candidate = if let Some(path) = path.strip_prefix("res://") {
            project.and_then(|root| safe_path(root, path))
        } else {
            safe_path(&path_parent(importer), path)
        };
        let Some(target) = candidate else {
            return ImportResolution::Unresolved;
        };
        if project != self.project(&target) {
            return ImportResolution::Unresolved;
        }
        Self::target(&target, "godot-resource", nodes)
    }

    fn unique_target(
        targets: &BTreeSet<String>,
        resolver: &'static str,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        if targets.len() != 1 {
            return ImportResolution::Unresolved;
        }
        targets
            .first()
            .map_or(ImportResolution::Unresolved, |target| {
                Self::target(target, resolver, nodes)
            })
    }

    fn target(target: &str, resolver: &'static str, nodes: &HashSet<String>) -> ImportResolution {
        if nodes.contains(target) {
            ImportResolution::Resolved {
                target: target.to_string(),
                resolver,
            }
        } else if detect(Path::new(target))
            .and_then(|info| info.first_class)
            .is_none()
        {
            // Binary assets and generic-language scripts are outside this graph's
            // analyzable universe. This does not validate the asset's existence.
            ImportResolution::NonGraph
        } else {
            ImportResolution::Unresolved
        }
    }
}

pub(super) fn is_godot(path: &str) -> bool {
    matches!(
        detect(Path::new(path)).and_then(|info| info.first_class),
        Some(FirstClass::GdScript | FirstClass::GdShader | FirstClass::GodotResource)
    )
}

pub(super) fn has_uid_sidecar(path: &str) -> bool {
    matches!(
        detect(Path::new(path)).and_then(|info| info.first_class),
        Some(FirstClass::GdScript | FirstClass::GdShader)
    )
}

fn valid_uid(uid: &str) -> bool {
    uid.strip_prefix("uid://").is_some_and(|id| {
        !id.is_empty()
            && id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

fn safe_path(base: &str, path: &str) -> Option<String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains([':', '\\'])
        || path.chars().any(char::is_control)
    {
        return None;
    }
    let mut parts = base
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests;
