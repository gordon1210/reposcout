//! Fixture facts are visible only to this evaluator, never to the CLI driver.

use super::driver::Source;
use super::{Evidence, World, fixture};
use git2::{Oid, Repository};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::Path;

/// The oracle must accept independently assembled evidence before any product output exists.
pub(super) fn assert_frozen_packet(world: &World) {
    let repository = Repository::open(world.fixture.path()).unwrap();
    let tree_for = |revision: &str| {
        repository
            .find_commit(Oid::from_str(revision).unwrap())
            .unwrap()
            .tree_id()
            .to_string()
    };
    let base_tree = tree_for(&world.base);
    let head_tree = tree_for(&world.head);
    let mut evidence = Evidence {
        review: json!({"comparison": {
            "base_commit": world.base, "head_commit": world.head,
            "base_tree": base_tree, "head_tree": head_tree,
        }, "relations": []}),
        sources: Vec::new(),
    };
    for (tree, path, body) in [
        (&head_tree, "shop/routes.py", fixture::ROUTES),
        (&head_tree, "shop/handler.py", fixture::HANDLER),
        (&head_tree, "shop/amounts.py", fixture::CALCULATOR),
        (&base_tree, "shop/presentation.py", world.base_serializer),
        (&head_tree, "shop/presentation.py", world.head_serializer),
        (&head_tree, "checks/test_checkout.py", world.head_checks),
    ] {
        let mut hash = String::new();
        for byte in Sha256::digest(body.as_bytes()) {
            write!(hash, "{byte:02x}").unwrap();
        }
        evidence.sources.push(Source {
            tree: tree.clone(),
            path: path.to_owned(),
            hash,
            body: body.to_owned(),
            span: json!({"start_byte": 0, "end_byte": body.len(),
                "start_line": 1, "end_line": body.lines().count()}),
            snapshot_kind: "tree".to_owned(),
        });
    }
    assert_attribution(world, &evidence);
    if world.head_checks == fixture::COUNTERFEIT {
        assert_eq!(
            missing(world, &evidence),
            ["genuine dispatcher-level request assertions"]
        );
        return;
    }
    assert!(
        missing(world, &evidence).is_empty(),
        "frozen sufficient packet is insufficient"
    );
    for index in 0..evidence.sources.len() {
        let removed = evidence.sources.remove(index);
        assert!(
            !missing(world, &evidence).is_empty(),
            "omitting necessary evidence from {} must invalidate the packet",
            removed.path
        );
        evidence.sources.insert(index, removed);
    }
}

pub(super) fn missing(world: &World, evidence: &Evidence) -> Vec<&'static str> {
    [
        (
            "head",
            "shop/routes.py",
            fixture::ROUTES,
            "active endpoint registration and dispatch",
        ),
        (
            "head",
            "shop/handler.py",
            fixture::HANDLER,
            "handler calculator-to-serializer bindings",
        ),
        (
            "head",
            "shop/amounts.py",
            fixture::CALCULATOR,
            "integer-cent calculation",
        ),
        (
            "base",
            "shop/presentation.py",
            world.base_serializer,
            "base serializer",
        ),
        (
            "head",
            "shop/presentation.py",
            world.head_serializer,
            "head serializer",
        ),
        (
            "head",
            "checks/test_checkout.py",
            fixture::CONTRACT,
            "genuine dispatcher-level request assertions",
        ),
    ]
    .into_iter()
    .filter_map(|(side, path, source, obligation)| {
        let fragments = source
            .lines()
            .map(str::trim)
            .filter(|line| {
                !line.is_empty() && *line != "import unittest" && !line.starts_with("class ")
            })
            .collect::<Vec<_>>();
        (!evidence.contains(side, path, &fragments)).then_some(obligation)
    })
    .collect()
}

pub(super) fn assert_attribution(world: &World, evidence: &Evidence) {
    let repository = Repository::open(world.fixture.path()).unwrap();
    let mut trees = Vec::new();
    for (side, revision) in [("base", &world.base), ("head", &world.head)] {
        let commit = repository
            .find_commit(Oid::from_str(revision).unwrap())
            .unwrap();
        let tree = commit.tree_id().to_string();
        assert_eq!(
            evidence.review["comparison"][format!("{side}_commit")],
            *revision
        );
        assert_eq!(evidence.review["comparison"][format!("{side}_tree")], tree);
        trees.push(tree);
    }
    for source in &evidence.sources {
        assert_source(&repository, &trees, source);
    }
    let test_target = if world.head_checks == fixture::COUNTERFEIT {
        "shop/amounts.py"
    } else {
        "shop/routes.py"
    };
    for relation in evidence.review["relations"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let edge = &relation["edge"];
        let (Some(source), Some(target)) = (edge["source"].as_str(), edge["target"].as_str())
        else {
            continue;
        };
        let checks = if relation["side"] == "head" {
            test_target
        } else {
            "shop/routes.py"
        };
        assert!(
            matches!(
                (source, target),
                (
                    "shop/handler.py",
                    "shop/amounts.py" | "shop/presentation.py"
                ) | ("shop/routes.py", "shop/handler.py")
            ) || (source == "checks/test_checkout.py" && target == checks),
            "reported import relation is not supported by the actual checkout wiring: {relation}"
        );
    }
}

fn assert_source(repository: &Repository, trees: &[String], source: &Source) {
    assert_eq!(source.snapshot_kind, "tree");
    assert!(
        trees.contains(&source.tree),
        "source came from an unrequested revision"
    );
    let tree = repository
        .find_tree(Oid::from_str(&source.tree).unwrap())
        .unwrap();
    let entry = tree.get_path(Path::new(&source.path)).unwrap();
    let blob = repository.find_blob(entry.id()).unwrap();
    let mut hash = String::new();
    for byte in Sha256::digest(blob.content()) {
        write!(hash, "{byte:02x}").unwrap();
    }
    assert_eq!(
        source.hash, hash,
        "wrong source identity for {}",
        source.path
    );
    let start = usize::try_from(source.span["start_byte"].as_u64().unwrap()).unwrap();
    let end = usize::try_from(source.span["end_byte"].as_u64().unwrap()).unwrap();
    let actual = std::str::from_utf8(blob.content()).unwrap();
    assert_eq!(
        actual.get(start..end),
        Some(source.body.as_str()),
        "source range does not belong to {}",
        source.path
    );
    let line = |byte: usize| {
        1 + actual
            .bytes()
            .take(byte)
            .filter(|byte| *byte == b'\n')
            .count()
    };
    assert_eq!(source.span["start_line"].as_u64(), Some(line(start) as u64));
    assert_eq!(
        source.span["end_line"].as_u64(),
        Some(line(end.saturating_sub(1)) as u64)
    );
}
