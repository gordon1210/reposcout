//! NDJSON output — one JSON record per line, streamable by agents/tools.

use crate::model::ScanReport;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;

/// Emit the report as newline-delimited JSON. The first line is the aggregate
/// `summary` (kind = "summary"); unless `summary_only`, one line per file
/// follows (kind = "file"), then one precise duplicate finding per line
/// (kind = "finding"). Compact, one object per line.
pub fn render(report: &ScanReport, summary_only: bool) -> Result<String> {
    let mut lines: Vec<String> = Vec::new();

    let Value::Object(mut map) =
        serde_json::to_value(&report.summary).context("failed to serialize NDJSON summary")?
    else {
        return Err(anyhow!("NDJSON summary did not serialize as an object"));
    };
    map.insert(
        "schema_version".to_string(),
        Value::String(report.schema_version.clone()),
    );
    map.insert("root".to_string(), serde_json::to_value(&report.root)?);
    map.insert("target".to_string(), serde_json::to_value(&report.target)?);
    map.insert(
        "generated_at".to_string(),
        Value::String(report.generated_at.clone()),
    );
    map.insert(
        "encoding".to_string(),
        Value::String(report.encoding.clone()),
    );
    map.insert(
        "analysis_profile".to_string(),
        serde_json::to_value(&report.analysis_profile)?,
    );
    map.insert(
        "diagnostics".to_string(),
        serde_json::to_value(&report.diagnostics)?,
    );
    if let Some(impact) = &report.impact {
        map.insert("impact".to_string(), serde_json::to_value(impact)?);
    }
    if let Some(graph) = &report.graph {
        map.insert("graph".to_string(), serde_json::to_value(graph)?);
    }
    if let Some(context) = &report.context {
        map.insert("context".to_string(), serde_json::to_value(context)?);
    }
    if let Some(baseline) = &report.baseline {
        map.insert("baseline".to_string(), serde_json::to_value(baseline)?);
    }
    if let Some(review) = &report.review {
        let mut metadata = review.clone();
        metadata.findings.clear();
        map.insert("review".to_string(), serde_json::to_value(metadata)?);
    }
    map.insert("kind".to_string(), Value::String("summary".to_string()));
    lines.push(
        serde_json::to_string(&Value::Object(map))
            .context("failed to render NDJSON summary record")?,
    );

    if !summary_only {
        for file in &report.files {
            let Value::Object(mut map) = serde_json::to_value(file)? else {
                return Err(anyhow!("NDJSON file did not serialize as an object"));
            };
            map.insert("kind".to_string(), Value::String("file".to_string()));
            lines.push(serde_json::to_string(&Value::Object(map))?);
        }
        for finding in &report.duplicates.findings {
            let Value::Object(mut map) = serde_json::to_value(finding)? else {
                return Err(anyhow!(
                    "NDJSON duplicate finding did not serialize as an object"
                ));
            };
            map.insert(
                "finding_kind".to_string(),
                Value::String(finding.kind.clone()),
            );
            map.insert("kind".to_string(), Value::String("finding".to_string()));
            lines.push(serde_json::to_string(&Value::Object(map))?);
        }
        if let Some(review) = &report.review {
            for finding in &review.findings {
                let Value::Object(mut map) = serde_json::to_value(finding)? else {
                    return Err(anyhow!(
                        "NDJSON review finding did not serialize as an object"
                    ));
                };
                map.insert(
                    "kind".to_string(),
                    Value::String("review_finding".to_string()),
                );
                lines.push(serde_json::to_string(&Value::Object(map))?);
            }
        }
    }

    Ok(lines.join("\n"))
}
