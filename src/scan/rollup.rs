use super::{BTreeMap, DirSummary, DuplicateCoverage, FileReport, lang, u64_to_f64, usize_to_f64};

/// The directory bucket for a file's relative path at the given depth.
///
/// ```text
/// dir_bucket("src/metrics/x.rs", 1) == "src"
/// dir_bucket("src/metrics/x.rs", 2) == "src/metrics"
/// dir_bucket("README.md",         1) == "."   (file at repo root)
/// ```
///
/// Backslashes are normalised to `/` before splitting. If `depth` exceeds the
/// number of parent components the result is clamped to however many exist.
pub(super) fn dir_bucket(rel: &str, depth: usize) -> String {
    let rel = rel.replace('\\', "/");
    let parts: Vec<&str> = rel.split('/').collect();
    // Drop the filename (last component).
    let parent = if parts.len() > 1 {
        &parts[..parts.len() - 1]
    } else {
        &[][..]
    };
    if parent.is_empty() {
        return ".".to_string();
    }
    let take = depth.min(parent.len());
    parent[..take].join("/")
}

pub(super) fn rollup_by_dir(
    files: &[FileReport],
    duplicate_coverage: &DuplicateCoverage,
    depth: usize,
) -> Vec<DirSummary> {
    struct Accum {
        summary: DirSummary,
        cyc_sum: u64,
        cyc_count: usize,
        mi_sum: f64,
        mi_count: usize,
    }

    let mut buckets: BTreeMap<String, Accum> = BTreeMap::new();

    for f in files {
        let path_str = f.path.to_string_lossy();
        let bucket = dir_bucket(path_str.as_ref(), depth);
        let key = bucket.clone();
        let entry = buckets.entry(key).or_insert_with(move || Accum {
            summary: DirSummary {
                path: bucket,
                ..DirSummary::default()
            },
            cyc_sum: 0,
            cyc_count: 0,
            mi_sum: 0.0,
            mi_count: 0,
        });

        entry.summary.files += 1;
        entry.summary.tokens += f.tokens;
        entry.summary.loc += f.loc;
        entry.summary.sloc += f.sloc;
        entry.summary.duplicated_lines += duplicate_coverage.covered_lines(&f.path);

        if lang::detect(&f.path).is_some_and(lang::LangInfo::is_code)
            && let Some(c) = &f.complexity
        {
            entry.mi_sum += c.maintainability_index;
            entry.mi_count += 1;
            for function in &c.functions {
                entry.cyc_sum += u64::from(function.cyclomatic);
                entry.cyc_count += 1;
                entry.summary.cyclomatic_max =
                    entry.summary.cyclomatic_max.max(function.cyclomatic);
            }
        }
    }

    let mut result: Vec<DirSummary> = buckets
        .into_values()
        .map(|a| {
            let mut s = a.summary;
            if a.cyc_count > 0 {
                s.cyclomatic_avg = u64_to_f64(a.cyc_sum) / usize_to_f64(a.cyc_count);
            }
            if a.mi_count > 0 {
                s.mi_avg = a.mi_sum / usize_to_f64(a.mi_count);
            }
            s
        })
        .collect();

    result.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.path.cmp(&b.path)));
    result
}
