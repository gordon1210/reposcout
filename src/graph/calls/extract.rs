use super::{
    MAX_DECLARATIONS_PER_FILE, MAX_IMPORTS_PER_FILE, MAX_NODES_PER_FILE, MAX_RELATIONS_PER_FILE,
    javascript, language_name, rust,
};
use crate::lang::FirstClass;
use crate::model::{
    CallDeclaration, CallFactCoverage, CallImportBinding, CallReferenceFact, CallReferenceFacts,
    CallReferenceKind, CallReferenceStatus, CallReferenceSyntax, CallSymbolIdentity,
    CallTargetCandidate, DefinitionFact, DefinitionFacts, DefinitionStatus, SourceSpan,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tree_sitter::Node;

#[derive(Debug)]
pub(super) struct ObservedRelation {
    pub kind: CallReferenceKind,
    pub site: SourceSpan,
    pub form: ObservedForm,
}

#[derive(Debug)]
pub(super) enum ObservedForm {
    Bare(String),
    Qualified {
        qualifier: String,
        member: String,
        inline_modules: Vec<String>,
    },
    Member {
        object: String,
        member: String,
        optional: bool,
    },
    Receiver {
        object: String,
        member: String,
    },
    Dynamic(String),
}

#[derive(Debug)]
pub(super) struct ShadowBinding {
    pub name: String,
    pub scope: SourceSpan,
    pub active_from: usize,
}

pub(super) struct State<'a> {
    language: FirstClass,
    path: &'a str,
    source_hash: &'a str,
    content: &'a str,
    root: Node<'a>,
    definitions: &'a DefinitionFacts,
    imports: Vec<CallImportBinding>,
    observed: Vec<ObservedRelation>,
    shadows: HashMap<String, Vec<ShadowBinding>>,
    declaration_scopes: HashMap<(usize, usize), SourceSpan>,
    hoisted_declarations: BTreeSet<(usize, usize)>,
    direct_exports: HashMap<(usize, usize), Vec<String>>,
    export_aliases: BTreeMap<String, Vec<String>>,
    overloaded_names: BTreeSet<String>,
    coverage: CallFactCoverage,
    nodes_truncated: bool,
    facts_truncated: bool,
}

impl<'a> State<'a> {
    fn new(
        language: FirstClass,
        path: &'a str,
        source_hash: &'a str,
        content: &'a str,
        root: Node<'a>,
        definitions: &'a DefinitionFacts,
    ) -> Self {
        Self {
            language,
            path,
            source_hash,
            content,
            root,
            definitions,
            imports: Vec::new(),
            observed: Vec::new(),
            shadows: HashMap::new(),
            declaration_scopes: HashMap::new(),
            hoisted_declarations: BTreeSet::new(),
            direct_exports: HashMap::new(),
            export_aliases: BTreeMap::new(),
            overloaded_names: BTreeSet::new(),
            coverage: CallFactCoverage::default(),
            nodes_truncated: false,
            facts_truncated: false,
        }
    }

    pub(super) fn content(&self) -> &str {
        self.content
    }

    pub(super) fn root(&self) -> Node<'a> {
        self.root
    }

    pub(super) fn definitions(&self) -> &DefinitionFacts {
        self.definitions
    }

    pub(super) fn push_import(&mut self, import: CallImportBinding) {
        self.coverage.observed_imports = self.coverage.observed_imports.saturating_add(1);
        if self.imports.len() < MAX_IMPORTS_PER_FILE {
            self.imports.push(import);
            self.coverage.retained_imports = self.coverage.retained_imports.saturating_add(1);
        } else {
            self.coverage.omitted_imports = self.coverage.omitted_imports.saturating_add(1);
            self.facts_truncated = true;
        }
    }

    pub(super) fn push_relation(&mut self, relation: ObservedRelation) {
        self.coverage.observed_relations = self.coverage.observed_relations.saturating_add(1);
        if self.observed.len() < MAX_RELATIONS_PER_FILE {
            self.observed.push(relation);
            self.coverage.retained_relations = self.coverage.retained_relations.saturating_add(1);
        } else {
            self.coverage.omitted_relations = self.coverage.omitted_relations.saturating_add(1);
            self.facts_truncated = true;
        }
    }

    pub(super) fn push_shadow(&mut self, binding: ShadowBinding) {
        self.shadows
            .entry(binding.name.clone())
            .or_default()
            .push(binding);
    }

    pub(super) fn mark_unsupported(&mut self) {
        self.coverage.unsupported_syntax = self.coverage.unsupported_syntax.saturating_add(1);
    }

    pub(super) fn mark_work_truncated(&mut self, omitted_nodes: usize) {
        self.nodes_truncated = true;
        self.coverage.omitted_nodes = self
            .coverage
            .omitted_nodes
            .saturating_add(omitted_nodes.max(1));
    }

    pub(super) fn set_declaration_scope(&mut self, key: (usize, usize), scope: SourceSpan) {
        self.declaration_scopes.insert(key, scope);
    }

    pub(super) fn mark_declaration_hoisted(&mut self, key: (usize, usize)) {
        self.hoisted_declarations.insert(key);
    }

    pub(super) fn add_direct_export(&mut self, key: (usize, usize), name: String) {
        self.direct_exports.entry(key).or_default().push(name);
    }

    pub(super) fn add_export_alias(&mut self, local: String, exported: String) {
        self.export_aliases.entry(local).or_default().push(exported);
    }

    pub(super) fn mark_overloaded(&mut self, name: String) {
        self.overloaded_names.insert(name);
    }

    fn finish(mut self) -> CallReferenceFacts {
        self.imports.sort_by(|left, right| {
            left.span
                .start_byte
                .cmp(&right.span.start_byte)
                .then_with(|| left.local_name.cmp(&right.local_name))
                .then_with(|| left.module_specifier.cmp(&right.module_specifier))
        });
        for bindings in self.shadows.values_mut() {
            bindings.sort_by(|left, right| {
                left.scope
                    .start_byte
                    .cmp(&right.scope.start_byte)
                    .then_with(|| left.active_from.cmp(&right.active_from))
            });
        }
        let declarations = self.finish_declarations();
        let relations = self.finish_relations(&declarations);
        let status = if self.nodes_truncated {
            CallReferenceStatus::WorkTruncated
        } else if self.facts_truncated {
            CallReferenceStatus::FactTruncated
        } else if self.root.has_error() || self.definitions.status == DefinitionStatus::ParseErrors
        {
            CallReferenceStatus::ParseErrors
        } else {
            CallReferenceStatus::Available
        };
        CallReferenceFacts {
            status,
            language: language_name(self.language).to_string(),
            source_path: self.path.to_string(),
            source_hash: self.source_hash.to_string(),
            declarations,
            imports: self.imports,
            relations,
            coverage: self.coverage,
        }
    }

    fn finish_declarations(&mut self) -> Vec<CallDeclaration> {
        self.coverage.observed_declarations = self.definitions.definitions.len();
        let retained = self
            .definitions
            .definitions
            .len()
            .min(MAX_DECLARATIONS_PER_FILE);
        self.coverage.retained_declarations = retained;
        self.coverage.omitted_declarations =
            self.definitions.definitions.len().saturating_sub(retained);
        if self.coverage.omitted_declarations > 0 {
            self.facts_truncated = true;
        }
        let file_scope = span(self.root);
        self.definitions
            .definitions
            .iter()
            .take(retained)
            .map(|definition| {
                let key = declaration_key(definition);
                let mut export_names = self.direct_exports.get(&key).cloned().unwrap_or_default();
                let simple_name = simple_name(&definition.symbol.name);
                if definition.symbol.exported
                    && self
                        .declaration_scopes
                        .get(&key)
                        .copied()
                        .unwrap_or(file_scope)
                        == file_scope
                    && export_names.is_empty()
                    && !self.export_aliases.contains_key(simple_name)
                {
                    export_names.push(simple_name.to_string());
                }
                if let Some(aliases) = self.export_aliases.get(simple_name) {
                    export_names.extend(aliases.iter().cloned());
                }
                export_names.sort();
                export_names.dedup();
                CallDeclaration {
                    symbol: identity(self.path, self.source_hash, definition),
                    export_names,
                    scope_span: self
                        .declaration_scopes
                        .get(&key)
                        .copied()
                        .unwrap_or(file_scope),
                    overloaded: self.overloaded_names.contains(simple_name),
                }
            })
            .collect()
    }

    fn finish_relations(&self, declarations: &[CallDeclaration]) -> Vec<CallReferenceFact> {
        let mut relations = self
            .observed
            .iter()
            .map(|observed| {
                let (syntax, mut candidate, provenance) = classify(
                    observed,
                    &self.imports,
                    declarations,
                    &self.hoisted_declarations,
                    self.language,
                );
                candidate.shadowed |= self.is_shadowed(&candidate.root, observed.site);
                if matches!(
                    syntax,
                    CallReferenceSyntax::ImportedBinding | CallReferenceSyntax::ModuleQualified
                ) {
                    candidate.shadowed |=
                        declaration_shadows(declarations, &candidate.root, observed.site);
                }
                if matches!(
                    syntax,
                    CallReferenceSyntax::Receiver | CallReferenceSyntax::Dynamic
                ) {
                    // Counted here because classification depends on the complete import table.
                    // Extraction remains useful: the relation itself is retained as unresolved evidence.
                }
                CallReferenceFact {
                    kind: observed.kind,
                    syntax,
                    site: observed.site,
                    owner: owner(declarations, observed.site),
                    candidate,
                    provenance: provenance.to_string(),
                }
            })
            .collect::<Vec<_>>();
        relations.sort_by(|left, right| {
            left.site
                .start_byte
                .cmp(&right.site.start_byte)
                .then_with(|| left.site.end_byte.cmp(&right.site.end_byte))
                .then_with(|| left.kind.cmp(&right.kind))
        });
        relations
    }

    fn is_shadowed(&self, name: &str, site: SourceSpan) -> bool {
        self.shadows.get(name).is_some_and(|bindings| {
            bindings.iter().any(|binding| {
                contains(binding.scope, site)
                    && binding.active_from <= site.start_byte
                    && !self.imports.iter().any(|import| {
                        import.local_name == name
                            && import.span.start_byte == binding.active_from
                            && contains(import.scope_span, site)
                    })
            })
        })
    }
}

pub(super) fn extract(
    language: FirstClass,
    path: &str,
    source_hash: &str,
    content: &str,
    root: Node<'_>,
    definitions: &DefinitionFacts,
) -> CallReferenceFacts {
    if !matches!(
        language,
        FirstClass::Rust | FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx
    ) {
        return CallReferenceFacts {
            status: CallReferenceStatus::Unsupported,
            language: language_name(language).to_string(),
            source_path: path.to_string(),
            source_hash: source_hash.to_string(),
            ..CallReferenceFacts::default()
        };
    }
    let mut state = State::new(language, path, source_hash, content, root, definitions);
    state.coverage.parse_errors =
        usize::from(root.has_error() || definitions.status == DefinitionStatus::ParseErrors);
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if state.coverage.visited_nodes >= MAX_NODES_PER_FILE {
            let omitted_nodes = node.descendant_count().saturating_add(
                stack
                    .iter()
                    .map(Node::descendant_count)
                    .fold(0usize, usize::saturating_add),
            );
            state.mark_work_truncated(omitted_nodes);
            break;
        }
        state.coverage.visited_nodes = state.coverage.visited_nodes.saturating_add(1);
        match language {
            FirstClass::Rust => rust::visit(node, &mut state),
            FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx => {
                javascript::visit(node, &mut state);
            }
            _ => {}
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(crate::numeric::usize_to_u32(index)) {
                stack.push(child);
            }
        }
    }
    let unsupported = state
        .observed
        .iter()
        .filter(|relation| {
            matches!(
                relation.form,
                ObservedForm::Receiver { .. } | ObservedForm::Dynamic(_)
            )
        })
        .count();
    state.coverage.unsupported_syntax = state
        .coverage
        .unsupported_syntax
        .saturating_add(unsupported);
    state.finish()
}

pub(super) fn span(node: Node<'_>) -> SourceSpan {
    SourceSpan {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: node.start_position().row + 1,
        end_line: (node.end_position().row + usize::from(node.end_position().column != 0))
            .max(node.start_position().row + 1),
    }
}

pub(super) fn source_text<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    node.utf8_text(content.as_bytes()).ok()
}

pub(super) fn enclosing_scope(node: Node<'_>) -> SourceSpan {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(
            parent.kind(),
            "block"
                | "statement_block"
                | "class_body"
                | "declaration_list"
                | "program"
                | "source_file"
        ) {
            return span(parent);
        }
        current = parent.parent();
    }
    span(node)
}

pub(super) fn enclosing_inline_rust_modules(node: Node<'_>, content: &str) -> Vec<String> {
    let mut modules = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "mod_item"
            && parent.child_by_field_name("body").is_some()
            && let Some(name) = parent
                .child_by_field_name("name")
                .and_then(|name| source_text(name, content))
        {
            modules.push(name.to_string());
        }
        current = parent.parent();
    }
    modules.reverse();
    modules
}

pub(super) fn declaration_key(definition: &DefinitionFact) -> (usize, usize) {
    (
        definition.declaration_span.start_byte,
        definition.declaration_span.end_byte,
    )
}

pub(super) fn simple_name(name: &str) -> &str {
    name.rsplit(['.', ':'])
        .find(|part| !part.is_empty())
        .unwrap_or(name)
}

pub(super) fn contains(outer: SourceSpan, inner: SourceSpan) -> bool {
    outer.start_byte <= inner.start_byte && outer.end_byte >= inner.end_byte
}

pub(super) fn is_call_target(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "call_expression"
            && parent
                .child_by_field_name("function")
                .is_some_and(|function| function.id() == node.id())
    })
}

pub(super) fn is_inside_handled_target(mut node: Node<'_>) -> bool {
    let original_start = node.start_byte();
    let original_end = node.end_byte();
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "member_expression" | "subscript_expression" | "field_expression" | "scoped_identifier"
        ) {
            return true;
        }
        if parent.kind() == "call_expression"
            && parent
                .child_by_field_name("function")
                .is_some_and(|function| {
                    function.start_byte() <= original_start && function.end_byte() >= original_end
                })
        {
            return true;
        }
        if matches!(
            parent.kind(),
            "arguments"
                | "binary_expression"
                | "unary_expression"
                | "return_expression"
                | "return_statement"
                | "expression_statement"
        ) {
            return false;
        }
        node = parent;
    }
    false
}

pub(super) fn is_declaration_name(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent
            .child_by_field_name("name")
            .is_some_and(|name| name.id() == node.id())
            && matches!(
                parent.kind(),
                "function_item"
                    | "function_declaration"
                    | "generator_function_declaration"
                    | "class_declaration"
                    | "method_definition"
                    | "variable_declarator"
            )
    })
}

pub(super) fn match_definition(
    node: Node<'_>,
    definitions: &DefinitionFacts,
) -> Option<(usize, usize)> {
    definitions
        .definitions
        .iter()
        .filter(|definition| {
            definition.declaration_span.start_byte <= node.start_byte()
                && definition.declaration_span.end_byte == node.end_byte()
        })
        .min_by_key(|definition| {
            node.start_byte()
                .saturating_sub(definition.declaration_span.start_byte)
        })
        .map(declaration_key)
}

fn identity(path: &str, source_hash: &str, definition: &DefinitionFact) -> CallSymbolIdentity {
    CallSymbolIdentity {
        path: path.to_string(),
        source_hash: source_hash.to_string(),
        name: definition.symbol.name.clone(),
        kind: definition.symbol.kind.clone(),
        declaration_span: definition.declaration_span,
    }
}

fn owner(declarations: &[CallDeclaration], site: SourceSpan) -> Option<CallSymbolIdentity> {
    declarations
        .iter()
        .filter(|declaration| contains(declaration.symbol.declaration_span, site))
        .min_by_key(|declaration| {
            declaration
                .symbol
                .declaration_span
                .end_byte
                .saturating_sub(declaration.symbol.declaration_span.start_byte)
        })
        .map(|declaration| declaration.symbol.clone())
}

fn declaration_shadows(declarations: &[CallDeclaration], name: &str, site: SourceSpan) -> bool {
    declarations.iter().any(|declaration| {
        simple_name(&declaration.symbol.name) == name && contains(declaration.scope_span, site)
    })
}

fn import_in_scope<'a>(
    imports: &'a [CallImportBinding],
    local: &str,
    site: SourceSpan,
) -> Vec<&'a CallImportBinding> {
    imports
        .iter()
        .filter(|import| import.local_name == local && contains(import.scope_span, site))
        .collect()
}

fn classify(
    observed: &ObservedRelation,
    imports: &[CallImportBinding],
    declarations: &[CallDeclaration],
    hoisted: &BTreeSet<(usize, usize)>,
    language: FirstClass,
) -> (CallReferenceSyntax, CallTargetCandidate, &'static str) {
    match &observed.form {
        ObservedForm::Bare(name) => classify_bare(
            name,
            observed.site,
            observed.kind,
            imports,
            declarations,
            hoisted,
        ),
        ObservedForm::Qualified {
            qualifier,
            member,
            inline_modules,
        } => (
            CallReferenceSyntax::ModuleQualified,
            CallTargetCandidate {
                root: qualifier
                    .split("::")
                    .next()
                    .unwrap_or(qualifier)
                    .to_string(),
                member: Some(member.clone()),
                qualifier: Some(qualifier.clone()),
                inline_modules: inline_modules.clone(),
                ..CallTargetCandidate::default()
            },
            "qualified-path",
        ),
        ObservedForm::Member {
            object,
            member,
            optional,
        } => classify_member(object, member, *optional, observed.site, imports),
        ObservedForm::Receiver { object, member } => (
            CallReferenceSyntax::Receiver,
            CallTargetCandidate {
                root: object.clone(),
                member: Some(member.clone()),
                ..CallTargetCandidate::default()
            },
            if language == FirstClass::Rust {
                "rust-receiver"
            } else {
                "receiver-member"
            },
        ),
        ObservedForm::Dynamic(kind) => (
            CallReferenceSyntax::Dynamic,
            CallTargetCandidate {
                root: kind.clone(),
                ..CallTargetCandidate::default()
            },
            "dynamic-expression",
        ),
    }
}

pub(super) fn target_kind_matches(declaration: &CallDeclaration, kind: CallReferenceKind) -> bool {
    declaration.symbol.kind == "function"
        || kind == CallReferenceKind::Reference
            && matches!(
                declaration.symbol.kind.as_str(),
                "class" | "enum" | "type" | "interface" | "trait" | "constant"
            )
}

fn classify_bare(
    name: &str,
    site: SourceSpan,
    kind: CallReferenceKind,
    imports: &[CallImportBinding],
    declarations: &[CallDeclaration],
    hoisted: &BTreeSet<(usize, usize)>,
) -> (CallReferenceSyntax, CallTargetCandidate, &'static str) {
    let mut local = declarations
        .iter()
        .filter(|declaration| {
            simple_name(&declaration.symbol.name) == name && contains(declaration.scope_span, site)
        })
        .collect::<Vec<_>>();
    if let Some(narrowest) = local
        .iter()
        .map(|declaration| declaration.scope_span.end_byte - declaration.scope_span.start_byte)
        .min()
    {
        local.retain(|declaration| {
            declaration.scope_span.end_byte - declaration.scope_span.start_byte == narrowest
        });
    }
    let active_target = local.iter().any(|declaration| {
        target_kind_matches(declaration, kind)
            && (hoisted.contains(&(
                declaration.symbol.declaration_span.start_byte,
                declaration.symbol.declaration_span.end_byte,
            )) || declaration.symbol.declaration_span.end_byte <= site.start_byte)
    });
    let uses_import = local.is_empty() && !import_in_scope(imports, name, site).is_empty();
    (
        if uses_import {
            CallReferenceSyntax::ImportedBinding
        } else {
            CallReferenceSyntax::LocalDirect
        },
        CallTargetCandidate {
            root: name.to_string(),
            shadowed: !local.is_empty() && !active_target,
            ..CallTargetCandidate::default()
        },
        if uses_import {
            "import-binding"
        } else {
            "bare-identifier"
        },
    )
}

fn classify_member(
    object: &str,
    member: &str,
    optional: bool,
    site: SourceSpan,
    imports: &[CallImportBinding],
) -> (CallReferenceSyntax, CallTargetCandidate, &'static str) {
    let namespace = import_in_scope(imports, object, site)
        .iter()
        .any(|import| import.kind == crate::model::CallImportKind::Namespace);
    let (syntax, provenance) = if namespace && !optional {
        (CallReferenceSyntax::ModuleQualified, "namespace-member")
    } else if optional {
        (CallReferenceSyntax::Dynamic, "optional-member")
    } else {
        (CallReferenceSyntax::Receiver, "receiver-member")
    };
    (
        syntax,
        CallTargetCandidate {
            root: object.to_string(),
            member: Some(member.to_string()),
            ..CallTargetCandidate::default()
        },
        provenance,
    )
}
