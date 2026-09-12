use super::extract::{
    ObservedForm, ObservedRelation, ShadowBinding, State, enclosing_scope, is_call_target,
    is_declaration_name, is_inside_handled_target, match_definition, source_text, span,
};
use crate::model::{CallImportBinding, CallImportKind, CallReferenceKind};
use tree_sitter::Node;

pub(super) fn visit(node: Node<'_>, state: &mut State<'_>) {
    match node.kind() {
        "import_statement" => extract_import(node, state),
        "export_statement" => extract_export_aliases(node, state),
        "function_declaration"
        | "generator_function_declaration"
        | "class_declaration"
        | "method_definition"
        | "variable_declarator" => record_declaration_metadata(node, state),
        "function_signature" => record_overload(node, state),
        "formal_parameters" => record_parameters(node, state),
        "arrow_function" => record_arrow_parameter(node, state),
        "call_expression" => record_call(node, state),
        "member_expression" if !is_call_target(node) => record_member_reference(node, state),
        "subscript_expression" if !is_call_target(node) => {
            state.push_relation(ObservedRelation {
                kind: CallReferenceKind::Reference,
                site: span(node),
                form: ObservedForm::Dynamic("computed-member".to_string()),
            });
        }
        "identifier" => record_identifier(node, state),
        _ => {}
    }
}

fn record_overload(node: Node<'_>, state: &mut State<'_>) {
    if let Some(name) = node
        .child_by_field_name("name")
        .and_then(|name| source_text(name, state.content()))
    {
        state.mark_overloaded(name.to_string());
    }
}

fn extract_import(node: Node<'_>, state: &mut State<'_>) {
    let Some(module) = node
        .child_by_field_name("source")
        .and_then(|source| source_text(source, state.content()))
        .and_then(strip_quotes)
        .map(str::to_string)
    else {
        state.mark_unsupported();
        return;
    };
    let scope_span = span(state.root());
    let import_span = span(node);
    let mut cursor = node.walk();
    let Some(clause) = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "import_clause")
    else {
        return;
    };
    let mut cursor = clause.walk();
    for child in clause.named_children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                if let Some(local) = source_text(child, state.content()).map(str::to_string) {
                    state.push_import(CallImportBinding {
                        module_specifier: module.clone(),
                        imported_name: "default".to_string(),
                        local_name: local,
                        kind: CallImportKind::Default,
                        span: import_span,
                        scope_span,
                        inline_modules: Vec::new(),
                    });
                }
            }
            "namespace_import" => {
                if let Some(local) = child
                    .named_child(0)
                    .and_then(|value| source_text(value, state.content()).map(str::to_string))
                {
                    state.push_import(CallImportBinding {
                        module_specifier: module.clone(),
                        imported_name: "*".to_string(),
                        local_name: local,
                        kind: CallImportKind::Namespace,
                        span: import_span,
                        scope_span,
                        inline_modules: Vec::new(),
                    });
                }
            }
            "named_imports" => {
                let mut imports = child.walk();
                for specifier in child.named_children(&mut imports) {
                    if specifier.kind() != "import_specifier" {
                        continue;
                    }
                    let Some(name) = specifier
                        .child_by_field_name("name")
                        .and_then(|value| source_text(value, state.content()))
                        .map(str::to_string)
                    else {
                        state.mark_unsupported();
                        continue;
                    };
                    let local = specifier
                        .child_by_field_name("alias")
                        .and_then(|value| source_text(value, state.content()))
                        .map_or_else(|| name.clone(), str::to_string);
                    state.push_import(CallImportBinding {
                        module_specifier: module.clone(),
                        imported_name: strip_quotes(&name).unwrap_or(&name).to_string(),
                        local_name: local,
                        kind: CallImportKind::Named,
                        span: span(specifier),
                        scope_span,
                        inline_modules: Vec::new(),
                    });
                }
            }
            _ => state.mark_unsupported(),
        }
    }
}

fn extract_export_aliases(node: Node<'_>, state: &mut State<'_>) {
    if node.child_by_field_name("source").is_some() {
        state.mark_unsupported();
        return;
    }
    let mut stack = Vec::new();
    let mut cursor = node.walk();
    stack.extend(node.named_children(&mut cursor));
    while let Some(candidate) = stack.pop() {
        if candidate.kind() == "export_specifier" {
            let local = candidate
                .child_by_field_name("name")
                .or_else(|| candidate.named_child(0))
                .and_then(|value| source_text(value, state.content()));
            let exported = candidate
                .child_by_field_name("alias")
                .or_else(|| candidate.named_child(1))
                .and_then(|value| source_text(value, state.content()))
                .or(local);
            if let (Some(local), Some(exported)) = (local, exported) {
                state.add_export_alias(
                    strip_quotes(local).unwrap_or(local).to_string(),
                    strip_quotes(exported).unwrap_or(exported).to_string(),
                );
            }
        }
        let mut cursor = candidate.walk();
        stack.extend(candidate.named_children(&mut cursor));
    }
    if let Some(value) = node.child_by_field_name("value")
        && value.kind() == "identifier"
        && let Some(local) = source_text(value, state.content())
        && source_text(node, state.content())
            .is_some_and(|text| text.trim_start().starts_with("export default"))
    {
        state.add_export_alias(local.to_string(), "default".to_string());
    }
}

fn record_declaration_metadata(node: Node<'_>, state: &mut State<'_>) {
    if node.kind() == "variable_declarator" && !is_callable_variable(node) {
        record_variable_shadow(node, state);
        return;
    }
    let Some(key) = match_definition(node, state.definitions()) else {
        return;
    };
    let scope = if node.kind() == "variable_declarator" && is_var_declaration(node, state.content())
    {
        function_or_program_scope(node)
    } else {
        enclosing_scope(node)
    };
    state.set_declaration_scope(key, scope);
    if matches!(
        node.kind(),
        "function_declaration" | "generator_function_declaration"
    ) {
        state.mark_declaration_hoisted(key);
    }
    let mut parent = node.parent();
    while let Some(candidate) = parent {
        if candidate.kind() == "export_statement" {
            let is_default = source_text(candidate, state.content())
                .is_some_and(|text| text.trim_start().starts_with("export default"));
            let name = if is_default {
                "default".to_string()
            } else {
                node.child_by_field_name("name")
                    .or_else(|| {
                        (node.kind() == "variable_declarator")
                            .then(|| node.child_by_field_name("name"))
                            .flatten()
                    })
                    .and_then(|value| source_text(value, state.content()))
                    .unwrap_or_default()
                    .to_string()
            };
            if !name.is_empty() {
                state.add_direct_export(key, name);
            }
            break;
        }
        if matches!(candidate.kind(), "program" | "statement_block") {
            break;
        }
        parent = candidate.parent();
    }
}

fn record_parameters(node: Node<'_>, state: &mut State<'_>) {
    let scope = node
        .parent()
        .and_then(|parent| parent.child_by_field_name("body"))
        .map_or_else(|| enclosing_scope(node), span);
    for identifier in pattern_identifiers(node, state.content()) {
        state.push_shadow(ShadowBinding {
            name: identifier,
            scope,
            active_from: scope.start_byte,
        });
    }
}

fn record_arrow_parameter(node: Node<'_>, state: &mut State<'_>) {
    let Some(parameter) = node.child_by_field_name("parameter") else {
        return;
    };
    let Some(name) = source_text(parameter, state.content()) else {
        return;
    };
    let scope = node
        .child_by_field_name("body")
        .map_or_else(|| span(node), span);
    state.push_shadow(ShadowBinding {
        name: name.to_string(),
        scope,
        active_from: scope.start_byte,
    });
}

fn record_variable_shadow(node: Node<'_>, state: &mut State<'_>) {
    let Some(name) = node.child_by_field_name("name") else {
        return;
    };
    let scope = if is_var_declaration(node, state.content()) {
        function_or_program_scope(node)
    } else {
        enclosing_scope(node)
    };
    for identifier in pattern_identifiers(name, state.content()) {
        state.push_shadow(ShadowBinding {
            name: identifier,
            scope,
            active_from: scope.start_byte,
        });
    }
}

fn is_var_declaration(node: Node<'_>, content: &str) -> bool {
    node.parent()
        .and_then(|parent| source_text(parent, content))
        .is_some_and(|text| text.trim_start().starts_with("var "))
}

fn function_or_program_scope(node: Node<'_>) -> crate::model::SourceSpan {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(
            parent.kind(),
            "function_declaration"
                | "generator_function_declaration"
                | "function_expression"
                | "generator_function"
                | "arrow_function"
                | "method_definition"
        ) && let Some(body) = parent.child_by_field_name("body")
        {
            return span(body);
        }
        if parent.kind() == "program" {
            return span(parent);
        }
        current = parent.parent();
    }
    enclosing_scope(node)
}

fn record_call(node: Node<'_>, state: &mut State<'_>) {
    if node.child_by_field_name("optional_chain").is_some()
        || source_text(node, state.content()).is_some_and(|text| text.contains("?.("))
    {
        state.push_relation(ObservedRelation {
            kind: CallReferenceKind::Call,
            site: span(node),
            form: ObservedForm::Dynamic("optional-call".to_string()),
        });
        return;
    }
    let Some(function) = node.child_by_field_name("function") else {
        state.mark_unsupported();
        return;
    };
    state.push_relation(ObservedRelation {
        kind: CallReferenceKind::Call,
        site: span(node),
        form: expression_form(function, state.content()),
    });
}

fn record_member_reference(node: Node<'_>, state: &mut State<'_>) {
    state.push_relation(ObservedRelation {
        kind: CallReferenceKind::Reference,
        site: span(node),
        form: expression_form(node, state.content()),
    });
}

fn record_identifier(node: Node<'_>, state: &mut State<'_>) {
    if is_declaration_name(node)
        || is_inside_handled_target(node)
        || is_non_reference_identifier(node)
    {
        return;
    }
    let Some(name) = source_text(node, state.content()) else {
        return;
    };
    state.push_relation(ObservedRelation {
        kind: CallReferenceKind::Reference,
        site: span(node),
        form: ObservedForm::Bare(name.to_string()),
    });
}

fn expression_form(node: Node<'_>, content: &str) -> ObservedForm {
    match node.kind() {
        "identifier" => source_text(node, content).map_or_else(
            || ObservedForm::Dynamic("invalid-identifier".to_string()),
            |name| ObservedForm::Bare(name.to_string()),
        ),
        "member_expression" => {
            let object = node.child_by_field_name("object");
            let property = node.child_by_field_name("property");
            match (object, property) {
                (Some(object), Some(property)) if object.kind() == "identifier" => {
                    let object = source_text(object, content).unwrap_or_default().to_string();
                    let member = source_text(property, content)
                        .unwrap_or_default()
                        .to_string();
                    let optional = node.child_by_field_name("optional_chain").is_some()
                        || source_text(node, content).is_some_and(|text| text.contains("?."));
                    ObservedForm::Member {
                        object,
                        member,
                        optional,
                    }
                }
                _ => ObservedForm::Dynamic("member-chain".to_string()),
            }
        }
        "subscript_expression" => ObservedForm::Dynamic("computed-member".to_string()),
        "import" => ObservedForm::Dynamic("dynamic-import".to_string()),
        kind => ObservedForm::Dynamic(kind.to_string()),
    }
}

fn is_non_reference_identifier(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "import_statement"
                | "import_clause"
                | "import_specifier"
                | "namespace_import"
                | "export_specifier"
                | "formal_parameters"
                | "required_parameter"
                | "optional_parameter"
                | "rest_pattern"
                | "array_pattern"
                | "object_pattern"
                | "pair_pattern"
                | "type_annotation"
                | "type_arguments"
                | "type_parameters"
                | "interface_declaration"
                | "type_alias_declaration"
        ) {
            return true;
        }
        if matches!(
            parent.kind(),
            "arguments"
                | "binary_expression"
                | "unary_expression"
                | "return_statement"
                | "expression_statement"
                | "variable_declarator"
                | "pair"
                | "array"
        ) {
            return false;
        }
        node = parent;
    }
    true
}

fn pattern_identifiers(root: Node<'_>, content: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "identifier" {
            if let Some(name) = source_text(node, content) {
                names.push(name.to_string());
            }
            continue;
        }
        if matches!(node.kind(), "type_annotation" | "default_type") {
            continue;
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(crate::numeric::usize_to_u32(index)) {
                stack.push(child);
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

fn is_callable_variable(node: Node<'_>) -> bool {
    node.child_by_field_name("value").is_some_and(|value| {
        matches!(
            value.kind(),
            "arrow_function" | "function_expression" | "generator_function"
        )
    })
}

fn strip_quotes(text: &str) -> Option<&str> {
    text.strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            text.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
}
