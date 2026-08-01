//! JSON output — the stable, agent-friendly representation.

use crate::model::ScanReport;
use anyhow::{Context, Result};

/// Render the report as compact JSON unless `pretty` is requested. When
/// `summary_only` is set, the heavy
/// default per-file (`files`) and duplicate (`duplicates`) arrays are dropped.
/// Explicitly requested analysis blocks remain present: compactness must not
/// silently erase the answer to a graph, impact, or directory query.
pub fn render(
    report: &ScanReport,
    summary_only: bool,
    baseline_ready: bool,
    pretty: bool,
) -> Result<String> {
    if summary_only || baseline_ready {
        let mut value = serde_json::to_value(report).context("failed to serialize scan report")?;
        if let Some(obj) = value.as_object_mut() {
            obj.remove("files");
            obj.remove("duplicates");
            if baseline_ready {
                obj.remove("directories");
                obj.remove("graph");
                obj.remove("impact");
                obj.remove("baseline");
                obj.remove("review");
                obj.remove("context");
                obj.remove("work_scope");
            }
            if summary_only && !baseline_ready {
                obj.remove("finding_catalog");
                if let Some(context) = obj.get_mut("context") {
                    super::projection::strip_context_outline_details(context);
                }
            }
        }
        return super::json_string(&value, pretty).context("failed to render scan report JSON");
    }
    super::json_string(report, pretty).context("failed to render scan report JSON")
}
