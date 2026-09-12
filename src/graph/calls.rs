mod extract;
mod javascript;
mod resolve;
mod rust;

#[cfg(test)]
mod tests;

use crate::lang::FirstClass;
use crate::model::{
    CallModuleRequest, CallModuleResolution, CallReferenceFacts, CallReferenceTopology,
    DefinitionFacts,
};
use std::collections::BTreeSet;
use tree_sitter::Node;

pub(crate) const MAX_NODES_PER_FILE: usize = 100_000;
pub(crate) const MAX_DECLARATIONS_PER_FILE: usize = 8_192;
pub(crate) const MAX_IMPORTS_PER_FILE: usize = 4_096;
pub(crate) const MAX_RELATIONS_PER_FILE: usize = 16_384;

/// Extract content-identified call and reference facts from supplied syntax and declaration facts without I/O.
#[must_use]
pub(crate) fn extract(
    language: FirstClass,
    path: &str,
    source_hash: &str,
    content: &str,
    root: Node<'_>,
    definitions: &DefinitionFacts,
) -> CallReferenceFacts {
    extract::extract(language, path, source_hash, content, root, definitions)
}

/// Collect deterministic module-resolution requests from captured import bindings.
#[must_use]
pub(crate) fn module_requests(facts: &[CallReferenceFacts]) -> Vec<CallModuleRequest> {
    let mut requests = BTreeSet::new();
    for file in facts {
        for import in &file.imports {
            requests.insert(CallModuleRequest {
                source_path: file.source_path.clone(),
                language: file.language.clone(),
                module_specifier: import.module_specifier.clone(),
                inline_modules: import.inline_modules.clone(),
            });
        }
        for relation in &file.relations {
            if let Some(qualifier) = &relation.candidate.qualifier {
                let qualified_through_import = file.imports.iter().any(|import| {
                    import.local_name == relation.candidate.root
                        && import.scope_span.start_byte <= relation.site.start_byte
                        && import.scope_span.end_byte >= relation.site.end_byte
                });
                if qualified_through_import {
                    continue;
                }
                requests.insert(CallModuleRequest {
                    source_path: file.source_path.clone(),
                    language: file.language.clone(),
                    module_specifier: qualifier.clone(),
                    inline_modules: relation.candidate.inline_modules.clone(),
                });
            }
        }
    }
    requests.into_iter().collect()
}

/// Bind captured call and reference sites using lexical scope and supplied module evidence, leaving ambiguous targets unresolved.
#[must_use]
pub(crate) fn resolve(
    facts: &[CallReferenceFacts],
    modules: &[CallModuleResolution],
) -> CallReferenceTopology {
    resolve::resolve(facts, modules)
}

pub(crate) fn language_name(language: FirstClass) -> &'static str {
    match language {
        FirstClass::Rust => "Rust",
        FirstClass::Python => "Python",
        FirstClass::JavaScript => "JavaScript",
        FirstClass::TypeScript => "TypeScript",
        FirstClass::Tsx => "TSX",
        FirstClass::Go => "Go",
        FirstClass::Php => "PHP",
        FirstClass::GdScript => "GDScript",
        FirstClass::GdShader => "Godot Shader",
        FirstClass::GodotResource => "Godot Resource",
    }
}
