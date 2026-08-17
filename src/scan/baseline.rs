use super::{
    BaselineDelta, BaselineInput, FindingCatalog, MetricDelta, Path, Result, SCHEMA_VERSION,
    ScanProfile, Summary, usize_to_f64,
};

/// Compare two summaries and produce deltas + regression flags. Pure/testable.
#[expect(
    clippy::too_many_lines,
    reason = "baseline metrics are declared together so every compared summary field and regression threshold remains auditable"
)]
pub(super) fn baseline_delta(
    baseline: &Summary,
    baseline_generated_at: &str,
    current: &Summary,
    profile: &ScanProfile,
) -> BaselineDelta {
    let mut metrics = vec![MetricDelta {
        metric: "files".to_string(),
        baseline: usize_to_f64(baseline.files),
        current: usize_to_f64(current.files),
        delta: usize_to_f64(current.files) - usize_to_f64(baseline.files),
    }];
    if profile.analyzers.tokens {
        metrics.push(MetricDelta {
            metric: "tokens".to_string(),
            baseline: usize_to_f64(baseline.tokens),
            current: usize_to_f64(current.tokens),
            delta: usize_to_f64(current.tokens) - usize_to_f64(baseline.tokens),
        });
    }
    metrics.push(MetricDelta {
        metric: "sloc".to_string(),
        baseline: usize_to_f64(baseline.sloc),
        current: usize_to_f64(current.sloc),
        delta: usize_to_f64(current.sloc) - usize_to_f64(baseline.sloc),
    });
    if profile.analyzers.duplication {
        metrics.push(MetricDelta {
            metric: "duplicated_pct".to_string(),
            baseline: baseline.duplication.duplicated_pct,
            current: current.duplication.duplicated_pct,
            delta: current.duplication.duplicated_pct - baseline.duplication.duplicated_pct,
        });
    }
    if profile.analyzers.complexity {
        metrics.extend([
            MetricDelta {
                metric: "cyclomatic_avg".to_string(),
                baseline: baseline.complexity.cyclomatic_avg,
                current: current.complexity.cyclomatic_avg,
                delta: current.complexity.cyclomatic_avg - baseline.complexity.cyclomatic_avg,
            },
            MetricDelta {
                metric: "cyclomatic_max".to_string(),
                baseline: f64::from(baseline.complexity.cyclomatic_max),
                current: f64::from(current.complexity.cyclomatic_max),
                delta: f64::from(current.complexity.cyclomatic_max)
                    - f64::from(baseline.complexity.cyclomatic_max),
            },
            MetricDelta {
                metric: "mi_avg".to_string(),
                baseline: baseline.complexity.mi_avg,
                current: current.complexity.mi_avg,
                delta: current.complexity.mi_avg - baseline.complexity.mi_avg,
            },
            MetricDelta {
                metric: "mi_min".to_string(),
                baseline: baseline.complexity.mi_min,
                current: current.complexity.mi_min,
                delta: current.complexity.mi_min - baseline.complexity.mi_min,
            },
        ]);
    }
    let mut regressions = Vec::new();

    let dup_base = baseline.duplication.duplicated_pct;
    let dup_cur = current.duplication.duplicated_pct;
    if profile.analyzers.duplication && dup_cur > dup_base + 0.01 {
        regressions.push(format!(
            "duplication +{:.1}% (now {:.1}%)",
            dup_cur - dup_base,
            dup_cur
        ));
    }

    let mi_avg_base = baseline.complexity.mi_avg;
    let mi_avg_cur = current.complexity.mi_avg;
    if profile.analyzers.complexity && mi_avg_cur < mi_avg_base - 0.01 {
        regressions.push(format!(
            "maintainability avg -{:.0} (now {:.0})",
            mi_avg_base - mi_avg_cur,
            mi_avg_cur
        ));
    }

    let mi_min_base = baseline.complexity.mi_min;
    let mi_min_cur = current.complexity.mi_min;
    if profile.analyzers.complexity && mi_min_cur < mi_min_base - 0.01 {
        regressions.push(format!(
            "maintainability min -{:.0} (now {:.0})",
            mi_min_base - mi_min_cur,
            mi_min_cur
        ));
    }

    let cycmax_base = baseline.complexity.cyclomatic_max;
    let cycmax_cur = current.complexity.cyclomatic_max;
    if profile.analyzers.complexity && cycmax_cur > cycmax_base {
        regressions.push(format!(
            "max cyclomatic +{} (now {})",
            i64::from(cycmax_cur) - i64::from(cycmax_base),
            i64::from(cycmax_cur)
        ));
    }

    let regressed = !regressions.is_empty();
    BaselineDelta {
        baseline_generated_at: baseline_generated_at.to_string(),
        metrics,
        regressions,
        regressed,
        finding_changes: crate::findings::unavailable(
            "baseline does not contain a compatible finding catalog",
        ),
    }
}

pub(super) fn compute_baseline_delta(
    path: &Path,
    current: &Summary,
    current_catalog: &FindingCatalog,
    current_profile: &ScanProfile,
    current_encoding: &str,
    current_root: &Path,
    current_target: &Path,
) -> Result<BaselineDelta> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read baseline {}: {e}", path.display()))?;
    let prior: BaselineInput = serde_json::from_str(&text).map_err(|e| {
        anyhow::anyhow!(
            "baseline {} is not a reposcout JSON report: {e}",
            path.display()
        )
    })?;
    if prior.schema_version != SCHEMA_VERSION {
        return Err(anyhow::anyhow!(
            "baseline schema version {} does not match current schema {SCHEMA_VERSION}; regenerate it with the current reposcout",
            prior.schema_version
        ));
    }
    match prior.analysis_profile.as_ref() {
        Some(profile)
            if baseline_predates_duplication_artifact_policy(profile, current_profile) =>
        {
            return Err(anyhow::anyhow!(
                "baseline predates duplication artifact filtering; regenerate it before comparison"
            ));
        }
        Some(profile)
            if scan_profiles_compatible_except_base(profile, current_profile)
                && profile.diff_base != current_profile.diff_base =>
        {
            return Err(anyhow::anyhow!(
                "baseline diff base tree does not match the current scan"
            ));
        }
        Some(profile) if !scan_profiles_compatible(profile, current_profile) => {
            return Err(anyhow::anyhow!(
                "baseline analyzer profile does not match the current scan"
            ));
        }
        None => {
            return Err(anyhow::anyhow!(
                "baseline lacks analyzer profile metadata; regenerate it with the current reposcout"
            ));
        }
        _ => {}
    }
    if current_profile.analyzers.tokens && prior.encoding != current_encoding {
        return Err(anyhow::anyhow!(
            "baseline token encoding does not match the current scan"
        ));
    }
    if target_scope(&prior.root, &prior.target) != target_scope(current_root, current_target) {
        return Err(anyhow::anyhow!(
            "baseline target scope does not match the current scan"
        ));
    }

    let mut delta = baseline_delta(
        &prior.summary,
        &prior.generated_at,
        current,
        current_profile,
    );
    delta.finding_changes = match prior.analysis_profile.as_ref() {
        Some(profile)
            if profile.findings == current_profile.findings
                && prior.finding_catalog.version > 0 =>
        {
            crate::findings::compare(&prior.finding_catalog, current_catalog)
        }
        Some(_) if prior.finding_catalog.version > 0 => {
            crate::findings::unavailable("baseline finding profile does not match the current scan")
        }
        _ => crate::findings::unavailable("baseline does not contain a finding catalog"),
    };
    let new = delta.finding_changes.counts.new;
    let worsened = delta.finding_changes.counts.worsened;
    if new > 0 || worsened > 0 {
        delta.regressions.push(format!(
            "finding regressions: {new} new, {worsened} worsened"
        ));
        delta.regressed = true;
    }
    Ok(delta)
}

fn baseline_predates_duplication_artifact_policy(
    baseline: &ScanProfile,
    current: &ScanProfile,
) -> bool {
    baseline
        .duplication
        .as_ref()
        .is_some_and(|profile| profile.artifact_policy.is_empty())
        && current
            .duplication
            .as_ref()
            .is_some_and(|profile| !profile.artifact_policy.is_empty())
}

pub(super) fn scan_profiles_compatible(left: &ScanProfile, right: &ScanProfile) -> bool {
    scan_profiles_compatible_except_base(left, right) && left.diff_base == right.diff_base
}

pub(super) fn scan_profiles_compatible_except_base(
    left: &ScanProfile,
    right: &ScanProfile,
) -> bool {
    left.analyzers == right.analyzers
        && left.diff_scope == right.diff_scope
        && left.duplication == right.duplication
        && health_profiles_compatible(left, right)
        && left.resources == right.resources
}

pub(super) fn health_profiles_compatible(left: &ScanProfile, right: &ScanProfile) -> bool {
    if left.health == right.health {
        return true;
    }
    if left.analyzers.markers || left.analyzers.duplication {
        return false;
    }

    // Reports created before path exclusions omitted health metadata when both
    // health analyzers were disabled. Scope and format includes cannot change
    // the remaining source-only signals, but a path exclusion can.
    match (left.health.as_ref(), right.health.as_ref()) {
        (None, Some(profile)) => profile.excludes.is_empty(),
        _ => false,
    }
}

pub(super) fn target_scope(root: &Path, target: &Path) -> String {
    target
        .strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map_or_else(
            || ".".to_string(),
            |relative| relative.to_string_lossy().replace('\\', "/"),
        )
}
