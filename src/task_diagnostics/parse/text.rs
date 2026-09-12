use super::{ParsedTaskDiagnostics, base, mark_record_limit, severity};

pub(super) fn parse(content: &str, parsed: &mut ParsedTaskDiagnostics) {
    for raw_line in content.lines() {
        if raw_line.contains('\0') {
            parsed.evidence.parse_errors += 1;
            continue;
        }
        let line = crate::task_diagnostics::sanitize(raw_line, raw_line.len(), false);
        let Some((path, row, column)) = location(&line) else {
            continue;
        };
        if parsed.records.len() >= parsed.limits.records {
            mark_record_limit(parsed);
            break;
        }
        let mut record = base(path, &line, "partial");
        record.line = Some(row);
        record.column = column;
        record.severity =
            severity(
                line.split_whitespace()
                    .find_map(|word| match word.trim_end_matches(':') {
                        "error" => Some("error"),
                        "warning" => Some("warning"),
                        "note" => Some("note"),
                        "info" => Some("info"),
                        _ => None,
                    }),
            );
        parsed.records.push(record);
    }
}

fn location(line: &str) -> Option<(&str, u32, Option<u32>)> {
    if let Some(rest) = line.strip_prefix("File \"") {
        let (path, rest) = rest.split_once("\", line ")?;
        return Some((path, number(rest)?.0, None));
    }
    let line = line.strip_prefix("--> ").unwrap_or(line);
    if let Some((path, rest)) = line.split_once('(')
        && let Some((position, _)) = rest.split_once("):")
    {
        let (row, column) = position.split_once(',')?;
        return Some((
            path.trim(),
            row.parse::<u32>().ok().filter(|n| *n > 0)?,
            Some(column.trim().parse::<u32>().ok().filter(|n| *n > 0)?),
        ));
    }
    for (index, _) in line.match_indices(':') {
        let rest = &line[index + 1..];
        let Some((row, rest)) = number(rest) else {
            continue;
        };
        if !rest.is_empty() && !rest.starts_with(':') && !rest.starts_with(' ') {
            continue;
        }
        let column = rest.strip_prefix(':').and_then(number).map(|(n, _)| n);
        let path = line[..index].trim();
        if path.is_empty() || (path.contains(' ') && !path.starts_with('"')) {
            continue;
        }
        return Some((path.trim_matches('"'), row, column));
    }
    None
}
fn number(value: &str) -> Option<(u32, &str)> {
    let length = value.bytes().take_while(u8::is_ascii_digit).count();
    if length == 0 {
        return None;
    }
    let number = value[..length].parse::<u32>().ok().filter(|n| *n > 0)?;
    Some((number, &value[length..]))
}
