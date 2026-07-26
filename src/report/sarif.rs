//! SARIF 2.1.0 output — surfaces duplicates, high-complexity functions, and
//! orphan files as static-analysis results for CI / code-scanning consumers.

use crate::model::{FindingLocation, ReviewReport, ScanReport};
use crate::report::{sarif_uri, sarif_uri_text, similarity_label};
use anyhow::{Context, Result};
use serde_json::{Value, json};

pub fn render(report: &ScanReport) -> Result<String> {
    if let Some(review) = &report.review {
        return render_review(review);
    }

    let mut results: Vec<Value> = Vec::new();

    // 1. Pair-oriented duplicate-code results with precise regions.
    for finding in &report.duplicates.findings {
        let a_uri = sarif_uri(&finding.fragment_a.path)?;
        let b_uri = sarif_uri(&finding.fragment_b.path)?;
        results.push(json!({
            "ruleId": "reposcout/duplicate-code",
            "level": if finding.kind == "exact" { "warning" } else { "note" },
            "message": {
                "text": format!(
                    "{} duplicate of {} tokens (similarity {})",
                    finding.kind,
                    finding.tokens,
                    similarity_label(finding.similarity)
                )
            },
            "locations": [{
                "physicalLocation": {
                    "artifactLocation": {"uri": a_uri},
                    "region": {
                        "startLine": finding.fragment_a.start_line,
                        "startColumn": finding.fragment_a.start_column,
                        "endLine": finding.fragment_a.end_line,
                        "endColumn": finding.fragment_a.end_column
                    }
                }
            }],
            "relatedLocations": [{
                "id": 1,
                "message": {"text": "Matching duplicate fragment"},
                "physicalLocation": {
                    "artifactLocation": {"uri": b_uri},
                    "region": {
                        "startLine": finding.fragment_b.start_line,
                        "startColumn": finding.fragment_b.start_column,
                        "endLine": finding.fragment_b.end_line,
                        "endColumn": finding.fragment_b.end_column
                    }
                }
            }],
            "properties": {
                "findingId": finding.id,
                "familyId": finding.family_id,
                "format": finding.format,
                "tokens": finding.tokens,
                "similarity": finding.similarity
            }
        }));
    }

    // 2. Per-function cyclomatic-complexity rule violations.
    let maximum = report.summary.complexity.cyclomatic_threshold;
    for file in &report.files {
        let Some(complexity) = &file.complexity else {
            continue;
        };
        for function in complexity
            .functions
            .iter()
            .filter(|function| function.cyclomatic > maximum)
        {
            let uri = sarif_uri(&file.path)?;
            results.push(json!({
                "ruleId": "reposcout/high-complexity-function",
                "level": "warning",
                "message": {
                    "text": format!(
                        "Function `{}` has cyclomatic complexity {}, exceeding the maximum of {}",
                        function.name,
                        function.cyclomatic,
                        maximum
                    )
                },
                "locations": [
                    {
                        "physicalLocation": {
                            "artifactLocation": {"uri": uri},
                            "region": {"startLine": function.line}
                        }
                    }
                ],
                "properties": {
                    "cyclomatic": function.cyclomatic,
                    "cognitive": function.cognitive,
                    "maxNesting": function.max_nesting,
                    "maximum": maximum
                }
            }));
        }
    }

    // 3. Orphan-file results
    if let Some(graph) = &report.graph {
        for path in &graph.orphans {
            let uri = sarif_uri_text(path)?;
            results.push(json!({
                "ruleId": "reposcout/orphan-file",
                "level": "note",
                "message": {
                    "text": "File has no internal importers (dead-code candidate)"
                },
                "locations": [
                    {
                        "physicalLocation": {
                            "artifactLocation": {"uri": uri}
                        }
                    }
                ]
            }));
        }
    }

    let doc = json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": "reposcout",
                        "version": env!("CARGO_PKG_VERSION"),
                        "rules": [
                            {
                                "id": "reposcout/duplicate-code",
                                "name": "DuplicateCode",
                                "shortDescription": {"text": "Duplicated code block"}
                            },
                            {
                                "id": "reposcout/high-complexity-function",
                                "name": "HighComplexityFunction",
                                "shortDescription": {"text": "Function exceeds the configured cyclomatic complexity maximum"}
                            },
                            {
                                "id": "reposcout/orphan-file",
                                "name": "OrphanFile",
                                "shortDescription": {"text": "File with no internal importers (dead-code candidate)"}
                            }
                        ]
                    }
                },
                "columnKind": "unicodeCodePoints",
                "results": results
            }
        ]
    });

    serde_json::to_string_pretty(&doc).context("failed to render SARIF report")
}

fn render_review(review: &ReviewReport) -> Result<String> {
    let mut results = Vec::with_capacity(review.findings.len());
    for item in &review.findings {
        let finding = item.after.as_ref().unwrap_or(&item.finding);
        let location = sarif_location(&finding.primary_location)?;
        let mut related_locations = Vec::with_capacity(finding.related_locations.len());
        for (index, related) in finding.related_locations.iter().enumerate() {
            related_locations.push(json!({
                "id": index + 1,
                "message": {"text": "Related finding location"},
                "physicalLocation": sarif_physical_location(related)?
            }));
        }
        let mut properties = serde_json::Map::new();
        properties.insert(
            "fingerprint".to_string(),
            Value::String(finding.fingerprint.clone()),
        );
        properties.insert("state".to_string(), Value::String(item.state.clone()));
        let metrics = serde_json::to_value(&finding.metrics)
            .context("failed to serialize SARIF finding metrics")?;
        if let Value::Object(metrics) = metrics {
            properties.extend(metrics);
        }

        let mut result = json!({
            "ruleId": finding_rule_id(&finding.kind),
            "level": sarif_level(&finding.severity),
            "message": {"text": finding.message},
            "locations": [location],
            "properties": properties
        });
        if !related_locations.is_empty() {
            result["relatedLocations"] = Value::Array(related_locations);
        }
        if let Some(state) = baseline_state(&item.state) {
            result["baselineState"] = Value::String(state.to_string());
        }
        results.push(result);
    }

    let doc = json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {"driver": {
                "name": "reposcout",
                "version": env!("CARGO_PKG_VERSION"),
                "rules": [
                    sarif_rule("reposcout/duplicate-code", "DuplicateCode", "Duplicated code block"),
                    sarif_rule("reposcout/high-complexity-function", "HighComplexityFunction", "Function exceeds the configured cyclomatic complexity maximum"),
                    sarif_rule("reposcout/source-marker", "SourceMarker", "Action marker in source code"),
                    sarif_rule("reposcout/high-risk-file", "HighRiskFile", "File exceeds the composite risk threshold")
                ]
            }},
            "columnKind": "unicodeCodePoints",
            "properties": {
                "reviewMode": review.mode,
                "reviewScope": review.scope,
                "reviewCounts": review.counts
            },
            "results": results
        }]
    });
    serde_json::to_string_pretty(&doc).context("failed to render review SARIF report")
}

fn sarif_rule(id: &str, name: &str, description: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "shortDescription": {"text": description}
    })
}

fn finding_rule_id(kind: &str) -> &'static str {
    match kind {
        "complexity" => "reposcout/high-complexity-function",
        "marker" => "reposcout/source-marker",
        "duplication" => "reposcout/duplicate-code",
        "risk" => "reposcout/high-risk-file",
        _ => "reposcout/finding",
    }
}

fn sarif_level(severity: &str) -> &'static str {
    match severity {
        "error" => "error",
        "warning" => "warning",
        _ => "note",
    }
}

fn baseline_state(state: &str) -> Option<&'static str> {
    match state {
        "new" => Some("new"),
        "resolved" => Some("absent"),
        "worsened" | "improved" => Some("updated"),
        _ => None,
    }
}

fn sarif_location(location: &FindingLocation) -> Result<Value> {
    Ok(json!({"physicalLocation": sarif_physical_location(location)?}))
}

fn sarif_physical_location(location: &FindingLocation) -> Result<Value> {
    let mut region = serde_json::Map::new();
    region.insert(
        "startLine".to_string(),
        Value::from(location.start_line.max(1)),
    );
    region.insert(
        "endLine".to_string(),
        Value::from(location.end_line.max(location.start_line).max(1)),
    );
    if let Some(column) = location.start_column {
        region.insert("startColumn".to_string(), Value::from(column.max(1)));
    }
    if let Some(column) = location.end_column {
        region.insert("endColumn".to_string(), Value::from(column.max(1)));
    }
    Ok(json!({
        "artifactLocation": {
            "uri": sarif_uri(&location.path)?
        },
        "region": region
    }))
}
