use super::fixture::{InvoiceCase, Variant};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Evaluation {
    pub(super) missing: Vec<String>,
    pub(super) wrong_identity: Vec<String>,
    pub(super) observed_counterfeit: bool,
    pub(super) cli_errors: Vec<Value>,
}

impl Evaluation {
    pub(super) fn json(&self) -> Value {
        json!({"missing_obligations": self.missing, "wrong_identity": self.wrong_identity,
            "counterfeit_test_observed": self.observed_counterfeit, "cli_errors": self.cli_errors})
    }
}

/// This consumes all delivered source; filtering never reduces the separate cost ledger.
pub(super) fn evaluate(case: &InvoiceCase, reports: &[Value]) -> Evaluation {
    let mut evaluation = Evaluation::default();
    let mut fragments: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for report in reports {
        if let Some(error) = report.get("driver_cli_error") {
            evaluation.cli_errors.push(error.clone());
        }
        for hit in array(&report["hits"]) {
            if let Some(signature) = hit["signature"].as_str()
                && !valid_signature(
                    case,
                    &hit["path"],
                    &hit["sha256"],
                    signature,
                    &hit["declaration_span"],
                )
            {
                evaluation.wrong_identity.push(format!(
                    "incorrect search signature identity: {}",
                    hit["path"]
                ));
            }
        }
        let source = report.get("source").unwrap_or(report);
        let files = array(&source["files"]);
        for result in array(&source["results"]) {
            for definition in result
                .get("definition")
                .into_iter()
                .chain(array(&result["candidates"]).iter())
            {
                if let Some(signature) = definition["signature"].as_str() {
                    let file = files.iter().find(|file| file["id"] == result["file"]);
                    if !file.is_some_and(|file| {
                        file["snapshot"]["kind"] == "worktree"
                            && valid_signature(
                                case,
                                &file["path"],
                                &file["sha256"],
                                signature,
                                &definition["declaration_span"],
                            )
                    }) {
                        evaluation.wrong_identity.push(format!(
                            "incorrect read signature identity: {}",
                            result["file"]
                        ));
                    }
                }
            }
        }
        for chunk in array(&source["sources"]) {
            let file = files.iter().find(|file| file["id"] == chunk["file"]);
            match file.and_then(|file| validated_chunk(case, file, chunk)) {
                Some((path, content)) => fragments.entry(path).or_default().push(content),
                None => evaluation.wrong_identity.push(format!(
                    "source chunk {} has missing, stale or incorrect file/span identity",
                    chunk["id"]
                )),
            }
        }
    }
    for report in reports {
        for edge in array(&report["hits"])
            .iter()
            .flat_map(|hit| array(&hit["evidence"]))
        {
            if edge["syntax"] == "imported-binding"
                && edge["source"]["path"].as_str().is_some_and(|path| {
                    matches!(path, "src/invoices.py" | "tests/test_invoices.py")
                })
                && !authored_binding(case, edge)
            {
                evaluation.wrong_identity.push(format!(
                    "concrete binding contradicts authored active import: {} -> {}",
                    edge["source"]["path"], edge["target"]["path"]
                ));
            }
        }
    }
    assess_obligations(case, reports, &fragments, &mut evaluation);
    evaluation
}

fn assess_obligations(
    case: &InvoiceCase,
    reports: &[Value],
    fragments: &BTreeMap<String, Vec<String>>,
    evaluation: &mut Evaluation,
) {
    for required in &case.required {
        if !contains(fragments, required.fragment.path, required.fragment.source)
            && !binding_substitute(case, reports, fragments, required.obligation)
        {
            evaluation.missing.push(required.obligation.to_owned());
        }
    }
    evaluation.observed_counterfeit =
        fragments
            .get("tests/test_invoices.py")
            .is_some_and(|pieces| {
                pieces.iter().any(|source| {
                    source.contains("from demo.invoices import net_due")
                        && source.contains(
                            "assert net_due(Invoice(amount_cents=1005, discount_bps=1000)) == 904",
                        )
                })
            });
}

fn valid_signature(
    case: &InvoiceCase,
    path: &Value,
    hash: &Value,
    signature: &str,
    span: &Value,
) -> bool {
    let Some(path) = path.as_str().filter(|path| case.has_path(path)) else {
        return false;
    };
    if *hash != case.hash(path) {
        return false;
    }
    let start = span["start_byte"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok());
    let end = span["end_byte"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok());
    let header = signature.strip_suffix(" …").unwrap_or(signature);
    !header.is_empty()
        && start
            .zip(end)
            .and_then(|(start, end)| case.source(path).get(start..end))
            .is_some_and(|source| source.contains(header))
}

pub(super) fn assert_packet_sensitivity(case: &InvoiceCase) {
    if case.variant == Variant::Counterfeit {
        return;
    }
    let fragments: BTreeMap<String, Vec<String>> =
        case.required
            .iter()
            .fold(BTreeMap::new(), |mut entries, piece| {
                entries
                    .entry(piece.fragment.path.to_owned())
                    .or_default()
                    .push(piece.fragment.source.to_owned());
                entries
            });
    let mut complete = Evaluation::default();
    assess_obligations(case, &[], &fragments, &mut complete);
    assert!(
        complete.missing.is_empty(),
        "attributable frozen packet is sufficient: {}",
        complete.json()
    );
    for removed in &case.required {
        let mut reduced = fragments.clone();
        reduced
            .get_mut(removed.fragment.path)
            .unwrap()
            .retain(|source| source != removed.fragment.source);
        let mut assessment = Evaluation::default();
        assess_obligations(case, &[], &reduced, &mut assessment);
        assert!(
            assessment
                .missing
                .iter()
                .any(|item| item == removed.obligation),
            "the actual evaluator must reject omitted {}: {}",
            removed.obligation,
            assessment.json()
        );
    }
}

fn validated_chunk(case: &InvoiceCase, file: &Value, chunk: &Value) -> Option<(String, String)> {
    let path = file["path"].as_str()?;
    if !case.has_path(path)
        || file["snapshot"]["kind"] != "worktree"
        || file["sha256"] != case.hash(path)
    {
        return None;
    }
    let content = chunk["content"].as_str()?;
    let start = usize::try_from(chunk["span"]["start_byte"].as_u64()?).ok()?;
    let end = usize::try_from(chunk["span"]["end_byte"].as_u64()?).ok()?;
    let authored = case.source(path);
    if end <= start || authored.get(start..end)? != content {
        return None;
    }
    let start_line = authored[..start]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    let end_line = authored[..end - 1]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1;
    if chunk["span"]["start_line"].as_u64()? != u64::try_from(start_line).ok()?
        || chunk["span"]["end_line"].as_u64()? != u64::try_from(end_line).ok()?
    {
        return None;
    }
    Some((path.to_owned(), content.to_owned()))
}

fn contains(fragments: &BTreeMap<String, Vec<String>>, path: &str, expected: &str) -> bool {
    // A complete declaration conventionally ends before its following line separator.
    let expected = expected.trim_end_matches('\n');
    fragments
        .get(path)
        .is_some_and(|pieces| pieces.iter().any(|piece| piece.contains(expected)))
}

fn binding_substitute(
    case: &InvoiceCase,
    reports: &[Value],
    fragments: &BTreeMap<String, Vec<String>>,
    obligation: &str,
) -> bool {
    // Public concrete relation records can prove bindings. File-import graph edges and
    // lexical matches lack an attributable site and cannot pass this check.
    let helper = &case
        .required
        .iter()
        .find(|piece| piece.obligation == "active rounding change site")
        .unwrap()
        .fragment;
    if obligation == "active module binding" {
        precise_relation(
            case,
            reports,
            "src/invoices.py",
            helper.path,
            "call",
            "discount_cent(",
        ) && precise_relation(
            case,
            reports,
            "src/invoices.py",
            "src/models.py",
            "reference",
            "Invoice",
        )
    } else if obligation == "production regression" {
        let body = case
            .required
            .iter()
            .find(|piece| piece.obligation == obligation)
            .unwrap()
            .fragment
            .source
            .split_once("def test_net_due_rounds_half_up")
            .unwrap()
            .1;
        contains(
            fragments,
            "tests/test_invoices.py",
            &format!("def test_net_due_rounds_half_up{body}"),
        ) && precise_relation(
            case,
            reports,
            "tests/test_invoices.py",
            "src/invoices.py",
            "call",
            "net_due(",
        ) && precise_relation(
            case,
            reports,
            "tests/test_invoices.py",
            "src/models.py",
            "reference",
            "Invoice",
        )
    } else {
        false
    }
}

fn precise_relation(
    case: &InvoiceCase,
    reports: &[Value],
    source_path: &str,
    target_path: &str,
    kind: &str,
    spelling: &str,
) -> bool {
    reports
        .iter()
        .flat_map(|report| array(&report["hits"]))
        .flat_map(|hit| array(&hit["evidence"]))
        .any(|edge| {
            if edge["kind"] != kind
                || edge["syntax"] != "imported-binding"
                || edge["source"]["path"] != source_path
                || edge["target"]["path"] != target_path
                || edge["source"]["source_hash"] != case.hash(source_path)
                || edge["target"]["source_hash"] != case.hash(target_path)
                || !authored_binding(case, edge)
            {
                return false;
            }
            let start = edge["site"]["start_byte"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok());
            let end = edge["site"]["end_byte"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok());
            start
                .zip(end)
                .and_then(|(start, end)| case.source(source_path).get(start..end))
                .is_some_and(|site| site.starts_with(spelling))
        })
}

fn authored_binding(case: &InvoiceCase, edge: &Value) -> bool {
    let Some(source) = edge["source"]["path"].as_str() else {
        return false;
    };
    let Some(target) = edge["target"]["path"].as_str() else {
        return false;
    };
    match (source, target) {
        ("src/invoices.py" | "tests/test_invoices.py", "src/models.py") => case
            .source(source)
            .contains("from src.models import Invoice"),
        ("tests/test_invoices.py", "src/invoices.py") => case
            .source(source)
            .contains("from src.invoices import net_due"),
        ("src/invoices.py", helper) => {
            let required = case
                .required
                .iter()
                .find(|piece| piece.obligation == "active rounding change site")
                .unwrap();
            let binding = case
                .required
                .iter()
                .find(|piece| piece.obligation == "active module binding")
                .unwrap();
            helper == required.fragment.path
                && case
                    .source(source)
                    .contains(binding.fragment.source.trim_end())
        }
        _ => false,
    }
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}
