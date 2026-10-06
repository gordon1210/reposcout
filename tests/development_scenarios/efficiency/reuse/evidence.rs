use super::driver::{
    Capture, Continuation, Investigation, Retained, changed_ranges, complete_changes, disjoint,
    text,
};
use super::sha256;
use super::world::{
    ALTERNATE_PATH, CEIL, ENTRY, ENTRY_PATH, FLOOR, HELPER_PATH, NEW_BINDING, OLD_BINDING,
    POLICY_BODY, POLICY_PATH, QuotaWorld, RequiredFragment, Variant,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn initial_missing(world: &QuotaWorld, state: &Investigation) -> Vec<String> {
    let mut missing = packet_missing(&world.initial_packet, state.retained.values());
    for (path, retained) in &state.retained {
        if retained.capture.file["snapshot"]["kind"] != "tree"
            || retained.capture.file["snapshot"]["revision"] != world.base_tree
        {
            missing.push(format!("initial {path} must belong to pinned HEAD"));
        }
        missing.extend(attribution(path, &retained.capture, &world.before));
    }
    missing
}

pub(super) fn final_missing(world: &QuotaWorld, state: &Continuation) -> Vec<String> {
    let binding = if matches!(world.variant, Variant::Binding) {
        NEW_BINDING
    } else {
        OLD_BINDING
    };
    let helper = if world.variant.expects_warning() {
        CEIL
    } else {
        FLOOR
    };
    let helper_path = if matches!(world.variant, Variant::Binding) {
        ALTERNATE_PATH
    } else {
        HELPER_PATH
    };
    let packet: Vec<_> = [
        (ENTRY_PATH, ENTRY),
        (POLICY_PATH, binding),
        (POLICY_PATH, POLICY_BODY),
        (helper_path, helper),
    ]
    .into_iter()
    .map(|(path, source)| RequiredFragment {
        path,
        source: source.trim_end_matches('\n').to_owned(),
    })
    .collect();
    let mut missing = packet_missing(
        &packet,
        state.retained.values().chain(state.reused_pieces.iter()),
    );
    if !complete_changes(&state.change) {
        missing.push("complete public working change/identity coverage".to_owned());
    }
    if state.change["change"]["base"]["revision"] != world.base_tree {
        missing.push("change base must match the retained HEAD tree".to_owned());
    }
    let actual_changed: BTreeSet<_> = world
        .after
        .iter()
        .filter(|(path, source)| world.before.get(*path) != Some(*source))
        .map(|(path, _)| path.as_str())
        .collect();
    let reported_changed: BTreeSet<_> = state.change["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|file| file["snapshot"]["kind"] == "worktree")
        .filter_map(|file| file["path"].as_str())
        .collect();
    if actual_changed != reported_changed {
        missing.push("public change inventory does not justify all retained files".to_owned());
    }
    for retained in state.retained.values().chain(state.reused_pieces.iter()) {
        let path = text(&retained.capture.file, "path");
        if retained.capture.file["snapshot"]["kind"] == "worktree" {
            missing.extend(attribution(path, &retained.capture, &world.after));
        } else {
            missing.extend(attribution(path, &retained.capture, &world.before));
            if !reuse_justified(world, path, retained) {
                missing.push(format!(
                    "unchanged current fragment identity is unproved: {path}"
                ));
            }
        }
    }
    for report in &state.deliveries {
        for source in report["sources"].as_array().into_iter().flatten() {
            let body = text(source, "content");
            if body.contains(ENTRY.trim_end_matches('\n'))
                || body.contains(POLICY_BODY.trim_end_matches('\n'))
            {
                missing.push(
                    "follow-up delivered a complete unchanged entrypoint or policy body again"
                        .to_owned(),
                );
            }
        }
    }
    if matches!(world.variant, Variant::Helper) && state.stale.is_empty() {
        missing.push("stale retained-hash handoff was not rejected before refresh".to_owned());
    }
    missing
}

fn packet_missing<'a>(
    packet: &[RequiredFragment],
    retained: impl Iterator<Item = &'a Retained>,
) -> Vec<String> {
    let retained: Vec<_> = retained.collect();
    packet
        .iter()
        .filter_map(|required| {
            let supplied = retained.iter().any(|retained| {
                retained.capture.file["path"] == required.path
                    && text(&retained.capture.source, "content")
                        .contains(required.source.trim_end_matches('\n'))
            });
            (!supplied).then(|| {
                format!(
                    "necessary attributed source: {} ({})",
                    required.path,
                    required.source.lines().next().unwrap_or("empty")
                )
            })
        })
        .collect()
}

pub(super) fn attribution(
    path: &str,
    captured: &Capture,
    authored: &BTreeMap<String, String>,
) -> Vec<String> {
    let Some(file_source) = authored.get(path) else {
        return vec![format!("unrecognized source identity: {path}")];
    };
    let hash = sha256(file_source);
    let span = &captured.source["span"];
    let source = captured.source["content"].as_str();
    let start = span["start_byte"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok());
    let end = span["end_byte"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok());
    let mut missing = Vec::new();
    if captured.file["path"] != path
        || captured.file["sha256"] != hash
        || captured.source["file"] != captured.file["id"]
    {
        missing.push(format!("wrong captured path/hash/chunk identity: {path}"));
    }
    match (start, end) {
        (Some(start), Some(end)) if start < end && file_source.get(start..end) == source => {
            let first = 1 + file_source[..start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count();
            let last = 1 + file_source[..end - 1]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count();
            if span["start_line"].as_u64() != Some(first as u64)
                || span["end_line"].as_u64() != Some(last as u64)
            {
                missing.push(format!("wrong captured source-line span: {path}"));
            }
        }
        _ => missing.push(format!(
            "source bytes do not belong to the captured span: {path}"
        )),
    }
    if let Some(origin) = &captured.quote_origin {
        let original_start = origin["span"]["start_byte"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok());
        let original_end = origin["span"]["end_byte"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok());
        match (original_start, original_end, start, end) {
            (Some(first), Some(last), Some(start), Some(end))
                if first <= start
                    && end <= last
                    && origin["id"] == captured.source["id"]
                    && origin["file"] == captured.file["id"]
                    && file_source.get(first..last) == origin["content"].as_str() => {}
            _ => missing.push(format!(
                "retained quote lost its original public chunk provenance: {path}"
            )),
        }
    }
    missing
}

fn reuse_justified(world: &QuotaWorld, path: &str, retained: &Retained) -> bool {
    let Some(proof) = &retained.current_proof else {
        return false;
    };
    if !complete_changes(proof) || proof["change"]["base"]["revision"] != world.base_tree {
        return false;
    }
    let current = proof["files"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|file| file["path"] == path && file["snapshot"]["kind"] == "worktree");
    let Some(current) = current else {
        return world.before.get(path) == world.after.get(path);
    };
    let base = proof["files"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|file| file["path"] == path && file["snapshot"]["kind"] == "tree");
    if base.is_none_or(|base| {
        base["sha256"] != retained.capture.file["sha256"]
            || base["snapshot"]["revision"] != world.base_tree
    }) {
        return false;
    }
    let Some(after_source) = world.after.get(path) else {
        return false;
    };
    if current["sha256"] != sha256(after_source) {
        return false;
    }
    for side in ["base", "current"] {
        let ranges = changed_ranges(proof, path, side);
        if ranges.is_empty()
            || ranges
                .iter()
                .any(|range| !disjoint(range, &retained.capture.source["span"]))
        {
            return false;
        }
    }
    let span = &retained.capture.source["span"];
    let Some(start) = span["start_byte"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    let Some(end) = span["end_byte"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    after_source.get(start..end) == retained.capture.source["content"].as_str()
}
