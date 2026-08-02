//! Deterministic dependency-graph exports for humans and graph tooling.

use crate::model::DepGraph;
use std::collections::BTreeMap;
use std::fmt::Write;

#[must_use]
pub fn dot(graph: &DepGraph) -> String {
    let ids = node_ids(graph);
    let mut out = String::from(
        "digraph reposcout {\n  graph [rankdir=LR, bgcolor=\"transparent\"];\n  node [shape=box, style=\"rounded\", fontname=\"Helvetica\"];\n",
    );
    for file in &graph.files {
        let Some(id) = ids.get(&file.path) else {
            continue;
        };
        let focus_style = if file.focus_distance == Some(0) {
            ", style=\"rounded,filled\", fillcolor=\"#e8eefc\""
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "  {id} [label=\"{}\"{focus_style}];",
            escape_dot(&file.path)
        );
    }
    for edge in &graph.edge_list {
        let (Some(source), Some(target)) = (ids.get(&edge.source), ids.get(&edge.target)) else {
            continue;
        };
        let _ = writeln!(out, "  {source} -> {target};");
    }
    out.push_str("}\n");
    out
}

#[must_use]
pub fn mermaid(graph: &DepGraph) -> String {
    let ids = node_ids(graph);
    let mut out = String::from("flowchart LR\n");
    let mut focus_ids = Vec::new();
    for file in &graph.files {
        let Some(id) = ids.get(&file.path) else {
            continue;
        };
        let _ = writeln!(out, "  {id}[\"{}\"]", escape_mermaid(&file.path));
        if file.focus_distance == Some(0) {
            focus_ids.push(id.as_str());
        }
    }
    for edge in &graph.edge_list {
        let (Some(source), Some(target)) = (ids.get(&edge.source), ids.get(&edge.target)) else {
            continue;
        };
        let _ = writeln!(out, "  {source} --> {target}");
    }
    if !focus_ids.is_empty() {
        let _ = writeln!(out, "  class {} focus", focus_ids.join(","));
        out.push_str("  classDef focus fill:#e8eefc,stroke:#36558f,stroke-width:2px\n");
    }
    out
}

fn node_ids(graph: &DepGraph) -> BTreeMap<String, String> {
    graph
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| (file.path.clone(), format!("n{index}")))
        .collect()
}

fn escape_dot(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn escape_mermaid(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::{dot, mermaid};
    use crate::model::{DepGraph, GraphEdge, GraphFile};

    fn graph() -> DepGraph {
        DepGraph {
            files: vec![
                GraphFile {
                    path: "src/a.ts".to_string(),
                    focus_distance: Some(0),
                    ..GraphFile::default()
                },
                GraphFile {
                    path: "src/b\".ts".to_string(),
                    ..GraphFile::default()
                },
            ],
            edge_list: vec![GraphEdge {
                source: "src/a.ts".to_string(),
                target: "src/b\".ts".to_string(),
                resolver: "relative".to_string(),
            }],
            ..DepGraph::default()
        }
    }

    #[test]
    fn dot_is_deterministic_and_escapes_labels() {
        let rendered = dot(&graph());
        assert!(rendered.contains("n0 -> n1"));
        assert!(rendered.contains("src/b\\\".ts"));
        assert!(rendered.contains("fillcolor"));
    }

    #[test]
    fn mermaid_uses_safe_ids_and_marks_focus() {
        let rendered = mermaid(&graph());
        assert!(rendered.contains("n0 --> n1"));
        assert!(rendered.contains("src/b&quot;.ts"));
        assert!(rendered.contains("class n0 focus"));
    }
}
