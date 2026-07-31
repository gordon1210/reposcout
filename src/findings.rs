//! Canonical actionable findings shared by review, baselines, and explain.

use crate::config::Config;
use crate::model::{
    CloneGroup, Duplication, FileReport, FindingCatalog, FindingChange, FindingChangeCounts,
    FindingDelta, FindingLocation, FindingRecord, RiskEntry,
};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::xxh3_128;

pub const CATALOG_VERSION: u32 = 1;
pub const RISK_ALGORITHM_VERSION: u32 = crate::metrics::risk::ALGORITHM_VERSION;
pub const RISK_THRESHOLD: f64 = 0.7;

/// Project analyzer outputs into one complete, deterministically ordered
/// finding catalog.
pub fn build(
    files: &[FileReport],
    duplication: &Duplication,
    risks: &[RiskEntry],
    cfg: &Config,
) -> FindingCatalog {
    let mut findings = Vec::new();

    for file in files {
        if let Some(complexity) = &file.complexity {
            for function in &complexity.functions {
                if function.cyclomatic <= cfg.max_complexity {
                    continue;
                }
                let symbol_key = if function.symbol_key.is_empty() {
                    function.name.as_str()
                } else {
                    function.symbol_key.as_str()
                };
                findings.push(FindingRecord {
                    fingerprint: path_fingerprint("complexity", &file.path, symbol_key),
                    identity: symbol_key.to_string(),
                    kind: "complexity".to_string(),
                    severity: "warning".to_string(),
                    message: format!(
                        "Function `{}` has cyclomatic complexity {}, exceeding {}",
                        function.name, function.cyclomatic, cfg.max_complexity
                    ),
                    primary_location: FindingLocation {
                        path: file.path.clone(),
                        start_line: function.line,
                        end_line: function.end_line.max(function.line),
                        ..FindingLocation::default()
                    },
                    related_locations: Vec::new(),
                    metrics: BTreeMap::from([
                        ("cyclomatic".to_string(), function.cyclomatic as f64),
                        ("cognitive".to_string(), function.cognitive as f64),
                        ("max_nesting".to_string(), function.max_nesting as f64),
                        ("threshold".to_string(), cfg.max_complexity as f64),
                    ]),
                });
            }
        }

        for marker in &file.marker_occurrences {
            let occurrence = marker.occurrence.to_string();
            let identity = format!("{}|{}|{}", marker.marker, marker.context_hash, occurrence);
            findings.push(FindingRecord {
                fingerprint: path_fingerprint("marker", &file.path, &identity),
                identity,
                kind: "marker".to_string(),
                severity: marker_severity(&marker.marker).to_string(),
                message: format!("{} marker", marker.marker),
                primary_location: FindingLocation {
                    path: file.path.clone(),
                    start_line: marker.line,
                    end_line: marker.line,
                    start_column: Some(marker.column),
                    end_column: Some(marker.column + marker.marker.chars().count()),
                },
                related_locations: Vec::new(),
                metrics: BTreeMap::new(),
            });
        }
    }

    add_duplicate_findings(&mut findings, "exact", &duplication.exact);
    add_duplicate_findings(&mut findings, "type2", &duplication.near);

    for risk in risks.iter().filter(|risk| risk.score >= RISK_THRESHOLD) {
        findings.push(FindingRecord {
            fingerprint: path_fingerprint("risk", Path::new(&risk.path), "file"),
            identity: "file".to_string(),
            kind: "risk".to_string(),
            severity: "warning".to_string(),
            message: format!("High-risk file ({:.2})", risk.score),
            primary_location: FindingLocation {
                path: PathBuf::from(&risk.path),
                start_line: 1,
                end_line: 1,
                ..FindingLocation::default()
            },
            related_locations: Vec::new(),
            metrics: BTreeMap::from([
                ("score".to_string(), risk.score),
                ("sloc".to_string(), risk.sloc as f64),
                ("cyclomatic".to_string(), risk.cyclomatic as f64),
                ("churn_commits".to_string(), risk.churn_commits as f64),
            ]),
        });
    }

    findings.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then(a.primary_location.path.cmp(&b.primary_location.path))
            .then(
                a.primary_location
                    .start_line
                    .cmp(&b.primary_location.start_line),
            )
            .then(a.fingerprint.cmp(&b.fingerprint))
    });
    FindingCatalog {
        version: CATALOG_VERSION,
        findings,
    }
}

/// Compare two compatible catalogs, returning only observable changes.
pub fn compare(before: &FindingCatalog, after: &FindingCatalog) -> FindingDelta {
    if before.version != after.version || before.version == 0 {
        return unavailable("finding catalog versions do not match");
    }

    let before_by_id = before
        .findings
        .iter()
        .map(|finding| (finding.fingerprint.as_str(), finding))
        .collect::<BTreeMap<_, _>>();
    let after_by_id = after
        .findings
        .iter()
        .map(|finding| (finding.fingerprint.as_str(), finding))
        .collect::<BTreeMap<_, _>>();
    let mut changes = Vec::new();

    for (fingerprint, current) in &after_by_id {
        match before_by_id.get(fingerprint) {
            None => changes.push(FindingChange {
                state: "new".to_string(),
                before: None,
                after: Some((*current).clone()),
            }),
            Some(previous) => match compare_severity(previous, current) {
                Ordering::Greater => changes.push(FindingChange {
                    state: "worsened".to_string(),
                    before: Some((**previous).clone()),
                    after: Some((*current).clone()),
                }),
                Ordering::Less => changes.push(FindingChange {
                    state: "improved".to_string(),
                    before: Some((**previous).clone()),
                    after: Some((*current).clone()),
                }),
                Ordering::Equal => {}
            },
        }
    }
    for (fingerprint, previous) in &before_by_id {
        if !after_by_id.contains_key(fingerprint) {
            changes.push(FindingChange {
                state: "resolved".to_string(),
                before: Some((*previous).clone()),
                after: None,
            });
        }
    }

    changes.sort_by(|a, b| {
        change_rank(&a.state)
            .cmp(&change_rank(&b.state))
            .then_with(|| change_finding(a).kind.cmp(&change_finding(b).kind))
            .then_with(|| {
                change_finding(a)
                    .primary_location
                    .path
                    .cmp(&change_finding(b).primary_location.path)
            })
            .then_with(|| {
                change_finding(a)
                    .primary_location
                    .start_line
                    .cmp(&change_finding(b).primary_location.start_line)
            })
            .then_with(|| {
                change_finding(a)
                    .fingerprint
                    .cmp(&change_finding(b).fingerprint)
            })
    });
    let counts = FindingChangeCounts {
        new: changes
            .iter()
            .filter(|change| change.state == "new")
            .count(),
        resolved: changes
            .iter()
            .filter(|change| change.state == "resolved")
            .count(),
        worsened: changes
            .iter()
            .filter(|change| change.state == "worsened")
            .count(),
        improved: changes
            .iter()
            .filter(|change| change.state == "improved")
            .count(),
    };
    FindingDelta {
        comparison: "complete".to_string(),
        reason: None,
        counts,
        changes,
    }
}

/// Rebase path-sensitive identities across Git-detected renames before a deep
/// review comparison. Saved baselines deliberately do not call this.
pub fn remap_renames(
    catalog: &FindingCatalog,
    changed_files: &[crate::model::ReviewChangedFile],
) -> FindingCatalog {
    let renames = changed_files
        .iter()
        .filter(|file| file.status == "renamed")
        .filter_map(|file| Some((file.old_path.as_ref()?, file.path.as_ref()?)))
        .collect::<BTreeMap<_, _>>();
    if renames.is_empty() {
        return catalog.clone();
    }

    let mut remapped = catalog.clone();
    for finding in &mut remapped.findings {
        remap_location(&mut finding.primary_location, &renames);
        for location in &mut finding.related_locations {
            remap_location(location, &renames);
        }
        if finding.kind != "duplication" {
            finding.fingerprint = path_fingerprint(
                &finding.kind,
                &finding.primary_location.path,
                &finding.identity,
            );
        }
    }
    remapped
        .findings
        .sort_by(|a, b| a.fingerprint.cmp(&b.fingerprint));
    remapped
}

pub fn unavailable(reason: impl Into<String>) -> FindingDelta {
    FindingDelta {
        comparison: "unavailable".to_string(),
        reason: Some(reason.into()),
        ..FindingDelta::default()
    }
}

fn compare_severity(before: &FindingRecord, after: &FindingRecord) -> Ordering {
    match after.kind.as_str() {
        "complexity" => {
            compare_metric_tuple(before, after, &["cyclomatic", "cognitive", "max_nesting"])
        }
        "duplication" => {
            compare_metric_tuple(before, after, &["removable_lines", "copies", "tokens"])
        }
        "risk" => {
            let before = metric(before, "score");
            let after = metric(after, "score");
            if (after - before).abs() < 0.01 {
                Ordering::Equal
            } else {
                after.total_cmp(&before)
            }
        }
        _ => Ordering::Equal,
    }
}

fn compare_metric_tuple(before: &FindingRecord, after: &FindingRecord, names: &[&str]) -> Ordering {
    for name in names {
        let ordering = metric(after, name).total_cmp(&metric(before, name));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn metric(finding: &FindingRecord, name: &str) -> f64 {
    finding.metrics.get(name).copied().unwrap_or(0.0)
}

fn change_rank(state: &str) -> u8 {
    match state {
        "new" => 0,
        "worsened" => 1,
        "improved" => 2,
        "resolved" => 3,
        _ => 4,
    }
}

fn change_finding(change: &FindingChange) -> &FindingRecord {
    change
        .after
        .as_ref()
        .or(change.before.as_ref())
        .expect("finding changes always retain a before or after record")
}

fn add_duplicate_findings(findings: &mut Vec<FindingRecord>, kind: &str, groups: &[CloneGroup]) {
    for group in groups {
        let Some(primary) = group.instances.first() else {
            continue;
        };
        let fingerprint = if group.fingerprint.is_empty() {
            let tokens = group.tokens.to_string();
            format!(
                "dup:v1:{}",
                fingerprint(&["duplication:v1", kind, &group.format, &tokens])
            )
        } else {
            group.fingerprint.clone()
        };
        let copies = group.instances.len();
        let removable_lines = group.lines.saturating_mul(copies.saturating_sub(1));
        findings.push(FindingRecord {
            fingerprint,
            identity: group.fingerprint.clone(),
            kind: "duplication".to_string(),
            severity: if kind == "exact" { "warning" } else { "note" }.to_string(),
            message: format!(
                "{kind} duplicate family with {copies} copies and {removable_lines} removable lines"
            ),
            primary_location: clone_location(primary),
            related_locations: group.instances.iter().skip(1).map(clone_location).collect(),
            metrics: BTreeMap::from([
                ("copies".to_string(), copies as f64),
                ("removable_lines".to_string(), removable_lines as f64),
                ("tokens".to_string(), group.tokens as f64),
                ("similarity".to_string(), group.similarity),
            ]),
        });
    }
}

fn clone_location(instance: &crate::model::CloneInstance) -> FindingLocation {
    let ends_at_next_line_start = instance.end_column == 1
        && instance.end_line > instance.start_line
        && instance.end_byte > instance.start_byte;
    let end_line = if ends_at_next_line_start {
        instance.end_line.saturating_sub(1)
    } else {
        instance.end_line
    }
    .max(instance.start_line);
    FindingLocation {
        path: instance.path.clone(),
        start_line: instance.start_line,
        end_line,
        start_column: (instance.start_column > 0).then_some(instance.start_column),
        end_column: (!ends_at_next_line_start && instance.end_column > 0)
            .then_some(instance.end_column),
    }
}

fn marker_severity(marker: &str) -> &'static str {
    match marker.to_ascii_uppercase().as_str() {
        "BUG" | "FIXME" => "warning",
        _ => "note",
    }
}

fn remap_location(location: &mut FindingLocation, renames: &BTreeMap<&PathBuf, &PathBuf>) {
    if let Some(path) = renames.get(&location.path) {
        location.path = (*path).clone();
    }
}

fn path_fingerprint(kind: &str, path: &Path, identity: &str) -> String {
    let versioned_kind = format!("{kind}:v1");
    let path = normalized_path(path);
    format!(
        "{versioned_kind}:{}",
        fingerprint(&[&versioned_kind, &path, identity])
    )
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn fingerprint(parts: &[&str]) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&part.len().to_le_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }
    format!("{:032x}", xxh3_128(&bytes))
}

#[cfg(test)]
mod tests {
    use super::{RISK_THRESHOLD, build, clone_location, compare};
    use crate::config::Config;
    use crate::model::{CloneInstance, Duplication, FindingCatalog, FindingRecord, RiskEntry};
    use std::collections::BTreeMap;
    use std::path::Path;

    fn complexity(fingerprint: &str, cyclomatic: f64) -> FindingRecord {
        FindingRecord {
            fingerprint: fingerprint.to_string(),
            kind: "complexity".to_string(),
            metrics: BTreeMap::from([("cyclomatic".to_string(), cyclomatic)]),
            ..FindingRecord::default()
        }
    }

    #[test]
    fn comparison_classifies_all_four_finding_states() {
        let before = FindingCatalog {
            version: 1,
            findings: vec![
                complexity("improved", 3.0),
                complexity("worsened", 1.0),
                complexity("resolved", 2.0),
            ],
        };
        let after = FindingCatalog {
            version: 1,
            findings: vec![
                complexity("improved", 2.0),
                complexity("worsened", 2.0),
                complexity("new", 1.0),
            ],
        };

        let delta = compare(&before, &after);

        assert_eq!(delta.counts.new, 1);
        assert_eq!(delta.counts.resolved, 1);
        assert_eq!(delta.counts.worsened, 1);
        assert_eq!(delta.counts.improved, 1);
    }

    #[test]
    fn newline_endpoint_projects_to_an_occupied_whole_line() {
        let location = clone_location(&CloneInstance {
            path: "source.rs".into(),
            start_line: 1,
            end_line: 2,
            start_column: 1,
            end_column: 1,
            start_byte: 0,
            end_byte: 7,
            ..CloneInstance::default()
        });

        assert_eq!(location.start_line, 1);
        assert_eq!(location.end_line, 1);
        assert_eq!(location.start_column, Some(1));
        assert_eq!(location.end_column, None);
    }

    #[test]
    fn risk_finding_threshold_remains_inclusive_at_point_seven() {
        let risks = [
            RiskEntry {
                path: "below.rs".to_string(),
                score: RISK_THRESHOLD - 0.001,
                ..RiskEntry::default()
            },
            RiskEntry {
                path: "at.rs".to_string(),
                score: RISK_THRESHOLD,
                ..RiskEntry::default()
            },
        ];

        let catalog = build(&[], &Duplication::default(), &risks, &Config::default());
        let risk_findings = catalog
            .findings
            .iter()
            .filter(|finding| finding.kind == "risk")
            .collect::<Vec<_>>();

        assert_eq!(risk_findings.len(), 1);
        assert_eq!(risk_findings[0].primary_location.path, Path::new("at.rs"));
    }
}
