use super::{ParsedTaskDiagnostics, TaskDiagnosticError, clean};
use crate::model::task_diagnostics::{
    TaskDiagnostic, TaskDiagnosticFormat, TaskDiagnosticSeverity,
};
use serde_json::Value;

mod sarif;
mod text;

pub(super) fn detect(content: &str) -> Result<TaskDiagnosticFormat, TaskDiagnosticError> {
    let prefix = content
        .get(..content.floor_char_boundary((64 * 1024).min(content.len())))
        .unwrap_or(content);
    if content.trim().is_empty() {
        return Ok(TaskDiagnosticFormat::Text);
    }
    for line in prefix.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            if is_rustc(&value) {
                return Ok(TaskDiagnosticFormat::RustcJson);
            }
            if value.get("version").is_some() && value.get("runs").is_some() {
                return Ok(TaskDiagnosticFormat::Sarif);
            }
        }
    }
    let trimmed = content.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if prefix.contains("\"version\"") && prefix.contains("\"runs\"") {
            return Ok(TaskDiagnosticFormat::Sarif);
        }
        if prefix.contains("\"compiler-message\"")
            || prefix.contains("\"$message_type\":\"diagnostic\"")
        {
            return Ok(TaskDiagnosticFormat::RustcJson);
        }
        return Err(TaskDiagnosticError::UnsupportedFormat);
    }
    Ok(TaskDiagnosticFormat::Text)
}

pub(super) fn parse(
    content: &str,
    parsed: &mut ParsedTaskDiagnostics,
) -> Result<(), TaskDiagnosticError> {
    if content.trim().is_empty() {
        return Ok(());
    }
    match parsed.evidence.format {
        TaskDiagnosticFormat::Sarif => sarif::parse(content, parsed),
        TaskDiagnosticFormat::RustcJson => rustc(content, parsed),
        TaskDiagnosticFormat::Text | TaskDiagnosticFormat::Auto => {
            text::parse(content, parsed);
            Ok(())
        }
    }
}

fn is_rustc(value: &Value) -> bool {
    value.get("reason").and_then(Value::as_str) == Some("compiler-message")
        || value.get("$message_type").and_then(Value::as_str) == Some("diagnostic")
        || (value.get("spans").is_some()
            && value.get("level").is_some()
            && value.get("message").is_some_and(Value::is_string))
}

fn rustc(content: &str, parsed: &mut ParsedTaskDiagnostics) -> Result<(), TaskDiagnosticError> {
    let mut valid = 0;
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        if parsed.records.len() >= parsed.limits.records {
            mark_record_limit(parsed);
            break;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            parsed.evidence.parse_errors += 1;
            continue;
        };
        if !is_rustc(&value) {
            if value
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|r| {
                    matches!(
                        r,
                        "compiler-artifact" | "build-script-executed" | "build-finished"
                    )
                })
            {
                parsed.evidence.ignored_records += 1;
                valid += 1;
            } else {
                parsed.evidence.parse_errors += 1;
            }
            continue;
        }
        let diagnostic = if value.get("reason").is_some() {
            &value["message"]
        } else {
            &value
        };
        let (Some(message), Some(spans)) = (
            diagnostic["message"].as_str(),
            diagnostic["spans"].as_array(),
        ) else {
            parsed.evidence.parse_errors += 1;
            continue;
        };
        let before = parsed.records.len();
        let mut primary = false;
        for span in spans
            .iter()
            .filter(|span| span["is_primary"].as_bool() == Some(true))
        {
            primary = true;
            if parsed.records.len() >= parsed.limits.records {
                mark_record_limit(parsed);
                break;
            }
            let Some(path) = span["file_name"].as_str() else {
                parsed.evidence.parse_errors += 1;
                continue;
            };
            let mut record = base(path, message, "high");
            record.severity = severity(diagnostic["level"].as_str());
            record.code = diagnostic["code"]["code"].as_str().map(|s| clean(s, 128));
            record.tool = Some("rustc".into());
            record.line = position(&span["line_start"]);
            record.column = position(&span["column_start"]);
            record.end_line = position(&span["line_end"]);
            record.end_column = position(&span["column_end"]);
            parsed.records.push(record);
        }
        if !primary {
            parsed.evidence.ignored_records += 1;
        }
        if !primary || parsed.records.len() > before {
            valid += 1;
        }
    }
    if valid == 0 && parsed.evidence.parse_errors > 0 {
        Err(TaskDiagnosticError::MalformedInput)
    } else {
        Ok(())
    }
}

fn base(path: &str, message: &str, confidence: &str) -> TaskDiagnostic {
    let invalid = path.len() > 4096 || path.chars().any(char::is_control);
    let mut original_path = if invalid {
        clean(path, 4096)
    } else {
        path.to_owned()
    };
    original_path.truncate(original_path.floor_char_boundary(4096.min(original_path.len())));
    TaskDiagnostic {
        original_path: Some(original_path),
        message: clean(message, 512),
        confidence: confidence.into(),
        reason: invalid.then(|| "invalid-path".into()),
        ..TaskDiagnostic::default()
    }
}

fn position(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
}

fn severity(value: Option<&str>) -> TaskDiagnosticSeverity {
    match value {
        Some("error") => TaskDiagnosticSeverity::Error,
        Some("warning") => TaskDiagnosticSeverity::Warning,
        Some("note" | "help" | "failure-note") => TaskDiagnosticSeverity::Note,
        Some("info" | "none") => TaskDiagnosticSeverity::Info,
        _ => TaskDiagnosticSeverity::Unknown,
    }
}

fn mark_record_limit(parsed: &mut ParsedTaskDiagnostics) {
    parsed.evidence.records_truncated = true;
    parsed.evidence.omitted_records = 1;
    parsed.evidence.omitted_records_exact = false;
}
