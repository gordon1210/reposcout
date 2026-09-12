use crate::metrics::tokens::TokenCounter;
use crate::model::{
    DefinitionEnvironment, DefinitionFact, DefinitionFacts, DefinitionPlanningFact,
    DefinitionPlanningFacts,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use tree_sitter::Tree;

/// Maximum declarations assigned source-token costs and bounded environment facts in one file.
pub const MAX_COSTED_DEFINITIONS: usize = 4_096;
const MAX_COSTED_BYTES: usize = 32 * 1_024 * 1_024;
const MAX_ENVIRONMENT: usize = 8;
const MAX_SIGNATURE_NODES: usize = 4_096;

/// Derive source-token costs and bounded signature-environment evidence from supplied content, syntax and declaration facts without I/O.
#[must_use]
pub fn extract(
    language: &str,
    content: &str,
    tree: Option<&Tree>,
    definitions: &DefinitionFacts,
    counter: &TokenCounter,
) -> DefinitionPlanningFacts {
    let mut facts = DefinitionPlanningFacts {
        sha256: Sha256::digest(content.as_bytes()).iter().fold(
            String::with_capacity(64),
            |mut output, byte| {
                let _ = write!(output, "{byte:02x}");
                output
            },
        ),
        encoding: counter.name().to_string(),
        ..DefinitionPlanningFacts::default()
    };
    let mut remaining = MAX_COSTED_BYTES;
    let type_bindings = type_bindings(definitions);
    for (index, definition) in definitions.definitions.iter().enumerate() {
        let Some(span) = definition.source_span else {
            continue;
        };
        let Some(source) = content.get(span.start_byte..span.end_byte) else {
            continue;
        };
        if facts.definitions.len() >= MAX_COSTED_DEFINITIONS || source.len() > remaining {
            facts.omitted_definitions += 1;
            continue;
        }
        remaining -= source.len();
        let (environment, gaps) =
            signature_environment(language, content, tree, definition, &type_bindings);
        facts.definitions.push(DefinitionPlanningFact {
            definition: index,
            tokens: counter.count(source),
            environment,
            gaps,
        });
    }
    facts
}

fn type_bindings(definitions: &DefinitionFacts) -> BTreeMap<String, Vec<usize>> {
    let mut bindings = BTreeMap::<String, Vec<usize>>::new();
    for (index, definition) in definitions.definitions.iter().enumerate() {
        if matches!(
            definition.symbol.kind.as_str(),
            "type" | "enum" | "class" | "interface" | "trait"
        ) && definition.source_span.is_some()
        {
            bindings
                .entry(definition.symbol.name.clone())
                .or_default()
                .push(index);
        }
    }
    bindings
}

fn signature_environment(
    language: &str,
    content: &str,
    tree: Option<&Tree>,
    definition: &DefinitionFact,
    bindings: &BTreeMap<String, Vec<usize>>,
) -> (Vec<DefinitionEnvironment>, Vec<String>) {
    if !matches!(definition.symbol.kind.as_str(), "function" | "method") {
        return (
            Vec::new(),
            vec!["definition-environment-not-expanded".to_string()],
        );
    }
    if !matches!(language, "Rust" | "TypeScript" | "TSX") {
        return (
            Vec::new(),
            vec!["signature-environment-unsupported-language".to_string()],
        );
    }
    let span = definition.declaration_span;
    let Some(node) = tree.and_then(|tree| {
        tree.root_node()
            .descendant_for_byte_range(span.start_byte, span.end_byte)
    }) else {
        return (
            Vec::new(),
            vec!["signature-environment-unavailable".to_string()],
        );
    };
    let mut names = BTreeSet::new();
    let mut generic_names = BTreeSet::new();
    let mut stack = vec![(node, false)];
    let body = node.child_by_field_name("body");
    let mut gaps = vec!["body-dependencies-not-expanded".to_string()];
    let mut visited = 0;
    while let Some((current, generic)) = stack.pop() {
        if visited >= MAX_SIGNATURE_NODES {
            gaps.push("signature-environment-work-limit".to_string());
            names.clear();
            break;
        }
        visited += 1;
        if body.is_some_and(|body| current.id() == body.id()) {
            continue;
        }
        if matches!(
            current.kind(),
            "scoped_type_identifier" | "nested_type_identifier" | "type_query"
        ) {
            gaps.push("qualified-signature-type-not-expanded".to_string());
            continue;
        }
        let generic = generic || current.kind() == "type_parameters";
        if current.kind() == "type_identifier"
            && let Some(name) = content.get(current.byte_range())
        {
            if generic {
                generic_names.insert(name.to_string());
            } else {
                names.insert(name.to_string());
            }
        }
        for index in (0..current.child_count()).rev() {
            if let Some(child) = current.child(index) {
                stack.push((child, generic));
            }
        }
    }
    let mut environment = Vec::new();
    for name in names.difference(&generic_names) {
        if is_primitive(name) {
            continue;
        }
        let qualified = sibling_name(&definition.symbol.name, name);
        match bindings.get(&qualified).map(Vec::as_slice) {
            Some([index]) if environment.len() < MAX_ENVIRONMENT => {
                environment.push(DefinitionEnvironment {
                    definition: *index,
                    reason: format!("same-scope signature type {qualified}"),
                });
            }
            Some([_]) => gaps.push("signature-environment-limit".to_string()),
            Some(_) => gaps.push(format!("ambiguous-signature-type:{qualified}")),
            None => gaps.push(format!("unresolved-signature-type:{qualified}")),
        }
    }
    environment.sort_by_key(|item| item.definition);
    environment.dedup_by_key(|item| item.definition);
    gaps.sort();
    gaps.dedup();
    (environment, gaps)
}

fn sibling_name(owner: &str, name: &str) -> String {
    owner.rsplit_once("::").map_or_else(
        || {
            owner
                .rsplit_once('.')
                .map_or_else(|| name.to_string(), |(scope, _)| format!("{scope}.{name}"))
        },
        |(scope, _)| format!("{scope}::{name}"),
    )
}

fn is_primitive(name: &str) -> bool {
    matches!(
        name,
        "Self"
            | "str"
            | "bool"
            | "char"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "f32"
            | "f64"
            | "number"
            | "string"
            | "boolean"
            | "void"
            | "unknown"
            | "never"
            | "any"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::FirstClass;

    #[test]
    fn costs_and_environment_use_captured_definition_identity() {
        let counter = TokenCounter::new("o200k_base").unwrap();
        let source = "pub struct Request { pub value: usize }\npub fn execute(input: Request) -> usize { input.value }\n";
        let tree = crate::parse::parse(FirstClass::Rust, source).unwrap();
        let definitions =
            crate::metrics::symbols::analyze(FirstClass::Rust, source, &tree).definitions;
        let facts = extract("Rust", source, Some(&tree), &definitions, &counter);
        assert_eq!(facts.encoding, "o200k_base");
        assert_eq!(
            facts.sha256,
            Sha256::digest(source.as_bytes()).iter().fold(
                String::with_capacity(64),
                |mut output, byte| {
                    let _ = write!(output, "{byte:02x}");
                    output
                }
            )
        );
        let execute = definitions
            .definitions
            .iter()
            .position(|item| item.symbol.name == "execute")
            .unwrap();
        let request = definitions
            .definitions
            .iter()
            .position(|item| item.symbol.name == "Request")
            .unwrap();
        let selected = facts
            .definitions
            .iter()
            .find(|item| item.definition == execute)
            .unwrap();
        assert_eq!(selected.environment.len(), 1);
        assert_eq!(selected.environment[0].definition, request);
        let span = definitions.definitions[execute].source_span.unwrap();
        assert_eq!(
            selected.tokens,
            counter.count(&source[span.start_byte..span.end_byte])
        );
        assert!(
            selected
                .gaps
                .iter()
                .any(|gap| gap == "body-dependencies-not-expanded")
        );
    }

    #[test]
    fn qualified_generic_and_body_names_are_not_local_type_proof() {
        let counter = TokenCounter::new("o200k_base").unwrap();
        for (language, fc, source) in [
            (
                "Rust",
                FirstClass::Rust,
                "struct Request {}\nfn execute(input: foreign::Request) {}",
            ),
            (
                "Rust",
                FirstClass::Rust,
                "struct Request {}\nfn execute<Request>(input: Request) {}",
            ),
            (
                "Rust",
                FirstClass::Rust,
                "struct Request {}\nfn execute() { let r = Request {}; }",
            ),
            (
                "TypeScript",
                FirstClass::TypeScript,
                "interface Request {}\nfunction execute(input: API.Request) {}",
            ),
            (
                "TypeScript",
                FirstClass::TypeScript,
                "interface Request {}\nfunction execute<Request>(input: Request) {}",
            ),
        ] {
            let tree = crate::parse::parse(fc, source).unwrap();
            let definitions = crate::metrics::symbols::analyze(fc, source, &tree).definitions;
            let facts = extract(language, source, Some(&tree), &definitions, &counter);
            let execute = definitions
                .definitions
                .iter()
                .position(|item| item.symbol.name == "execute")
                .unwrap();
            let selected = facts
                .definitions
                .iter()
                .find(|item| item.definition == execute)
                .unwrap();
            assert!(
                selected.environment.is_empty(),
                "{source}: {:?}",
                selected.environment
            );
        }
    }

    #[test]
    fn unsupported_environment_does_not_remove_source_costs() {
        let source = "def execute(value):\n    return value\n";
        let counter = TokenCounter::new("o200k_base").unwrap();
        let tree = crate::parse::parse(FirstClass::Python, source).unwrap();
        let definitions =
            crate::metrics::symbols::analyze(FirstClass::Python, source, &tree).definitions;
        let facts = extract("Python", source, Some(&tree), &definitions, &counter);
        assert_eq!(facts.definitions.len(), 1);
        assert!(facts.definitions[0].tokens > 0);
        assert!(
            facts.definitions[0]
                .gaps
                .iter()
                .any(|gap| gap == "signature-environment-unsupported-language")
        );
    }
}
