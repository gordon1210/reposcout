use super::*;

fn project(root: &Path) {
    for (path, content) in [
        (
            "project.godot",
            "config_version=5\n[application]\nconfig/name=\"Fixture\"\nrun/main_scene=\"res://main.tscn\"\n",
        ),
        (
            "player.gd",
            "class_name Player\nextends Node\nsignal damaged(amount: int)\nfunc damage(amount: int):\n    if amount > 0:\n        damaged.emit(amount)\n",
        ),
        ("player.gd.uid", "uid://player1\n"),
        (
            "main.tscn",
            "[gd_scene format=3]\n[ext_resource type=\"Script\" uid=\"uid://player1\" path=\"res://player.gd\" id=\"1\"]\n[node name=\"Main\" type=\"Node\"]\nscript=ExtResource(\"1\")\n",
        ),
        (
            "item.tres",
            "; TODO content is not code health\n[gd_resource type=\"Resource\" format=3]\n[resource]\nresource_name=\"Item\"\n",
        ),
        (
            "effect.gdshader",
            "shader_type canvas_item;\nvoid fragment() { COLOR = vec4(1.0); }\n",
        ),
    ] {
        std::fs::write(root.join(path), content).unwrap();
    }
}

#[test]
fn godot_cli_inventory_health_graph_and_navigation_are_integrated() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let path = dir.path().to_str().unwrap();
    let report = run_json(&["-f", "json", "--graph", path]);
    assert_eq!(report["diagnostics"]["unsupported_files"], 0);
    for name in [
        "GDScript",
        "Godot Shader",
        "Godot Scene",
        "Godot Resource",
        "Godot Project",
        "Text",
    ] {
        assert!(
            language_names(&report)
                .iter()
                .any(|language| language == name),
            "{name}"
        );
    }
    let files = report["files"].as_array().unwrap();
    for name in ["project.godot", "main.tscn", "item.tres", "player.gd.uid"] {
        let file = files.iter().find(|file| file["path"] == name).unwrap();
        assert!(file["tokens"].as_u64().unwrap() > 0);
        assert!(file["complexity"].is_null(), "{name}");
    }
    assert_eq!(
        report["summary"]["markers"]
            .get("TODO")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        0
    );
    assert_eq!(report["graph"]["unresolved_imports"], 0);
    let signals = run_json(&[
        "locate", "damaged", "--kind", "signal", "--exact", "-f", "json", path,
    ]);
    assert_eq!(signals["total_matches"], 1);
    assert_eq!(signals["matches"][0]["path"], "player.gd");
    let nodes = run_json(&[
        "locate", "Main", "--kind", "node", "--exact", "-f", "json", path,
    ]);
    assert_eq!(nodes["total_matches"], 1);
    assert_eq!(nodes["matches"][0]["path"], "main.tscn");
    let context = run_json(&[
        "-f",
        "json",
        "--summary",
        "--context",
        "--focus",
        "player.gd",
        "--context-max-files",
        "3",
        path,
    ]);
    let selected = context["context"]["files"].as_array().unwrap();
    assert!(selected.iter().any(|file| file["path"] == "player.gd"));
    assert!(selected.iter().any(|file| file["path"] == "main.tscn"));
}

#[test]
fn godot_cached_graph_matches_cold_scan_and_picks_up_uid_changes() {
    let dir = tempfile::tempdir().unwrap();
    project(dir.path());
    let path = dir.path().to_str().unwrap();
    let scan = |graph: bool| {
        let mut command = reposcout_command();
        command.args(["-f", "json", "--quiet", path]);
        if graph {
            command.arg("--graph");
        }
        serde_json::from_slice::<Value>(&command.assert().success().get_output().stdout).unwrap()
    };
    // Ordinary scan populates the cache without any graph enrichment.
    scan(false);
    let cold = scan(true);
    let warm = scan(true);
    assert_eq!(cold["graph"], warm["graph"]);
    assert_eq!(cold["files"], warm["files"]);
    assert_eq!(warm["graph"]["unresolved_imports"], 0);
    std::fs::write(dir.path().join("other.gd"), "extends Node\n").unwrap();
    std::fs::write(dir.path().join("other.gd.uid"), "uid://player1\n").unwrap();
    let ambiguous = scan(true);
    assert_eq!(
        ambiguous["graph"]["unresolved_imports"], 1,
        "ambiguous UID must not silently use its path fallback"
    );
}
