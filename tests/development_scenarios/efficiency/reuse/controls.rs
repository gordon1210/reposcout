//! Cheap oracle sensitivity controls, never claimed as successful public CLI episodes.

use super::driver::{Capture, Investigation, Retained};
use super::evidence::{attribution, initial_missing, packet_missing};
use super::sha256;
use super::world::{FLOOR, HELPER_PATH, QuotaWorld};
use serde_json::{Value, json};

fn literal_capture(world: &QuotaWorld, path: &str, source: &str) -> Capture {
    let file = &world.before[path];
    let start = file.find(source).unwrap();
    let end = start + source.len();
    Capture {
        file: json!({
            "id": 1, "path": path,
            "sha256": sha256(file),
            "snapshot": {"kind": "tree", "revision": world.base_tree},
        }),
        source: json!({
            "id": 1, "file": 1, "content": source,
            "span": {
                "start_byte": start, "end_byte": end,
                "start_line": 1 + file[..start].bytes().filter(|byte| *byte == b'\n').count(),
                "end_line": 1 + file[..end - 1].bytes().filter(|byte| *byte == b'\n').count(),
            },
        }),
        symbol: None,
        quote_origin: None,
    }
}

pub(super) fn assert_disjoint_packet_sensitivity(world: &QuotaWorld) {
    let initial: Vec<_> = world
        .initial_packet
        .iter()
        .map(|fragment| Retained {
            capture: literal_capture(world, fragment.path, &fragment.source),
            current_proof: None,
        })
        .collect();
    assert!(packet_missing(&world.initial_packet, initial.iter()).is_empty());
    for removed in 0..initial.len() {
        assert!(
            !packet_missing(
                &world.initial_packet,
                initial
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| { (index != removed).then_some(value) }),
            )
            .is_empty(),
            "every independently frozen initial fragment is necessary"
        );
    }
    let delta: Vec<_> = world
        .delta_packet
        .iter()
        .map(|fragment| {
            let file = &world.after[fragment.path];
            let start = file.find(&fragment.source).unwrap();
            let end = start + fragment.source.len();
            Retained {
                capture: Capture {
                    file: json!({"id": 1, "path": fragment.path,
                        "sha256": sha256(file), "snapshot": {"kind": "worktree"}}),
                    source: json!({"id": 1, "file": 1, "content": fragment.source,
                        "span": {
                            "start_byte": start, "end_byte": end,
                            "start_line": 1 + file[..start].bytes().filter(|byte| *byte == b'\n').count(),
                            "end_line": 1 + file[..end - 1].bytes().filter(|byte| *byte == b'\n').count(),
                        }}),
                    symbol: None,
                    quote_origin: None,
                },
                current_proof: None,
            }
        })
        .collect();
    assert!(packet_missing(&world.delta_packet, delta.iter()).is_empty());
    for removed in 0..delta.len() {
        assert!(
            !packet_missing(
                &world.delta_packet,
                delta
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| { (index != removed).then_some(value) }),
            )
            .is_empty(),
            "each changed binding and each actual helper is indispensable"
        );
    }
}

pub(super) fn assert_oracle_sensitivity(world: &QuotaWorld) {
    let mut state = Investigation::default();
    for required in &world.initial_packet {
        state.retained.insert(
            required.path.to_owned(),
            vec![Retained {
                capture: literal_capture(world, required.path, &required.source),
                current_proof: None,
            }],
        );
    }
    assert!(
        initial_missing(world, &state).is_empty(),
        "the independently authored initial packet must be sufficient"
    );
    for required in &world.initial_packet {
        let removed = state.retained.remove(required.path).unwrap();
        assert!(
            !initial_missing(world, &state).is_empty(),
            "removing {} must lose a necessary obligation",
            required.path
        );
        state.retained.insert(required.path.to_owned(), removed);
    }
    let mut counterfeit = literal_capture(world, HELPER_PATH, FLOOR.trim_end_matches('\n'));
    counterfeit.file["snapshot"] = json!({"kind": "worktree"});
    counterfeit.file["sha256"] = Value::String(sha256(&world.after[HELPER_PATH]));
    assert!(
        !attribution(HELPER_PATH, &counterfeit, &world.after).is_empty(),
        "attaching current metadata to retained old bytes cannot establish current source"
    );
}
