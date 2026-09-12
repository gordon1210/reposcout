use super::extract::target_kind_matches;
use crate::model::{
    CallDeclaration, CallImportBinding, CallImportKind, CallModuleRequest, CallModuleResolution,
    CallReferenceFact, CallReferenceFacts, CallReferenceKind, CallReferenceSyntax,
    CallReferenceTopology, CallResolutionStatus, CallSymbolIdentity, CallUnresolvedReason,
    ResolvedCallReference, SourceSpan, UnresolvedCallReference,
};
use std::collections::BTreeMap;

pub(super) fn resolve(
    facts: &[CallReferenceFacts],
    modules: &[CallModuleResolution],
) -> CallReferenceTopology {
    let declarations = declaration_index(facts);
    let module_index = module_index(modules);
    let mut topology = CallReferenceTopology::default();
    topology.coverage.omitted = facts
        .iter()
        .map(|file| file.coverage.omitted_relations)
        .fold(0usize, usize::saturating_add);

    let mut ordered = facts.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    for file in ordered {
        for relation in &file.relations {
            topology.coverage.examined = topology.coverage.examined.saturating_add(1);
            match resolve_relation(file, relation, &declarations, &module_index) {
                Ok((target, resolver)) => {
                    let Some(source) = relation.owner.clone() else {
                        push_unresolved(
                            &mut topology,
                            file,
                            relation,
                            CallUnresolvedReason::MissingOwner,
                        );
                        continue;
                    };
                    topology.edges.push(ResolvedCallReference {
                        source,
                        target,
                        site: relation.site,
                        kind: relation.kind,
                        syntax: relation.syntax,
                        resolver,
                    });
                    topology.coverage.resolved = topology.coverage.resolved.saturating_add(1);
                }
                Err(reason) => push_unresolved(&mut topology, file, relation, reason),
            }
        }
    }
    topology.edges.sort_by(|left, right| {
        left.source
            .path
            .cmp(&right.source.path)
            .then_with(|| left.site.start_byte.cmp(&right.site.start_byte))
            .then_with(|| left.site.end_byte.cmp(&right.site.end_byte))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.target.path.cmp(&right.target.path))
            .then_with(|| left.target.name.cmp(&right.target.name))
    });
    topology.unresolved.sort_by(|left, right| {
        left.source_path
            .cmp(&right.source_path)
            .then_with(|| left.site.start_byte.cmp(&right.site.start_byte))
            .then_with(|| left.site.end_byte.cmp(&right.site.end_byte))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.reason.cmp(&right.reason))
    });
    topology
}

fn resolve_relation(
    file: &CallReferenceFacts,
    relation: &CallReferenceFact,
    declarations: &BTreeMap<String, Vec<&CallDeclaration>>,
    modules: &BTreeMap<CallModuleRequest, Vec<&CallModuleResolution>>,
) -> Result<(CallSymbolIdentity, String), CallUnresolvedReason> {
    match file.status {
        crate::model::CallReferenceStatus::WorkTruncated => {
            return Err(CallUnresolvedReason::WorkLimit);
        }
        crate::model::CallReferenceStatus::FactTruncated => {
            return Err(CallUnresolvedReason::FactLimit);
        }
        crate::model::CallReferenceStatus::ParseErrors => {
            return Err(CallUnresolvedReason::ParseErrors);
        }
        crate::model::CallReferenceStatus::Unsupported => {
            return Err(CallUnresolvedReason::UnsupportedLanguage);
        }
        crate::model::CallReferenceStatus::Available
        | crate::model::CallReferenceStatus::Unavailable => {}
    }
    match relation.syntax {
        CallReferenceSyntax::Dynamic => return Err(CallUnresolvedReason::DynamicTarget),
        CallReferenceSyntax::Receiver => return Err(CallUnresolvedReason::DynamicReceiver),
        CallReferenceSyntax::LocalDirect
        | CallReferenceSyntax::ImportedBinding
        | CallReferenceSyntax::ModuleQualified => {}
    }
    if relation.candidate.shadowed {
        return Err(CallUnresolvedReason::ShadowedBinding);
    }
    if relation.owner.is_none() {
        return Err(CallUnresolvedReason::MissingOwner);
    }

    match relation.syntax {
        CallReferenceSyntax::LocalDirect => resolve_local(file, relation, declarations),
        CallReferenceSyntax::ImportedBinding => {
            resolve_imported(file, relation, declarations, modules)
        }
        CallReferenceSyntax::ModuleQualified => {
            resolve_qualified(file, relation, declarations, modules)
        }
        CallReferenceSyntax::Receiver | CallReferenceSyntax::Dynamic => {
            Err(CallUnresolvedReason::UnsupportedSyntax)
        }
    }
}

fn resolve_local(
    file: &CallReferenceFacts,
    relation: &CallReferenceFact,
    declarations: &BTreeMap<String, Vec<&CallDeclaration>>,
) -> Result<(CallSymbolIdentity, String), CallUnresolvedReason> {
    let Some(file_declarations) = declarations.get(&file.source_path) else {
        return Err(CallUnresolvedReason::MissingTarget);
    };
    let mut candidates = file_declarations
        .iter()
        .copied()
        .filter(|declaration| {
            target_kind_matches(declaration, relation.kind)
                && simple_name(&declaration.symbol.name) == relation.candidate.root
                && contains(declaration.scope_span, relation.site)
        })
        .collect::<Vec<_>>();
    if candidates.iter().any(|candidate| candidate.overloaded) {
        return Err(CallUnresolvedReason::AmbiguousTarget);
    }
    retain_narrowest_scope(&mut candidates);
    unique_target(&candidates).map(|target| (target, "local-lexical".to_string()))
}

fn resolve_imported(
    file: &CallReferenceFacts,
    relation: &CallReferenceFact,
    declarations: &BTreeMap<String, Vec<&CallDeclaration>>,
    modules: &BTreeMap<CallModuleRequest, Vec<&CallModuleResolution>>,
) -> Result<(CallSymbolIdentity, String), CallUnresolvedReason> {
    let mut imports = imports_in_scope(file, &relation.candidate.root, relation.site);
    retain_narrowest_import_scope(&mut imports);
    let import = unique_import(&imports)?;
    let target_path = resolved_module_target(file, import, modules)?;
    let target_name = relation
        .candidate
        .member
        .as_deref()
        .unwrap_or(import.imported_name.as_str());
    let target = resolve_target(
        declarations,
        &target_path,
        target_name,
        &file.source_path,
        relation.kind,
    )?;
    Ok((target, import_resolver(import, file, modules)?))
}

fn resolve_qualified(
    file: &CallReferenceFacts,
    relation: &CallReferenceFact,
    declarations: &BTreeMap<String, Vec<&CallDeclaration>>,
    modules: &BTreeMap<CallModuleRequest, Vec<&CallModuleResolution>>,
) -> Result<(CallSymbolIdentity, String), CallUnresolvedReason> {
    let mut namespace = imports_in_scope(file, &relation.candidate.root, relation.site)
        .into_iter()
        .filter(|import| import.kind == CallImportKind::Namespace || file.language == "Rust")
        .collect::<Vec<_>>();
    retain_narrowest_import_scope(&mut namespace);
    if namespace.len() > 1 {
        return Err(CallUnresolvedReason::AmbiguousTarget);
    }
    if let Some(import) = namespace.first() {
        let target_path = resolved_module_target(file, import, modules)?;
        let target_name = relation
            .candidate
            .member
            .as_deref()
            .unwrap_or(relation.candidate.root.as_str());
        let target = resolve_target(
            declarations,
            &target_path,
            target_name,
            &file.source_path,
            relation.kind,
        )?;
        return Ok((target, import_resolver(import, file, modules)?));
    }

    let Some(qualifier) = relation.candidate.qualifier.as_deref() else {
        return Err(CallUnresolvedReason::MissingImportResolution);
    };
    let request = CallModuleRequest {
        source_path: file.source_path.clone(),
        language: file.language.clone(),
        module_specifier: qualifier.to_string(),
        inline_modules: relation.candidate.inline_modules.clone(),
    };
    let resolution = unique_module_resolution(modules.get(&request))?;
    let target_path = module_target_path(&file.source_path, resolution)?;
    let target = resolve_target(
        declarations,
        &target_path,
        relation
            .candidate
            .member
            .as_deref()
            .unwrap_or(relation.candidate.root.as_str()),
        &file.source_path,
        relation.kind,
    )?;
    let module_resolver = resolution.resolver.as_deref().unwrap_or("local-module");
    Ok((target, format!("rust-qualified+{module_resolver}")))
}

fn imports_in_scope<'a>(
    file: &'a CallReferenceFacts,
    local_name: &str,
    site: SourceSpan,
) -> Vec<&'a CallImportBinding> {
    file.imports
        .iter()
        .filter(|import| import.local_name == local_name && contains(import.scope_span, site))
        .collect()
}

fn retain_narrowest_scope(candidates: &mut Vec<&CallDeclaration>) {
    let Some(shortest) = candidates
        .iter()
        .map(|candidate| span_len(candidate.scope_span))
        .min()
    else {
        return;
    };
    candidates.retain(|candidate| span_len(candidate.scope_span) == shortest);
}

fn retain_narrowest_import_scope(imports: &mut Vec<&CallImportBinding>) {
    let Some(shortest) = imports
        .iter()
        .map(|import| span_len(import.scope_span))
        .min()
    else {
        return;
    };
    imports.retain(|import| span_len(import.scope_span) == shortest);
}

fn unique_import<'a>(
    imports: &[&'a CallImportBinding],
) -> Result<&'a CallImportBinding, CallUnresolvedReason> {
    match imports {
        [] => Err(CallUnresolvedReason::MissingImportResolution),
        [import] => Ok(*import),
        _ => Err(CallUnresolvedReason::AmbiguousTarget),
    }
}

fn resolved_module_target(
    file: &CallReferenceFacts,
    import: &CallImportBinding,
    modules: &BTreeMap<CallModuleRequest, Vec<&CallModuleResolution>>,
) -> Result<String, CallUnresolvedReason> {
    let request = CallModuleRequest {
        source_path: file.source_path.clone(),
        language: file.language.clone(),
        module_specifier: import.module_specifier.clone(),
        inline_modules: import.inline_modules.clone(),
    };
    module_target_path(
        &file.source_path,
        unique_module_resolution(modules.get(&request))?,
    )
}

fn import_resolver(
    import: &CallImportBinding,
    file: &CallReferenceFacts,
    modules: &BTreeMap<CallModuleRequest, Vec<&CallModuleResolution>>,
) -> Result<String, CallUnresolvedReason> {
    let request = CallModuleRequest {
        source_path: file.source_path.clone(),
        language: file.language.clone(),
        module_specifier: import.module_specifier.clone(),
        inline_modules: import.inline_modules.clone(),
    };
    let resolution = unique_module_resolution(modules.get(&request))?;
    let binding = match (file.language.as_str(), import.kind) {
        ("JavaScript" | "TypeScript" | "TSX", CallImportKind::Named) => "js-named-import",
        ("JavaScript" | "TypeScript" | "TSX", CallImportKind::Default) => "js-default-import",
        ("JavaScript" | "TypeScript" | "TSX", CallImportKind::Namespace) => "js-namespace-import",
        ("Rust", CallImportKind::RustPath) if import.local_name != import.imported_name => {
            "rust-use-alias"
        }
        ("Rust", CallImportKind::RustPath) => "rust-use",
        _ => return Err(CallUnresolvedReason::UnsupportedSyntax),
    };
    let module_resolver = resolution.resolver.as_deref().unwrap_or("local-module");
    Ok(format!("{binding}+{module_resolver}"))
}

fn unique_module_resolution<'a>(
    resolutions: Option<&'a Vec<&'a CallModuleResolution>>,
) -> Result<&'a CallModuleResolution, CallUnresolvedReason> {
    let Some(resolutions) = resolutions else {
        return Err(CallUnresolvedReason::MissingImportResolution);
    };
    let mut outcomes = resolutions
        .iter()
        .filter(|resolution| {
            matches!(
                resolution.status,
                CallResolutionStatus::Resolved | CallResolutionStatus::Local
            )
        })
        .copied()
        .collect::<Vec<_>>();
    outcomes.sort_by(|left, right| {
        left.target_path
            .cmp(&right.target_path)
            .then_with(|| left.resolver.cmp(&right.resolver))
    });
    outcomes.dedup_by(|left, right| {
        left.target_path == right.target_path && left.resolver == right.resolver
    });
    match outcomes.as_slice() {
        [resolution] => Ok(*resolution),
        [] => Err(CallUnresolvedReason::MissingImportResolution),
        _ => Err(CallUnresolvedReason::AmbiguousTarget),
    }
}

fn module_target_path(
    source_path: &str,
    resolution: &CallModuleResolution,
) -> Result<String, CallUnresolvedReason> {
    match resolution.status {
        CallResolutionStatus::Resolved => resolution
            .target_path
            .clone()
            .ok_or(CallUnresolvedReason::MissingImportResolution),
        CallResolutionStatus::Local => Ok(source_path.to_string()),
        CallResolutionStatus::External | CallResolutionStatus::Unresolved => {
            Err(CallUnresolvedReason::MissingImportResolution)
        }
    }
}

fn resolve_target(
    declarations: &BTreeMap<String, Vec<&CallDeclaration>>,
    target_path: &str,
    target_name: &str,
    source_path: &str,
    kind: CallReferenceKind,
) -> Result<CallSymbolIdentity, CallUnresolvedReason> {
    let Some(file_declarations) = declarations.get(target_path) else {
        return Err(CallUnresolvedReason::MissingTarget);
    };
    let candidates = file_declarations
        .iter()
        .copied()
        .filter(|declaration| {
            target_kind_matches(declaration, kind)
                && if target_path == source_path {
                    simple_name(&declaration.symbol.name) == target_name
                } else {
                    declaration
                        .export_names
                        .iter()
                        .any(|name| name == target_name)
                }
        })
        .collect::<Vec<_>>();
    if candidates.iter().any(|candidate| candidate.overloaded) {
        return Err(CallUnresolvedReason::AmbiguousTarget);
    }
    unique_target(&candidates)
}

fn unique_target(
    candidates: &[&CallDeclaration],
) -> Result<CallSymbolIdentity, CallUnresolvedReason> {
    match candidates {
        [] => Err(CallUnresolvedReason::MissingTarget),
        [candidate] => Ok(candidate.symbol.clone()),
        _ => Err(CallUnresolvedReason::AmbiguousTarget),
    }
}

fn declaration_index(facts: &[CallReferenceFacts]) -> BTreeMap<String, Vec<&CallDeclaration>> {
    let mut declarations = BTreeMap::<String, Vec<&CallDeclaration>>::new();
    for file in facts {
        declarations
            .entry(file.source_path.clone())
            .or_default()
            .extend(&file.declarations);
    }
    for file_declarations in declarations.values_mut() {
        file_declarations.sort_by(|left, right| {
            left.symbol
                .declaration_span
                .start_byte
                .cmp(&right.symbol.declaration_span.start_byte)
                .then_with(|| left.symbol.name.cmp(&right.symbol.name))
        });
    }
    declarations
}

fn module_index(
    modules: &[CallModuleResolution],
) -> BTreeMap<CallModuleRequest, Vec<&CallModuleResolution>> {
    let mut index = BTreeMap::<CallModuleRequest, Vec<&CallModuleResolution>>::new();
    for module in modules {
        index
            .entry(module.request.clone())
            .or_default()
            .push(module);
    }
    index
}

fn push_unresolved(
    topology: &mut CallReferenceTopology,
    file: &CallReferenceFacts,
    relation: &CallReferenceFact,
    reason: CallUnresolvedReason,
) {
    topology.coverage.unresolved = topology.coverage.unresolved.saturating_add(1);
    if matches!(
        reason,
        CallUnresolvedReason::UnsupportedLanguage
            | CallUnresolvedReason::UnsupportedSyntax
            | CallUnresolvedReason::DynamicReceiver
            | CallUnresolvedReason::DynamicTarget
    ) {
        topology.coverage.unsupported = topology.coverage.unsupported.saturating_add(1);
    }
    topology.unresolved.push(UnresolvedCallReference {
        source_path: file.source_path.clone(),
        source_hash: file.source_hash.clone(),
        site: relation.site,
        kind: relation.kind,
        syntax: relation.syntax,
        candidate: relation.candidate.clone(),
        reason,
    });
}

fn contains(outer: SourceSpan, inner: SourceSpan) -> bool {
    outer.start_byte <= inner.start_byte && outer.end_byte >= inner.end_byte
}

fn span_len(span: SourceSpan) -> usize {
    span.end_byte.saturating_sub(span.start_byte)
}

fn simple_name(name: &str) -> &str {
    name.rsplit(['.', ':'])
        .find(|part| !part.is_empty())
        .unwrap_or(name)
}
