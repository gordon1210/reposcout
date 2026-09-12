use crate::model::task_diagnostics::{
    TaskDiagnostic, TaskDiagnosticEvidence, TaskDiagnosticFormat, TaskDiagnosticStatus,
};
use std::{collections::BTreeSet, io::Read, path::Path};

mod parse;
mod paths;

#[derive(Debug, Clone, Copy)]
/// Hard bounds for diagnostic input bytes, normalized records and serialized details.
pub struct TaskDiagnosticLimits {
    pub input_bytes: usize,
    pub records: usize,
    pub details: usize,
}
impl TaskDiagnosticLimits {
    #[must_use]
    pub const fn for_safe(safe: bool) -> Self {
        if safe {
            Self {
                input_bytes: 1024 * 1024,
                records: 250,
                details: 50,
            }
        } else {
            Self {
                input_bytes: 8 * 1024 * 1024,
                records: 1000,
                details: 100,
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
/// An input read, file-kind or supported-format failure preventing diagnostic normalization.
pub enum TaskDiagnosticError {
    #[error("cannot read task diagnostic input")]
    Io(#[from] std::io::Error),
    #[error("task diagnostic input must be a regular file")]
    NotRegularFile,
    #[error("unsupported task diagnostic format; select text explicitly for heuristic extraction")]
    UnsupportedFormat,
    #[error("task diagnostic structured input contains no usable records")]
    MalformedInput,
}

#[derive(Clone)]
/// Bounded normalized diagnostics and parse coverage before repository location resolution.
pub struct ParsedTaskDiagnostics {
    pub records: Vec<TaskDiagnostic>,
    pub evidence: TaskDiagnosticEvidence,
    limits: TaskDiagnosticLimits,
}

impl std::fmt::Debug for ParsedTaskDiagnostics {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParsedTaskDiagnostics")
            .field("format", &self.evidence.format)
            .field("status", &self.evidence.status)
            .field("bytes_read", &self.evidence.bytes_read)
            .field("parsed_records", &self.evidence.parsed_records)
            .field("parse_errors", &self.evidence.parse_errors)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
/// The full bounded record set for planning and its separately detail-capped report evidence.
pub struct ResolvedTaskDiagnostics {
    pub records: Vec<TaskDiagnostic>,
    pub evidence: TaskDiagnosticEvidence,
}

#[must_use]
/// Describe supported diagnostic normalization and normal/safe limits without reading input.
pub fn capability() -> crate::model::TaskDiagnosticCapability {
    let normal = TaskDiagnosticLimits::for_safe(false);
    let safe = TaskDiagnosticLimits::for_safe(true);
    crate::model::TaskDiagnosticCapability {
        flag: "--task-diagnostics".into(),
        format_flag: "--task-diagnostics-format".into(),
        formats: ["auto", "sarif", "rustc-json", "text"]
            .map(str::to_owned)
            .to_vec(),
        input_sources: ["regular-file", "stdin"].map(str::to_owned).to_vec(),
        implies_context: true,
        normal_limits: crate::model::TaskDiagnosticLimitCapability {
            input_bytes: normal.input_bytes,
            records: normal.records,
            details: normal.details,
        },
        safe_limits: crate::model::TaskDiagnosticLimitCapability {
            input_bytes: safe.input_bytes,
            records: safe.records,
            details: safe.details,
        },
        max_probe_bytes: 1,
        max_message_chars: 512,
        max_tool_or_code_chars: 128,
        max_path_bytes: 4096,
        id_scope: "deterministic-report-local".into(),
    }
}

/// Read an explicitly supplied regular diagnostic file and normalize its bounded contents.
///
/// # Errors
///
/// Returns an error if the input cannot be inspected or read, is not a regular file, has no supported
/// format, or contains structured input without usable records.
pub fn load_file(
    path: &Path,
    format: TaskDiagnosticFormat,
    limits: TaskDiagnosticLimits,
) -> Result<ParsedTaskDiagnostics, TaskDiagnosticError> {
    if !path.metadata()?.is_file() {
        return Err(TaskDiagnosticError::NotRegularFile);
    }
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?
    };
    #[cfg(not(unix))]
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(TaskDiagnosticError::NotRegularFile);
    }
    read_input(file, format, limits)
}

/// Normalize a bounded diagnostic stream, reading at most one byte beyond the effective input limit to detect truncation.
///
/// # Errors
///
/// Returns an error if reading fails, format detection finds unsupported structured input, or the
/// selected structured format contains no usable records.
pub fn read_input(
    reader: impl Read,
    format: TaskDiagnosticFormat,
    limits: TaskDiagnosticLimits,
) -> Result<ParsedTaskDiagnostics, TaskDiagnosticError> {
    let limits = TaskDiagnosticLimits {
        input_bytes: limits.input_bytes.min(8 * 1024 * 1024),
        records: limits.records.min(1000),
        details: limits.details.min(100),
    };
    let mut bytes = Vec::new();
    reader
        .take((limits.input_bytes + 1) as u64)
        .read_to_end(&mut bytes)?;
    let bytes_read = bytes.len();
    let truncated = bytes_read > limits.input_bytes;
    bytes.truncate(limits.input_bytes);
    let content = decode_input(&bytes);
    let content = content.trim_start_matches('\u{feff}');
    let format = if format == TaskDiagnosticFormat::Auto {
        parse::detect(content)?
    } else {
        format
    };
    let evidence = TaskDiagnosticEvidence {
        format,
        bytes_read,
        input_truncated: truncated,
        omitted_records_exact: !truncated,
        ..TaskDiagnosticEvidence::default()
    };
    let mut parsed = ParsedTaskDiagnostics {
        records: Vec::new(),
        evidence,
        limits,
    };
    parse::parse(content, &mut parsed)?;
    parsed.evidence.parsed_records = parsed.records.len();
    parsed.records.sort();
    parsed.records.dedup();
    parsed.evidence.deduplicated_records = parsed.evidence.parsed_records - parsed.records.len();
    parsed.evidence.status = if parsed.evidence.input_truncated
        || parsed.evidence.records_truncated
        || parsed.evidence.parse_errors > 0
    {
        "partial"
    } else {
        "complete"
    }
    .into();
    Ok(parsed)
}

/// Resolve normalized locations against the existing inventory and target, assign stable report-local IDs and cap serialized details without another filesystem walk.
#[must_use]
pub fn resolve(
    mut parsed: ParsedTaskDiagnostics,
    root: &Path,
    target: &Path,
    inventory: &[String],
) -> ResolvedTaskDiagnostics {
    let inventory: BTreeSet<&str> = inventory.iter().map(String::as_str).collect();
    for record in &mut parsed.records {
        paths::resolve(
            record,
            root,
            target,
            &inventory,
            parsed.evidence.format == TaskDiagnosticFormat::Sarif,
        );
    }
    parsed.records.sort();
    let before = parsed.records.len();
    parsed.records.dedup();
    parsed.evidence.deduplicated_records += before - parsed.records.len();
    for (index, record) in parsed.records.iter_mut().enumerate() {
        record.id = format!("diagnostic-{}", index + 1);
        match record.status {
            TaskDiagnosticStatus::Resolved => parsed.evidence.resolved_records += 1,
            TaskDiagnosticStatus::OutOfScope => parsed.evidence.out_of_scope_records += 1,
            TaskDiagnosticStatus::Unresolved => parsed.evidence.unresolved_records += 1,
        }
    }
    parsed.evidence.omitted_details = parsed.records.len().saturating_sub(parsed.limits.details);
    parsed.evidence.diagnostics = parsed
        .records
        .iter()
        .take(parsed.limits.details)
        .cloned()
        .collect();
    ResolvedTaskDiagnostics {
        records: parsed.records,
        evidence: parsed.evidence,
    }
}

fn decode_input(mut bytes: &[u8]) -> String {
    let mut output = String::new();
    loop {
        match std::str::from_utf8(bytes) {
            Ok(valid) => {
                output.push_str(valid);
                break;
            }
            Err(error) => {
                if let Ok(prefix) = std::str::from_utf8(&bytes[..error.valid_up_to()]) {
                    output.push_str(prefix);
                }
                output.push('\0');
                let consumed = error.valid_up_to()
                    + error
                        .error_len()
                        .unwrap_or(bytes.len() - error.valid_up_to());
                bytes = &bytes[consumed..];
            }
        }
    }
    output
}

fn clean(value: &str, cap: usize) -> String {
    sanitize(value, cap, true)
}

fn sanitize(value: &str, cap: usize, collapse: bool) -> String {
    let mut result = String::new();
    let mut chars = value.chars().peekable();
    let mut count = 0;
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' || (next == '\u{1b}' && chars.next() == Some('\\')) {
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        if collapse && c.is_control() && !c.is_whitespace() {
            continue;
        }
        let c = if collapse && c.is_whitespace() {
            ' '
        } else {
            c
        };
        if collapse && c == ' ' && (result.is_empty() || result.ends_with(' ')) {
            continue;
        }
        if count == cap {
            break;
        }
        result.push(c);
        count += 1;
    }
    result.trim().to_owned()
}
