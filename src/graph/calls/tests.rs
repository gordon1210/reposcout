use super::{extract, module_requests, resolve};
use crate::lang::FirstClass;
use crate::metrics::symbols;
use crate::model::{
    CallModuleResolution, CallReferenceFacts, CallReferenceKind, CallReferenceStatus,
    CallReferenceSyntax, CallResolutionStatus, CallUnresolvedReason,
};
use crate::parse;
use std::collections::BTreeMap;

fn extracted(language: FirstClass, path: &str, source: &str) -> CallReferenceFacts {
    let tree = parse::parse(language, source).unwrap();
    let definitions = symbols::analyze(language, source, &tree).definitions;
    extract(
        language,
        path,
        &format!("hash:{path}"),
        source,
        tree.root_node(),
        &definitions,
    )
}

fn resolved_modules(
    facts: &[CallReferenceFacts],
    targets: &[(&str, &str)],
) -> Vec<CallModuleResolution> {
    let targets = targets.iter().copied().collect::<BTreeMap<_, _>>();
    module_requests(facts)
        .into_iter()
        .map(|request| {
            let target_path = targets
                .get(request.module_specifier.as_str())
                .map(ToString::to_string);
            CallModuleResolution {
                request,
                resolver: target_path.as_ref().map(|_| "fixture-module".to_string()),
                status: if target_path.is_some() {
                    CallResolutionStatus::Resolved
                } else {
                    CallResolutionStatus::Unresolved
                },
                target_path,
            }
        })
        .collect()
}

#[test]
fn rust_local_alias_qualified_calls_and_references_resolve_with_exact_sites() {
    let dependency = extracted(FirstClass::Rust, "src/dep.rs", "pub fn work() {}\n");
    let source = r"
use crate::dep::work as perform;
use crate::dep as dependency;
fn local() {}
fn run() {
    local();
    perform();
    dependency::work();
    crate::dep::work();
    let keep = perform;
}
";
    let consumer = extracted(FirstClass::Rust, "src/lib.rs", source);
    let facts = [dependency, consumer];
    let modules = resolved_modules(
        &facts,
        &[
            ("crate::dep::work", "src/dep.rs"),
            ("crate::dep", "src/dep.rs"),
        ],
    );
    let topology = resolve(&facts, &modules);

    let resolved_sites = topology
        .edges
        .iter()
        .map(|edge| {
            (
                &source[edge.site.start_byte..edge.site.end_byte],
                edge.kind,
                edge.syntax,
                edge.target.path.as_str(),
                edge.target.name.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert!(resolved_sites.contains(&(
        "local()",
        CallReferenceKind::Call,
        CallReferenceSyntax::LocalDirect,
        "src/lib.rs",
        "local",
    )));
    assert!(resolved_sites.contains(&(
        "perform()",
        CallReferenceKind::Call,
        CallReferenceSyntax::ImportedBinding,
        "src/dep.rs",
        "work",
    )));
    assert!(resolved_sites.contains(&(
        "dependency::work()",
        CallReferenceKind::Call,
        CallReferenceSyntax::ModuleQualified,
        "src/dep.rs",
        "work",
    )));
    assert!(resolved_sites.contains(&(
        "crate::dep::work()",
        CallReferenceKind::Call,
        CallReferenceSyntax::ModuleQualified,
        "src/dep.rs",
        "work",
    )));
    assert!(resolved_sites.contains(&(
        "perform",
        CallReferenceKind::Reference,
        CallReferenceSyntax::ImportedBinding,
        "src/dep.rs",
        "work",
    )));
    assert!(
        topology
            .edges
            .iter()
            .all(|edge| edge.source.source_hash == "hash:src/lib.rs")
    );
}

#[test]
fn javascript_named_default_namespace_and_local_bindings_resolve() {
    let dependency_source = "export function work() {}\nexport default function fallback() {}\n";
    let dependency = extracted(FirstClass::TypeScript, "dep.ts", dependency_source);
    let consumer_source = r"
import fallback, { work as perform } from './dep';
import * as api from './dep';
function local() {}
export function run() {
    local();
    perform();
    fallback();
    api.work();
    const keep = perform;
}
";
    let consumer = extracted(FirstClass::Tsx, "main.tsx", consumer_source);
    let facts = [dependency, consumer];
    let modules = resolved_modules(&facts, &[("./dep", "dep.ts")]);
    let topology = resolve(&facts, &modules);
    let reversed_facts = facts.iter().cloned().rev().collect::<Vec<_>>();
    let reversed_modules = modules.iter().cloned().rev().collect::<Vec<_>>();
    assert_eq!(topology, resolve(&reversed_facts, &reversed_modules));

    let by_site = topology
        .edges
        .iter()
        .map(|edge| {
            (
                &consumer_source[edge.site.start_byte..edge.site.end_byte],
                edge.target.name.as_str(),
                edge.resolver.as_str(),
            )
        })
        .collect::<Vec<_>>();
    assert!(by_site.iter().any(|(site, target, resolver)| {
        *site == "perform()" && *target == "work" && resolver.starts_with("js-named-import+")
    }));
    assert!(by_site.iter().any(|(site, target, resolver)| {
        *site == "fallback()" && *target == "fallback" && resolver.starts_with("js-default-import+")
    }));
    assert!(by_site.iter().any(|(site, target, resolver)| {
        *site == "api.work()" && *target == "work" && resolver.starts_with("js-namespace-import+")
    }));
    assert!(by_site.iter().any(|(site, target, resolver)| {
        *site == "local()" && *target == "local" && *resolver == "local-lexical"
    }));
    assert!(
        by_site
            .iter()
            .any(|(site, target, _)| { *site == "perform" && *target == "work" })
    );
}

#[test]
fn shadowing_receivers_dynamic_targets_and_name_collisions_stay_unresolved() {
    let dependency = extracted(
        FirstClass::TypeScript,
        "dep.ts",
        "export function work(value: string): string;\nexport function work(value: number): number;\nexport function work(value: unknown) { return value; }\n",
    );
    let consumer_source = r"
import { work } from './dep';
function run(work, client, key) {
    work();
    client.work();
    client[key]();
}
";
    let consumer = extracted(FirstClass::TypeScript, "main.ts", consumer_source);
    let unrelated = extracted(
        FirstClass::TypeScript,
        "unrelated.ts",
        "export function work() {}\n",
    );
    let facts = [dependency, consumer, unrelated];
    let modules = resolved_modules(&facts, &[("./dep", "dep.ts")]);
    let topology = resolve(&facts, &modules);

    assert!(topology.unresolved.iter().any(|fact| {
        fact.source_path == "main.ts"
            && fact.reason == CallUnresolvedReason::ShadowedBinding
            && &consumer_source[fact.site.start_byte..fact.site.end_byte] == "work()"
    }));
    assert!(topology.unresolved.iter().any(|fact| {
        fact.source_path == "main.ts"
            && fact.reason == CallUnresolvedReason::DynamicReceiver
            && &consumer_source[fact.site.start_byte..fact.site.end_byte] == "client.work()"
    }));
    assert!(topology.unresolved.iter().any(|fact| {
        fact.source_path == "main.ts"
            && fact.reason == CallUnresolvedReason::DynamicTarget
            && &consumer_source[fact.site.start_byte..fact.site.end_byte] == "client[key]()"
    }));
    assert!(
        !topology
            .edges
            .iter()
            .any(|edge| { edge.source.path == "main.ts" && edge.target.path == "unrelated.ts" })
    );
}

#[test]
fn local_declarations_and_tdz_shadow_imports_without_false_external_edges() {
    let dependency = extracted(
        FirstClass::TypeScript,
        "dep.ts",
        "export function work() {}\n",
    );
    let consumer_source = r"
import { work } from './dep';
import * as api from './dep';
export function run() {
    {
        function work() {}
        work();
    }
    {
        work();
        const work = () => {};
    }
    {
        api.work();
        class api {}
    }
}
";
    let consumer = extracted(FirstClass::TypeScript, "main.ts", consumer_source);
    let facts = [dependency, consumer];
    let modules = resolved_modules(&facts, &[("./dep", "dep.ts")]);
    let topology = resolve(&facts, &modules);

    assert!(topology.edges.iter().any(|edge| {
        edge.source.path == "main.ts"
            && edge.target.path == "main.ts"
            && edge.target.name.ends_with("work")
    }));
    assert_eq!(
        topology
            .edges
            .iter()
            .filter(|edge| edge.source.path == "main.ts" && edge.target.path == "dep.ts")
            .count(),
        0
    );
    assert!(topology.unresolved.iter().any(|fact| {
        fact.reason == CallUnresolvedReason::ShadowedBinding
            && &consumer_source[fact.site.start_byte..fact.site.end_byte] == "work()"
    }));
    assert!(topology.unresolved.iter().any(|fact| {
        fact.reason == CallUnresolvedReason::ShadowedBinding
            && &consumer_source[fact.site.start_byte..fact.site.end_byte] == "api.work()"
    }));
}

#[test]
fn overloads_and_unimported_same_names_never_use_global_unique_name_shortcuts() {
    let overloaded = extracted(
        FirstClass::TypeScript,
        "dep.ts",
        "export function work(value: string): string;\nexport function work(value: number): number;\nexport function work(value: unknown) { return String(value); }\n",
    );
    let imported_source =
        "import { work } from './dep';\nexport function run() { return work('x'); }\n";
    let imported = extracted(FirstClass::TypeScript, "main.ts", imported_source);
    let unrelated = extracted(FirstClass::Rust, "src/a.rs", "pub fn missing() {}\n");
    let rust_source = "fn run() { missing(); }\n";
    let rust_consumer = extracted(FirstClass::Rust, "src/lib.rs", rust_source);
    let facts = [overloaded, imported, unrelated, rust_consumer];
    let modules = resolved_modules(&facts, &[("./dep", "dep.ts")]);
    let topology = resolve(&facts, &modules);

    assert!(topology.unresolved.iter().any(|fact| {
        fact.reason == CallUnresolvedReason::AmbiguousTarget
            && fact.source_path == "main.ts"
            && &imported_source[fact.site.start_byte..fact.site.end_byte] == "work('x')"
    }));
    assert!(topology.unresolved.iter().any(|fact| {
        fact.reason == CallUnresolvedReason::MissingTarget
            && fact.source_path == "src/lib.rs"
            && &rust_source[fact.site.start_byte..fact.site.end_byte] == "missing()"
    }));
    assert!(
        !topology
            .edges
            .iter()
            .any(|edge| { edge.source.path == "src/lib.rs" && edge.target.path == "src/a.rs" })
    );
}

#[test]
fn unsupported_languages_and_parse_errors_remain_visible() {
    let python = extracted(
        FirstClass::Python,
        "main.py",
        "def run():\n    return work()\n",
    );
    assert_eq!(python.status, CallReferenceStatus::Unsupported);
    assert!(python.relations.is_empty());

    let broken_source = "fn run() { missing(); let broken = ; }\n";
    let broken = extracted(FirstClass::Rust, "src/lib.rs", broken_source);
    assert_eq!(broken.status, CallReferenceStatus::ParseErrors);
    let topology = resolve(&[broken], &[]);
    assert!(!topology.unresolved.is_empty());
    assert!(
        topology
            .unresolved
            .iter()
            .all(|fact| { fact.reason == CallUnresolvedReason::ParseErrors })
    );
}
