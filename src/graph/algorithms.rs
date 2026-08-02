use super::{HashSet, Path};
#[cfg(test)]
use super::{ImportResolution, JsResolver};

/// Resolve a JS/TS import specifier to a node path.
///
/// Only handles local specs: `./`, `../`, or `@/` prefixes. All others return
/// `None` (treated as external / npm packages).
#[cfg(test)]
pub(crate) fn resolve_js(
    importer_rel: &str,
    spec: &str,
    nodes: &HashSet<String>,
) -> Option<String> {
    match JsResolver::default().resolve(importer_rel, spec, nodes) {
        ImportResolution::Resolved { target, .. } => Some(target),
        ImportResolution::Local
        | ImportResolution::NonGraph
        | ImportResolution::Unresolved
        | ImportResolution::External => None,
    }
}

/// Resolve a Python import specifier to a node path.
///
/// Only handles relative imports (specs starting with `.`). All others return
/// `None`.
pub(crate) fn resolve_py(
    importer_rel: &str,
    spec: &str,
    nodes: &HashSet<String>,
) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }

    let level = spec.chars().take_while(|&c| c == '.').count();
    let remainder_str = &spec[level..];
    let remainder: Vec<&str> = remainder_str.split('.').filter(|s| !s.is_empty()).collect();

    let parent = path_parent(importer_rel);
    let start_dir = go_up(&parent, level.saturating_sub(1));

    if remainder.is_empty() {
        // `from . import x` — look for __init__.py in the package dir.
        let candidate = if start_dir.is_empty() {
            "__init__.py".to_string()
        } else {
            format!("{start_dir}/__init__.py")
        };
        return nodes.contains(candidate.as_str()).then_some(candidate);
    }

    let base = if start_dir.is_empty() {
        remainder.join("/")
    } else {
        format!("{}/{}", start_dir, remainder.join("/"))
    };

    let c1 = format!("{base}.py");
    if nodes.contains(c1.as_str()) {
        return Some(c1);
    }
    let c2 = format!("{base}/__init__.py");
    if nodes.contains(c2.as_str()) {
        return Some(c2);
    }

    None
}

// ---------------------------------------------------------------------------
// Entrypoint heuristic
// ---------------------------------------------------------------------------

/// Returns `true` if `rel` looks like a well-known entrypoint or config file.
///
/// Filenames checked (case-insensitive stem): `index`, `main`, `app`;
/// exact names: `__init__.py`, `__main__.py`, `setup.py`, `conftest.py`,
/// `bootstrap.php`, `artisan`;
/// suffix patterns: `.config.{js,ts,mjs,cjs}`, `.d.ts`.
pub(crate) fn is_entrypoint(rel: &str) -> bool {
    let filename = rel.rsplit('/').next().unwrap_or(rel);
    let lower = filename.to_ascii_lowercase();

    if matches!(
        lower.as_str(),
        "__init__.py"
            | "__main__.py"
            | "setup.py"
            | "conftest.py"
            | "bootstrap.php"
            | "artisan"
            | "lib.rs"
            | "build.rs"
    ) {
        return true;
    }

    if lower.ends_with(".d.ts") {
        return true;
    }

    for suffix in &[".config.js", ".config.ts", ".config.mjs", ".config.cjs"] {
        if lower.ends_with(suffix) {
            return true;
        }
    }

    // index.*, main.*, app.* — any extension.
    if let Some(dot) = lower.find('.')
        && matches!(&lower[..dot], "index" | "main" | "app")
    {
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// Strongly-connected components (Kosaraju's algorithm, iterative)
// ---------------------------------------------------------------------------

/// Compute strongly-connected components of the directed graph.
///
/// `nodes` is the ordered node list (used only for its length).
/// `edges` is a list of `(from, to)` index pairs.
/// Returns one `Vec<usize>` per component (indices into `nodes`).
pub(crate) fn strongly_connected(nodes: &[String], edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let n = nodes.len();
    if n == 0 {
        return Vec::new();
    }

    let mut adj = vec![vec![]; n];
    let mut radj = vec![vec![]; n];
    for &(u, v) in edges {
        if u < n && v < n {
            adj[u].push(v);
            radj[v].push(u);
        }
    }

    // Phase 1: DFS on the original graph to collect finish order.
    let mut visited = vec![false; n];
    let mut finish_order: Vec<usize> = Vec::with_capacity(n);

    for start in 0..n {
        if visited[start] {
            continue;
        }
        // Iterative DFS: stack stores (node, next_adj_index).
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        visited[start] = true;
        loop {
            match stack.last_mut() {
                None => break,
                Some((node, idx)) => {
                    let node = *node;
                    if *idx < adj[node].len() {
                        let next = adj[node][*idx];
                        *idx += 1;
                        if !visited[next] {
                            visited[next] = true;
                            stack.push((next, 0));
                        }
                    } else {
                        finish_order.push(node);
                        stack.pop();
                    }
                }
            }
        }
    }

    // Phase 2: DFS on the transposed graph in reverse finish order.
    let mut comp_id = vec![usize::MAX; n];
    let mut components: Vec<Vec<usize>> = Vec::new();

    for &start in finish_order.iter().rev() {
        if comp_id[start] != usize::MAX {
            continue;
        }
        let c = components.len();
        components.push(Vec::new());
        let mut stack = vec![start];
        comp_id[start] = c;
        while let Some(node) = stack.pop() {
            components[c].push(node);
            for &next in &radj[node] {
                if comp_id[next] == usize::MAX {
                    comp_id[next] = c;
                    stack.push(next);
                }
            }
        }
    }

    components
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Return the directory portion of a relative path (everything before the last `/`).
pub(super) fn path_parent(rel: &str) -> String {
    match rel.rfind('/') {
        Some(pos) => rel[..pos].to_string(),
        None => String::new(),
    }
}

/// Walk `levels` directories up from `dir` (string-based, no filesystem I/O).
pub(super) fn go_up(dir: &str, levels: usize) -> String {
    if levels == 0 {
        return dir.to_string();
    }
    let parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= levels {
        return String::new();
    }
    parts[..parts.len() - levels].join("/")
}

/// Normalise a `/`-joined path by resolving `.` and `..` segments.
///
/// Does not touch the real filesystem. A leading `./` is stripped.
pub(super) fn normalize_path(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Resolve a normalised base path against the node set, trying bare path,
/// bare + extension, and bare + `/index` + extension.
pub(super) fn try_resolve_js(base: &str, nodes: &HashSet<String>) -> Option<String> {
    const EXTS: &[&str] = &[".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"];

    if nodes.contains(base) {
        return Some(base.to_string());
    }
    for (runtime, substitutions) in [
        (".js", &[".ts", ".tsx", ".d.ts", ".js", ".jsx"][..]),
        (".jsx", &[".tsx", ".d.ts", ".jsx"][..]),
        (".mjs", &[".mts", ".d.mts", ".mjs"][..]),
        (".cjs", &[".cts", ".d.cts", ".cjs"][..]),
    ] {
        if let Some(stem) = base.strip_suffix(runtime) {
            for substitution in substitutions {
                let candidate = format!("{stem}{substitution}");
                if nodes.contains(&candidate) {
                    return Some(candidate);
                }
            }
            return None;
        }
    }
    for &ext in EXTS {
        let c = format!("{base}{ext}");
        if nodes.contains(c.as_str()) {
            return Some(c);
        }
    }
    for &ext in EXTS {
        let c = format!("{base}/index{ext}");
        if nodes.contains(c.as_str()) {
            return Some(c);
        }
    }
    None
}

pub(super) fn try_resolve_php(base: &str, nodes: &HashSet<String>) -> Option<String> {
    let base = normalize_path(base);
    if nodes.contains(&base) {
        return Some(base);
    }
    if Path::new(&base).extension().is_none() {
        for candidate in [format!("{base}.php"), format!("{base}/index.php")] {
            if nodes.contains(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------
