//! Shared syntax helpers for Godot's text formats. No filesystem access.
use tree_sitter::Node;

pub(crate) fn text<'a>(node: Node<'_>, content: &'a str) -> &'a str {
    node.utf8_text(content.as_bytes()).unwrap_or_default()
}

pub(crate) fn string(node: Node<'_>, content: &str) -> Option<String> {
    if !matches!(node.kind(), "string" | "string_literal") {
        return None;
    }
    let raw = text(node, content);
    let (raw, is_raw) = raw
        .strip_prefix('r')
        .map_or((raw, false), |raw| (raw, true));
    let quote = raw.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let delimiter = quote.to_string();
    let triple = delimiter.repeat(3);
    let delimiter = if raw.starts_with(&triple) {
        &triple
    } else {
        &delimiter
    };
    let inner = raw.strip_prefix(delimiter)?.strip_suffix(delimiter)?;
    if is_raw {
        return Some(inner.to_string());
    }
    let mut decoded = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }
        decoded.push(match chars.next()? {
            '\\' => '\\',
            '\'' => '\'',
            '"' => '"',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            // Unhandled escape forms stay unresolved instead of guessing a path.
            _ => return None,
        });
    }
    Some(decoded)
}

pub(crate) fn section_name<'a>(node: Node<'_>, content: &'a str) -> &'a str {
    node.named_child(0).map_or("", |name| text(name, content))
}

pub(crate) fn attribute(node: Node<'_>, name: &str, content: &str) -> Option<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "attribute")
        .find_map(|child| {
            (text(child.named_child(0)?, content) == name)
                .then(|| string(child.named_child(1)?, content))?
        })
}

#[cfg(test)]
mod tests {
    use crate::{
        lang::{self, FirstClass, HealthScope},
        metrics::{complexity, symbols},
        parse,
    };
    use std::path::Path;

    #[test]
    fn godot_formats_keep_inventory_separate_from_code_health() {
        for (path, name, code) in [
            ("player.gd", "GDScript", true),
            ("effect.gdshader", "Godot Shader", true),
            ("lib.gdshaderinc", "Godot Shader", true),
            ("main.tscn", "Godot Scene", false),
            ("export.escn", "Godot Scene", false),
            ("item.tres", "Godot Resource", false),
            ("project.godot", "Godot Project", false),
            ("addons/tool/plugin.cfg", "Godot Resource", false),
        ] {
            let info = lang::detect(Path::new(path)).unwrap();
            assert_eq!(info.name, name);
            assert_eq!(info.is_code(), code, "{path}");
            assert_eq!(
                lang::included_in_health(info, HealthScope::Source, &[]),
                code
            );
            assert!(lang::included_in_health(info, HealthScope::All, &[]));
        }
        assert!(lang::detect(Path::new("other.godot")).is_none());
        assert!(lang::detect(Path::new("settings.cfg")).is_none());
    }

    #[test]
    fn godot_gdscript_metrics_and_body_free_navigation() {
        let content = "class_name Player\nextends Node\nsignal damaged(amount: int)\nconst SPEED = 4\n@export var health: int = 10\nfunc _init():\n    pass\nfunc choose(value: int) -> int:\n    match value:\n        1:\n            return SPEED\n        _ when value > 2:\n            return 2\n        _:\n            return 0\nclass Inventory:\n    func count_items():\n        return 5\n";
        let tree = parse::parse(FirstClass::GdScript, content).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let analysis = symbols::analyze(FirstClass::GdScript, content, &tree);
        assert_eq!(analysis.counts.functions, 3);
        assert_eq!(analysis.counts.types, 2);
        for (name, kind) in [
            ("Player", "class"),
            ("damaged", "signal"),
            ("SPEED", "constant"),
            ("health", "property"),
            ("_init", "method"),
            ("Inventory.count_items", "method"),
        ] {
            assert!(
                analysis
                    .outlines
                    .iter()
                    .any(|outline| outline.name == name && outline.kind == kind),
                "{name}: {:?}",
                analysis.outlines
            );
        }
        assert!(
            analysis
                .outlines
                .iter()
                .all(|outline| !outline.signature.contains("return"))
        );
        let info = lang::detect(Path::new("player.gd")).unwrap();
        let metrics = complexity::analyze(
            info,
            content,
            Some(&tree),
            &crate::metrics::lines::LineStats::default(),
        )
        .0;
        let choose = metrics
            .functions
            .iter()
            .find(|function| function.name == "choose")
            .unwrap();
        assert_eq!(
            choose.cyclomatic, 4,
            "base + two non-default patterns + guard"
        );
        assert_eq!(
            metrics
                .functions
                .iter()
                .filter(|function| function.name == "_init")
                .count(),
            1
        );
    }

    #[test]
    fn godot_shader_metrics_and_scene_node_outlines() {
        let shader = "shader_type canvas_item;\n#include \"res://math.gdshaderinc\"\nvoid fragment() {\n    if (UV.x > 0.5) { COLOR = vec4(1.0); } else { COLOR = vec4(0.0); }\n}\n";
        let tree = parse::parse(FirstClass::GdShader, shader).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let analysis = symbols::analyze(FirstClass::GdShader, shader, &tree);
        assert_eq!(analysis.counts.functions, 1);
        assert_eq!(analysis.outlines[0].name, "fragment");
        assert!(!analysis.outlines[0].signature.contains("COLOR"));
        let info = lang::detect(Path::new("effect.gdshader")).unwrap();
        assert_eq!(
            complexity::analyze(
                info,
                shader,
                Some(&tree),
                &crate::metrics::lines::LineStats::default()
            )
            .0
            .functions[0]
                .cyclomatic,
            2
        );
        let scene = "[gd_scene format=3]\n[node name=\"Main\" type=\"Node2D\"]\n[node name=\"Label\" type=\"Label\" parent=\"Panel\"]\ntext=\"Body must be omitted\"\n";
        let tree = parse::parse(FirstClass::GodotResource, scene).unwrap();
        assert!(!tree.root_node().has_error());
        let outlines = symbols::analyze(FirstClass::GodotResource, scene, &tree).outlines;
        assert_eq!(outlines.len(), 2);
        assert_eq!(outlines[1].name, "Panel/Label");
        assert!(
            outlines
                .iter()
                .all(|outline| outline.kind == "node" && !outline.signature.contains("Body"))
        );
    }

    #[test]
    fn godot_property_accessors_have_independent_complexity() {
        let content = "extends Node\nvar health: int:\n    get:\n        if health < 0:\n            return 0\n        return health\n    set(value):\n        if value > 0:\n            health = value\n";
        let tree = parse::parse(FirstClass::GdScript, content).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let info = lang::detect(Path::new("player.gd")).unwrap();
        let metrics = complexity::analyze(
            info,
            content,
            Some(&tree),
            &crate::metrics::lines::LineStats::default(),
        )
        .0;
        let outlines = symbols::analyze(FirstClass::GdScript, content, &tree).outlines;
        assert!(
            outlines
                .iter()
                .all(|outline| !outline.signature.contains("return")
                    && !outline.signature.contains("if value"))
        );
        assert_eq!(metrics.functions.len(), 2);
        for name in ["health.get", "health.set"] {
            assert!(
                metrics
                    .functions
                    .iter()
                    .any(|function| function.name == name && function.cyclomatic == 2),
                "{:?}",
                metrics.functions
            );
        }
    }

    #[test]
    fn godot_shader_ternaries_cases_and_else_if_use_the_shader_grammar() {
        let content = "shader_type canvas_item;\nfloat decide(float x) {\n    float y = x > 0.0 ? 1.0 : 0.0;\n    switch (int(x)) { case 1: y += 1.0; break; default: break; }\n    return y;\n}\nvoid fragment() {\n    if (UV.x > 0.5) { COLOR = vec4(1.0); } else if (UV.x > 0.2) { COLOR = vec4(0.5); } else { COLOR = vec4(0.0); }\n}\n";
        let tree = parse::parse(FirstClass::GdShader, content).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let info = lang::detect(Path::new("effect.gdshader")).unwrap();
        let metrics = complexity::analyze(
            info,
            content,
            Some(&tree),
            &crate::metrics::lines::LineStats::default(),
        )
        .0;
        let decide = metrics
            .functions
            .iter()
            .find(|function| function.name == "decide")
            .unwrap();
        assert_eq!(decide.cyclomatic, 3);
        let fragment = metrics
            .functions
            .iter()
            .find(|function| function.name == "fragment")
            .unwrap();
        assert_eq!(fragment.cyclomatic, 3);
        assert_eq!(fragment.cognitive, 3);
        assert_eq!(fragment.max_nesting, 1);
    }
}
