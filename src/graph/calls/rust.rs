use super::MAX_NODES_PER_FILE;
use super::extract::{
    ObservedForm, ObservedRelation, ShadowBinding, State, enclosing_inline_rust_modules,
    enclosing_scope, is_call_target, is_declaration_name, is_inside_handled_target,
    match_definition, source_text, span,
};
use crate::model::{CallImportBinding, CallImportKind, CallReferenceKind, SourceSpan};
use tree_sitter::Node;

pub(super) fn visit(node: Node<'_>, state: &mut State<'_>) {
    match node.kind() {
        "use_declaration" => extract_use(node, state),
        "function_item" => record_declaration_metadata(node, state),
        "parameters" | "closure_parameters" => record_parameters(node, state),
        "let_declaration" => record_local_binding(node, "pattern", state),
        "for_expression" | "match_arm" => record_scoped_pattern(node, state),
        "call_expression" => record_call(node, state),
        "macro_invocation" => {
            state.mark_unsupported();
            state.push_relation(ObservedRelation {
                kind: CallReferenceKind::Call,
                site: span(node),
                form: ObservedForm::Dynamic("macro-invocation".to_string()),
            });
        }
        "scoped_identifier" if !is_call_target(node) && !inside_scoped_identifier(node) => {
            record_scoped_reference(node, state);
        }
        "field_expression" if !is_call_target(node) => record_field_reference(node, state),
        "identifier" => record_identifier(node, state),
        _ => {}
    }
}

fn extract_use(node: Node<'_>, state: &mut State<'_>) {
    let Some(argument) = node.child_by_field_name("argument") else {
        state.mark_unsupported();
        return;
    };
    let scope = enclosing_scope(node);
    let inline_modules = enclosing_inline_rust_modules(node, state.content());
    let mut stack = vec![(argument, String::new())];
    let mut examined = 0usize;
    while let Some((candidate, prefix)) = stack.pop() {
        if examined >= MAX_NODES_PER_FILE {
            let omitted_nodes = candidate.descendant_count().saturating_add(
                stack
                    .iter()
                    .map(|(queued, _)| queued.descendant_count())
                    .fold(0usize, usize::saturating_add),
            );
            state.mark_work_truncated(omitted_nodes);
            break;
        }
        examined = examined.saturating_add(1);
        match candidate.kind() {
            "scoped_use_list" => {
                let next = candidate
                    .child_by_field_name("path")
                    .and_then(|path| source_text(path, state.content()))
                    .map_or(prefix.clone(), |path| join_path(&prefix, path));
                if let Some(list) = candidate.child_by_field_name("list") {
                    stack.push((list, next));
                }
            }
            "use_list" => {
                for index in (0..candidate.named_child_count()).rev() {
                    if let Some(child) = candidate.named_child(crate::numeric::usize_to_u32(index))
                    {
                        stack.push((child, prefix.clone()));
                    }
                }
            }
            "use_as_clause" => {
                let path = candidate
                    .child_by_field_name("path")
                    .and_then(|path| source_text(path, state.content()))
                    .map(str::to_string);
                let alias = candidate
                    .child_by_field_name("alias")
                    .and_then(|alias| source_text(alias, state.content()))
                    .map(str::to_string);
                if let (Some(path), Some(alias)) = (path, alias) {
                    push_use_binding(
                        join_path(&prefix, &path),
                        alias,
                        span(candidate),
                        scope,
                        &inline_modules,
                        state,
                    );
                } else {
                    state.mark_unsupported();
                }
            }
            "identifier" | "scoped_identifier" | "crate" | "self" | "super" => {
                if let Some(path) = source_text(candidate, state.content()).map(str::to_string) {
                    let full = join_path(&prefix, &path);
                    let local = if path == "self" {
                        prefix.rsplit("::").next().unwrap_or(&prefix)
                    } else {
                        path.rsplit("::").next().unwrap_or(&path)
                    }
                    .to_string();
                    push_use_binding(full, local, span(candidate), scope, &inline_modules, state);
                }
            }
            _ => state.mark_unsupported(),
        }
    }
}

fn push_use_binding(
    path: String,
    local_name: String,
    import_span: SourceSpan,
    scope_span: SourceSpan,
    inline_modules: &[String],
    state: &mut State<'_>,
) {
    let imported_name = path
        .rsplit("::")
        .next()
        .unwrap_or(path.as_str())
        .to_string();
    state.push_import(CallImportBinding {
        module_specifier: path,
        imported_name,
        local_name,
        kind: CallImportKind::RustPath,
        span: import_span,
        scope_span,
        inline_modules: inline_modules.to_vec(),
    });
}

fn record_declaration_metadata(node: Node<'_>, state: &mut State<'_>) {
    if let Some(key) = match_definition(node, state.definitions()) {
        state.set_declaration_scope(key, enclosing_scope(node));
        state.mark_declaration_hoisted(key);
    }
}

fn record_parameters(node: Node<'_>, state: &mut State<'_>) {
    let body = node
        .parent()
        .and_then(|parent| parent.child_by_field_name("body"));
    let scope = body.map_or_else(|| enclosing_scope(node), span);
    for name in pattern_identifiers(node, state.content()) {
        state.push_shadow(ShadowBinding {
            name,
            scope,
            active_from: scope.start_byte,
        });
    }
}

fn record_local_binding(node: Node<'_>, field: &str, state: &mut State<'_>) {
    let Some(pattern) = node.child_by_field_name(field) else {
        return;
    };
    let scope = enclosing_scope(node);
    for name in pattern_identifiers(pattern, state.content()) {
        state.push_shadow(ShadowBinding {
            name,
            scope,
            active_from: node.end_byte(),
        });
    }
}

fn record_scoped_pattern(node: Node<'_>, state: &mut State<'_>) {
    let Some(pattern) = node.child_by_field_name("pattern") else {
        return;
    };
    let body = node.child_by_field_name("body");
    let scope = body.map_or_else(|| enclosing_scope(node), span);
    for name in pattern_identifiers(pattern, state.content()) {
        state.push_shadow(ShadowBinding {
            name,
            scope,
            active_from: scope.start_byte,
        });
    }
}

fn record_call(node: Node<'_>, state: &mut State<'_>) {
    let Some(mut function) = node.child_by_field_name("function") else {
        state.mark_unsupported();
        return;
    };
    if function.kind() == "generic_function"
        && let Some(inner) = function.child_by_field_name("function")
    {
        function = inner;
    }
    state.push_relation(ObservedRelation {
        kind: CallReferenceKind::Call,
        site: span(node),
        form: expression_form(function, state.content()),
    });
}

fn record_scoped_reference(node: Node<'_>, state: &mut State<'_>) {
    if is_inside_handled_target(node) || is_non_reference_identifier(node) {
        return;
    }
    state.push_relation(ObservedRelation {
        kind: CallReferenceKind::Reference,
        site: span(node),
        form: expression_form(node, state.content()),
    });
}

fn record_field_reference(node: Node<'_>, state: &mut State<'_>) {
    if is_inside_handled_target(node) {
        return;
    }
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
        "scoped_identifier" => {
            let text = source_text(node, content).unwrap_or_default();
            let mut segments = text.split("::").filter(|segment| !segment.is_empty());
            let Some(first) = segments.next() else {
                return ObservedForm::Dynamic("invalid-qualified-path".to_string());
            };
            let mut all = vec![first];
            all.extend(segments);
            if all.len() < 2 {
                return ObservedForm::Bare(first.to_string());
            }
            let member = all.pop().unwrap_or_default().to_string();
            let qualifier = all.join("::");
            if qualifier == "Self"
                || qualifier
                    .split("::")
                    .any(|segment| segment.chars().next().is_some_and(char::is_uppercase))
                || qualifier.starts_with('<')
            {
                ObservedForm::Receiver {
                    object: qualifier,
                    member,
                }
            } else {
                ObservedForm::Qualified {
                    qualifier,
                    member,
                    inline_modules: enclosing_inline_rust_modules(node, content),
                }
            }
        }
        "field_expression" => {
            let object = node
                .child_by_field_name("value")
                .and_then(|value| source_text(value, content))
                .unwrap_or("receiver")
                .to_string();
            let member = node
                .child_by_field_name("field")
                .and_then(|field| source_text(field, content))
                .unwrap_or("field")
                .to_string();
            ObservedForm::Receiver { object, member }
        }
        kind => ObservedForm::Dynamic(kind.to_string()),
    }
}

fn is_non_reference_identifier(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        if parent.kind() == "let_declaration"
            && parent
                .child_by_field_name("pattern")
                .is_some_and(|pattern| pattern.id() == node.id())
        {
            return true;
        }
        if matches!(
            parent.kind(),
            "use_declaration"
                | "use_as_clause"
                | "use_list"
                | "scoped_use_list"
                | "use_wildcard"
                | "macro_invocation"
                | "parameters"
                | "closure_parameters"
                | "parameter"
                | "self_parameter"
                | "identifier_pattern"
                | "tuple_pattern"
                | "struct_pattern"
                | "field_pattern"
                | "type_identifier"
                | "generic_type"
                | "type_arguments"
                | "type_parameters"
                | "function_type"
                | "scoped_type_identifier"
        ) {
            return true;
        }
        if matches!(
            parent.kind(),
            "arguments"
                | "binary_expression"
                | "unary_expression"
                | "return_expression"
                | "expression_statement"
                | "let_declaration"
                | "array_expression"
                | "tuple_expression"
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
        if matches!(
            node.kind(),
            "type_identifier"
                | "generic_type"
                | "scoped_type_identifier"
                | "type_arguments"
                | "type_parameters"
        ) {
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

fn inside_scoped_identifier(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "scoped_identifier")
}

fn join_path(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        suffix.to_string()
    } else if suffix == "self" {
        prefix.to_string()
    } else {
        format!("{prefix}::{suffix}")
    }
}
