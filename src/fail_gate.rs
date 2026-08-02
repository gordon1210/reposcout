use anyhow::{Result, anyhow};
use reposcout::config::Enabled;
use reposcout::model::Summary;

#[derive(Debug, Clone, Copy)]
enum Comparison {
    GreaterOrEqual,
    LessOrEqual,
    Equal,
    Greater,
    Less,
}

#[derive(Debug, Clone, Copy)]
enum FailMetric {
    MaxCyclomatic,
    AvgCyclomatic,
    MaxCognitive,
    AvgCognitive,
    MinMaintainability,
    AvgMaintainability,
    DuplicatedPercent,
    Tokens,
    Files,
    Sloc,
}

#[derive(Debug)]
pub(crate) struct FailCondition {
    metric: FailMetric,
    comparison: Comparison,
    threshold: f64,
}

pub(crate) fn parse(expr: &str, enabled: Enabled) -> Result<Vec<FailCondition>> {
    expr.split(',')
        .map(str::trim)
        .filter(|condition| !condition.is_empty())
        .map(|condition| parse_condition(condition, enabled))
        .collect()
}

fn parse_condition(condition: &str, enabled: Enabled) -> Result<FailCondition> {
    let operators = [
        (">=", Comparison::GreaterOrEqual),
        ("<=", Comparison::LessOrEqual),
        ("==", Comparison::Equal),
        (">", Comparison::Greater),
        ("<", Comparison::Less),
    ];
    let (key, comparison, rhs) = operators
        .iter()
        .find_map(|(operator, comparison)| {
            condition
                .split_once(operator)
                .map(|(key, rhs)| (key.trim(), *comparison, rhs.trim()))
        })
        .ok_or_else(|| {
            anyhow!("invalid --fail-on condition '{condition}' (expected key OP number)")
        })?;

    let threshold: f64 = rhs
        .parse()
        .map_err(|_| anyhow!("invalid number in --fail-on '{condition}'"))?;
    if !threshold.is_finite() {
        return Err(anyhow!("invalid number in --fail-on '{condition}'"));
    }
    let metric = parse_metric(key)?;
    validate_metric_availability(metric, key, enabled)?;

    Ok(FailCondition {
        metric,
        comparison,
        threshold,
    })
}

fn parse_metric(key: &str) -> Result<FailMetric> {
    match key {
        "max-cyclomatic" => Ok(FailMetric::MaxCyclomatic),
        "avg-cyclomatic" => Ok(FailMetric::AvgCyclomatic),
        "max-cognitive" => Ok(FailMetric::MaxCognitive),
        "avg-cognitive" => Ok(FailMetric::AvgCognitive),
        "min-mi" | "min-maintainability" => Ok(FailMetric::MinMaintainability),
        "avg-mi" | "avg-maintainability" => Ok(FailMetric::AvgMaintainability),
        "duplicated-pct" => Ok(FailMetric::DuplicatedPercent),
        "tokens" => Ok(FailMetric::Tokens),
        "files" => Ok(FailMetric::Files),
        "sloc" => Ok(FailMetric::Sloc),
        _ => Err(anyhow!("unknown --fail-on key '{key}'")),
    }
}

fn validate_metric_availability(metric: FailMetric, key: &str, enabled: Enabled) -> Result<()> {
    let requirement = match metric {
        FailMetric::MaxCyclomatic
        | FailMetric::AvgCyclomatic
        | FailMetric::MaxCognitive
        | FailMetric::AvgCognitive
        | FailMetric::MinMaintainability
        | FailMetric::AvgMaintainability => Some((enabled.complexity, "complexity")),
        FailMetric::DuplicatedPercent => Some((enabled.duplication, "duplication")),
        FailMetric::Tokens => Some((enabled.tokens, "tokens")),
        FailMetric::Files | FailMetric::Sloc => None,
    };
    if let Some((available, analyzer)) = requirement
        && !available
    {
        return Err(anyhow!(
            "--fail-on metric {key} requires the {analyzer} analyzer"
        ));
    }
    Ok(())
}

pub(crate) fn evaluate(conditions: &[FailCondition], summary: &Summary) -> bool {
    conditions.iter().any(|condition| {
        let lhs = metric_value(condition.metric, summary);
        match condition.comparison {
            Comparison::Greater => lhs > condition.threshold,
            Comparison::Less => lhs < condition.threshold,
            Comparison::GreaterOrEqual => lhs >= condition.threshold,
            Comparison::LessOrEqual => lhs <= condition.threshold,
            Comparison::Equal => (lhs - condition.threshold).abs() < f64::EPSILON,
        }
    })
}

fn metric_value(metric: FailMetric, summary: &Summary) -> f64 {
    match metric {
        FailMetric::MaxCyclomatic => f64::from(summary.complexity.cyclomatic_max),
        FailMetric::AvgCyclomatic => summary.complexity.cyclomatic_avg,
        FailMetric::MaxCognitive => f64::from(summary.complexity.cognitive_max),
        FailMetric::AvgCognitive => summary.complexity.cognitive_avg,
        FailMetric::MinMaintainability => summary.complexity.mi_min,
        FailMetric::AvgMaintainability => summary.complexity.mi_avg,
        FailMetric::DuplicatedPercent => summary.duplication.duplicated_pct,
        FailMetric::Tokens => usize_to_metric(summary.tokens),
        FailMetric::Files => usize_to_metric(summary.files),
        FailMetric::Sloc => usize_to_metric(summary.sloc),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "gate thresholds are approximate metrics and repository counters cannot exceed representable practical ranges"
)]
fn usize_to_metric(value: usize) -> f64 {
    value as f64
}
