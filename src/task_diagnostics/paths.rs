use crate::model::task_diagnostics::{TaskDiagnostic, TaskDiagnosticStatus};
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

pub(super) fn resolve(
    record: &mut TaskDiagnostic,
    root: &Path,
    target: &Path,
    inventory: &BTreeSet<&str>,
    uri: bool,
) {
    if record.reason.is_some() {
        return;
    }
    let Some(original) = record.original_path.as_deref() else {
        record.reason = Some("missing-path".into());
        return;
    };
    let Some(path) = local_path(original, uri) else {
        record.reason = Some("invalid-or-external-path".into());
        return;
    };
    let Ok(root) = root.canonicalize() else {
        record.reason = Some("unavailable-root".into());
        return;
    };
    let target = target.canonicalize().unwrap_or_else(|_| target.to_owned());
    let mut candidates = Vec::with_capacity(2);
    if path.is_absolute() {
        if !path.starts_with(&root) {
            record.reason = Some("outside-root".into());
            return;
        }
        candidates.push(path);
    } else {
        candidates.push(root.join(&path));
        let anchor = if target.is_file() {
            target.parent().unwrap_or(&target)
        } else {
            &target
        };
        candidates.push(anchor.join(path));
    }
    for candidate in candidates {
        let Ok(relative) = candidate.strip_prefix(&root) else {
            continue;
        };
        let identity = relative.to_string_lossy().replace('\\', "/");
        if !inventory.contains(identity.as_str()) {
            continue;
        }
        if !no_symlink(&root, relative) {
            record.reason = Some("symlink-or-unavailable-path".into());
            return;
        }
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if !canonical.starts_with(&root) {
            record.reason = Some("outside-root".into());
            return;
        }
        record.path = Some(identity);
        record.original_path = None;
        record.status = if canonical.starts_with(&target) {
            TaskDiagnosticStatus::Resolved
        } else {
            TaskDiagnosticStatus::OutOfScope
        };
        record.reason =
            (record.status == TaskDiagnosticStatus::OutOfScope).then(|| "outside-target".into());
        return;
    }
    record.reason = Some("not-in-inventory".into());
}

fn no_symlink(root: &Path, relative: &Path) -> bool {
    let mut current = root.to_owned();
    for component in relative.components() {
        current.push(component);
        let Ok(metadata) = current.symlink_metadata() else {
            return false;
        };
        if metadata.file_type().is_symlink() {
            return false;
        }
    }
    current.is_file()
}

fn local_path(value: &str, uri: bool) -> Option<PathBuf> {
    let decoded;
    let value = if let Some(uri) = value.strip_prefix("file:") {
        let uri = if let Some(rest) = uri.strip_prefix("//") {
            if rest.starts_with('/') {
                rest
            } else {
                let rest = rest.strip_prefix("localhost/")?;
                decoded = format!("/{rest}");
                &decoded
            }
        } else {
            uri
        };
        if uri.contains('?') || uri.contains('#') {
            return None;
        }
        percent_decode(uri)?
    } else {
        if value.contains("://") || value.contains(':') {
            return None;
        }
        if uri {
            if value.contains('?') || value.contains('#') {
                return None;
            }
            percent_decode(value)?
        } else {
            value.to_owned()
        }
    };
    if value.len() > 4096 || value.chars().any(char::is_control) {
        return None;
    }
    let value = value.replace('\\', "/");
    let path = PathBuf::from(value);
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
    {
        return None;
    }
    Some(
        path.components()
            .filter(|c| !matches!(c, Component::CurDir))
            .collect(),
    )
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = bytes.get(index + 1..index + 3)?;
            let high = char::from(pair[0]).to_digit(16)?;
            let low = char::from(pair[1]).to_digit(16)?;
            output.push(u8::try_from(high * 16 + low).ok()?);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}
