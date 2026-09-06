use crate::graph::{
    self, BTreeMap, GraphBuildRequest, GraphReadLimits, HashSet, Path, PathBuf, SourceFacts,
};
use crate::{cli::GraphDirection, lang::detect};

fn graph(sources: &[(&str, &str)], sidecars: &[(&str, &str)]) -> graph::GraphAnalysis {
    let paths = sources
        .iter()
        .map(|(path, _)| PathBuf::from(path))
        .collect::<Vec<_>>();
    let facts = sources
        .iter()
        .filter_map(|(path, content)| {
            let language = detect(Path::new(path))?.first_class?;
            let facts = graph::extract_source_facts(language, path, content);
            assert_eq!(facts.parse_errors, 0, "{path}");
            Some((PathBuf::from(path), facts))
        })
        .collect::<BTreeMap<PathBuf, SourceFacts>>();
    let configs = sources
        .iter()
        .copied()
        .filter(|(path, _)| path.ends_with("project.godot"))
        .chain(sidecars.iter().copied())
        .map(|(path, content)| (path.to_string(), content.to_string()))
        .collect();
    let dir = tempfile::tempdir().unwrap();
    graph::build_from_paths_with_query(GraphBuildRequest {
        paths: &paths,
        root: dir.path(),
        virtual_paths: &HashSet::new(),
        source_facts: Some(&facts),
        resolver_configs: Some(&configs),
        limits: GraphReadLimits {
            max_files: 0,
            max_total_bytes: 0,
            facts_only_sources: true,
            ..GraphReadLimits::default()
        },
        focus: &[],
        direction: GraphDirection::Both,
        depth: 2,
    })
}

fn has_edge(graph: &graph::GraphAnalysis, source: &str, target: &str) -> bool {
    graph.topology.edges.iter().any(|&(from, to)| {
        graph.topology.graph_files[from] == source && graph.topology.graph_files[to] == target
    })
}

#[test]
fn godot_project_scenes_scripts_shaders_classes_and_autoloads_share_graph() {
    let built = graph(
        &[
            (
                "project.godot",
                "config_version=5\n[application]\nrun/main_scene=\"res://main.tscn\"\n[autoload]\nGlobals=\"*res://globals.gd\"\n",
            ),
            (
                "main.tscn",
                "[gd_scene format=3 uid=\"uid://scene1\"]\n[ext_resource type=\"Script\" path=\"res://player.gd\" id=\"1\"]\n[ext_resource type=\"ShaderMaterial\" path=\"res://material.tres\" id=\"2\"]\n[node name=\"Main\" type=\"Node\"]\nscript=ExtResource(\"1\")\n",
            ),
            (
                "player.gd",
                "extends Actor\nconst ITEM = preload(\"uid://item1\")\nfunc run():\n    Globals.score += 1\n    return ITEM.new()\n",
            ),
            ("actor.gd", "class_name Actor\nextends Node\n"),
            ("globals.gd", "extends Node\nvar score = 0\n"),
            ("item.gd", "extends Resource\n"),
            (
                "material.tres",
                "[gd_resource type=\"ShaderMaterial\" format=3]\n[ext_resource type=\"Shader\" path=\"res://effect.gdshader\" id=\"1\"]\n[resource]\nshader=ExtResource(\"1\")\n",
            ),
            (
                "effect.gdshader",
                "shader_type canvas_item;\n#include \"res://math.gdshaderinc\"\nvoid fragment() { COLOR = vec4(1.0); }\n",
            ),
            (
                "math.gdshaderinc",
                "float scale(float value) { return value * 2.0; }\n",
            ),
        ],
        &[("item.gd.uid", "uid://item1\n")],
    );
    for (source, target) in [
        ("project.godot", "main.tscn"),
        ("project.godot", "globals.gd"),
        ("main.tscn", "player.gd"),
        ("main.tscn", "material.tres"),
        ("player.gd", "actor.gd"),
        ("player.gd", "globals.gd"),
        ("player.gd", "item.gd"),
        ("material.tres", "effect.gdshader"),
        ("effect.gdshader", "math.gdshaderinc"),
    ] {
        assert!(
            has_edge(&built, source, target),
            "missing {source} -> {target}: {:?}",
            built.topology.edges
        );
    }
    assert_eq!(built.topology.unresolved_imports, 0);
    assert_eq!(built.topology.config_errors, 0);
    assert!(built.topology.unreadable_nodes.is_empty());
    let impact = graph::impact_from_topology(
        &built.topology,
        &HashSet::from([PathBuf::from("math.gdshaderinc")]),
    );
    assert!(!impact.transitive_dependents.is_empty());
}

#[test]
fn godot_project_names_and_uids_do_not_cross_nested_projects() {
    let built = graph(
        &[
            ("project.godot", "config_version=5\n"),
            ("base.gd", "class_name Actor\nextends Node\n"),
            (
                "use.gd",
                "extends Actor\nconst Base = preload(\"uid://same1\")\n",
            ),
            ("nested/project.godot", "config_version=5\n"),
            ("nested/base.gd", "class_name Actor\nextends Node\n"),
            (
                "nested/use.gd",
                "extends Actor\nconst Base = preload(\"uid://same1\")\n",
            ),
        ],
        &[
            ("base.gd.uid", "uid://same1"),
            ("nested/base.gd.uid", "uid://same1"),
        ],
    );
    assert!(has_edge(&built, "use.gd", "base.gd"));
    assert!(has_edge(&built, "nested/use.gd", "nested/base.gd"));
    assert_eq!(built.topology.edges.len(), 2);
    assert_eq!(built.topology.unresolved_imports, 0);
}

#[test]
fn godot_dynamic_ambiguous_and_escaping_paths_stay_unresolved() {
    let built = graph(
        &[
            ("project.godot", "config_version=5\n"),
            ("first.gd", "class_name Actor\nextends Node\n"),
            ("second.gd", "class_name Actor\nextends Node\n"),
            (
                "use.gd",
                "extends Actor\nfunc run(path):\n    var a = load(path)\n    var b = load(\"res://../outside.gd\")\n    var c = load(\"uid://ambiguous1\")\n    return [a, b, c]\n",
            ),
        ],
        &[
            ("first.gd.uid", "uid://ambiguous1"),
            ("second.gd.uid", "uid://ambiguous1"),
        ],
    );
    assert_eq!(built.topology.unresolved_imports, 4);
    assert!(built.topology.edges.is_empty());
}

#[test]
fn godot_uid_prefers_snapshot_target_and_path_is_a_fallback() {
    let built = graph(
        &[
            ("project.godot", "config_version=5\n"),
            ("new.gd", "extends Node\n"),
            ("old.gd", "extends Node\n"),
            (
                "main.tscn",
                "[gd_scene format=3]\n[ext_resource type=\"Script\" uid=\"uid://script1\" path=\"res://old.gd\" id=\"1\"]\n[ext_resource type=\"Script\" uid=\"uid://missing1\" path=\"res://old.gd\" id=\"2\"]\n[node name=\"Main\" type=\"Node\"]\n",
            ),
        ],
        &[("new.gd.uid", "uid://script1\n")],
    );
    assert!(has_edge(&built, "main.tscn", "new.gd"));
    assert!(has_edge(&built, "main.tscn", "old.gd"));
    assert_eq!(built.topology.unresolved_imports, 0);
}

#[test]
fn godot_comments_nodepaths_strings_and_shadowed_globals_are_not_edges() {
    let built = graph(
        &[
            ("project.godot", "config_version=5\n"),
            ("actor.gd", "class_name Actor\nextends Node\n"),
            (
                "use.gd",
                "extends Node\n# preload(\"res://actor.gd\")\nvar label = \"res://actor.gd\"\n@onready var actor = $Actor\nfunc run(Actor):\n    return Actor.new()\n",
            ),
            (
                "main.tscn",
                "[gd_scene format=3]\n[node name=\"Main\" type=\"Node\"]\ntarget=NodePath(\"res://actor.gd\")\n",
            ),
        ],
        &[],
    );
    assert!(built.topology.edges.is_empty());
    assert_eq!(built.topology.unresolved_imports, 0);
}

#[test]
fn godot_relative_paths_and_resource_loader_calls_are_resolved() {
    let built = graph(
        &[
            ("project.godot", "config_version=5\n"),
            ("base.gd", "extends Node\n"),
            (
                "sub/child.gd",
                "extends \"../base.gd\"\nfunc run():\n    return ResourceLoader.load(\"res://base.gd\")\n",
            ),
        ],
        &[],
    );
    assert!(has_edge(&built, "sub/child.gd", "base.gd"));
    assert_eq!(built.topology.unresolved_imports, 0);
}

#[test]
fn godot_binary_assets_are_not_missing_code_and_self_names_are_not_cycles() {
    let built = graph(
        &[
            ("project.godot", "config_version=5\n"),
            (
                "actor.gd",
                "class_name Actor\nextends Node\nfunc create():\n    return Actor.new()\n",
            ),
            (
                "main.tscn",
                "[gd_scene format=3]\n[ext_resource type=\"Texture2D\" path=\"res://icon.png\" id=\"1\"]\n[ext_resource type=\"Script\" path=\"res://Actor.cs\" id=\"2\"]\n[node name=\"Main\" type=\"Node\"]\n",
            ),
        ],
        &[],
    );
    assert_eq!(built.topology.unresolved_imports, 0);
    assert!(built.report.cycles.is_empty());
}

#[test]
fn godot_resolver_config_capture_is_bounded_and_snapshot_only() {
    let dir = tempfile::tempdir().unwrap();
    let paths = vec![PathBuf::from("actor.gd")];
    std::fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    std::fs::write(dir.path().join("actor.gd.uid"), "uid://actor1\n").unwrap();
    let captured = graph::collect_resolver_configs(dir.path(), &paths, &GraphReadLimits::default());
    assert_eq!(captured.len(), 2);
    let tiny = GraphReadLimits {
        max_file_bytes: 4,
        ..GraphReadLimits::default()
    };
    assert!(graph::collect_resolver_configs(dir.path(), &paths, &tiny).is_empty());
    let expired = GraphReadLimits {
        deadline: Some(std::time::Instant::now()),
        ..GraphReadLimits::default()
    };
    assert!(graph::collect_resolver_configs(dir.path(), &paths, &expired).is_empty());
    std::fs::write(dir.path().join("actor.gd.uid"), "uid://modified1\n").unwrap();
    let mut budget = GraphReadLimits::default().budget();
    let mut access = graph::ConfigAccess {
        root: dir.path(),
        budget: &mut budget,
        snapshot: Some(&captured),
    };
    let resolver = super::GodotResolver::discover(&["actor.gd".to_string()], &mut access);
    let nodes = HashSet::from(["actor.gd".to_string()]);
    let reference = super::Reference::Resource {
        path: Some("uid://actor1".to_string()),
        uid: None,
    };
    assert!(
        matches!(resolver.resolve("main.gd", reference, &nodes), graph::ImportResolution::Resolved { target, .. } if target == "actor.gd")
    );
}

#[cfg(unix)]
#[test]
fn godot_uid_sidecars_do_not_follow_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    std::fs::write(outside.path().join("private.uid"), "uid://private1\n").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("private.uid"),
        dir.path().join("actor.gd.uid"),
    )
    .unwrap();
    let captured = graph::collect_resolver_configs(
        dir.path(),
        &[PathBuf::from("actor.gd")],
        &GraphReadLimits::default(),
    );
    assert_eq!(captured.len(), 1);
    assert!(captured.contains_key("project.godot"));
}

#[test]
fn godot_known_project_boundaries_survive_unavailable_config_contents() {
    let dir = tempfile::tempdir().unwrap();
    let snapshot = BTreeMap::new();
    let mut budget = GraphReadLimits::default().budget();
    let mut access = graph::ConfigAccess {
        root: dir.path(),
        budget: &mut budget,
        snapshot: Some(&snapshot),
    };
    let files = ["project.godot", "nested/project.godot", "nested/use.gd"].map(str::to_string);
    let resolver = super::GodotResolver::discover(&files, &mut access);
    assert_eq!(resolver.config_errors_by_path.len(), 2);
    let nodes = HashSet::from(["base.gd".to_string(), "nested/base.gd".to_string()]);
    let reference = super::Reference::Resource {
        path: Some("res://base.gd".to_string()),
        uid: None,
    };
    assert!(
        matches!(resolver.resolve("nested/use.gd", reference, &nodes), graph::ImportResolution::Resolved { target, .. } if target == "nested/base.gd")
    );
}
