use super::{
    BTreeMap, BTreeSet, ConfigAccess, FirstClass, HashSet, ImportResolution, Path, Value, detect,
    normalize_path, path_parent, try_resolve_js,
};

#[derive(Default)]
pub(super) struct JsResolver {
    configs: BTreeMap<String, TsConfig>,
    packages: BTreeMap<String, PackageConfig>,
    packages_by_directory: BTreeMap<String, PackageConfig>,
    ambiguous_packages: HashSet<String>,
    pub(super) config_files: Vec<String>,
    pub(super) config_errors: usize,
    pub(super) config_errors_by_path: BTreeMap<String, usize>,
}

pub(super) struct TsConfig {
    directory: String,
    base_url: Option<String>,
    paths: Vec<PathMapping>,
}

pub(super) struct PathMapping {
    pattern: String,
    targets: Vec<String>,
    base: String,
}

pub(super) struct ParsedTsConfig {
    config: TsConfig,
    related: Vec<String>,
}

#[derive(Clone)]
pub(super) struct PackageConfig {
    directory: String,
    name: Option<String>,
    has_exports: bool,
    has_imports: bool,
    exports: Vec<PackageMapping>,
    imports: Vec<PackageMapping>,
    entrypoints: Vec<String>,
}

#[derive(Clone)]
pub(super) struct PackageMapping {
    pattern: String,
    targets: Vec<String>,
}

impl JsResolver {
    pub(super) fn discover(graph_files: &[String], access: &mut ConfigAccess<'_>) -> Self {
        if access.root.is_file() {
            return Self::default();
        }
        let (config_candidates, package_candidates) =
            collect_resolver_candidates(graph_files, access);
        let mut resolver = Self::default();
        resolver.load_ts_configs(config_candidates, access);
        resolver.load_packages(package_candidates, access);
        resolver.config_files.sort();
        resolver.config_files.dedup();
        resolver
    }

    fn load_ts_configs(&mut self, mut candidates: BTreeSet<String>, access: &mut ConfigAccess<'_>) {
        let mut seen = BTreeSet::new();
        while let Some(relative) = candidates.pop_first() {
            if !seen.insert(relative.clone()) {
                continue;
            }
            match read_ts_config(access, &relative) {
                Some(Ok(parsed)) => {
                    candidates.extend(parsed.related);
                    self.merge_ts_config(relative, parsed.config);
                }
                Some(Err(())) => self.record_config_error(relative),
                None => {}
            }
        }
    }

    fn merge_ts_config(&mut self, relative: String, config: TsConfig) {
        if config.base_url.is_some() || !config.paths.is_empty() {
            self.config_files.push(relative);
        }
        self.configs
            .entry(config.directory.clone())
            .and_modify(|existing| {
                if config.base_url.is_some() {
                    existing.base_url.clone_from(&config.base_url);
                }
                existing
                    .paths
                    .extend(config.paths.iter().map(|mapping| PathMapping {
                        pattern: mapping.pattern.clone(),
                        targets: mapping.targets.clone(),
                        base: mapping.base.clone(),
                    }));
                existing
                    .paths
                    .sort_by(|left, right| left.pattern.cmp(&right.pattern));
                existing.paths.dedup_by(|left, right| {
                    left.pattern == right.pattern && left.base == right.base
                });
            })
            .or_insert(config);
    }

    fn load_packages(&mut self, candidates: BTreeSet<String>, access: &mut ConfigAccess<'_>) {
        for relative in candidates {
            match read_package_config(access, &relative) {
                Some(Ok(package)) => self.record_package(relative, package),
                Some(Err(())) => self.record_config_error(relative),
                None => {}
            }
        }
    }

    fn record_package(&mut self, relative: String, package: PackageConfig) {
        if package.name.is_some() || package.has_exports || package.has_imports {
            self.config_files.push(relative.clone());
        }
        self.packages_by_directory
            .insert(package.directory.clone(), package.clone());
        let Some(name) = package.name.clone() else {
            return;
        };
        if self.ambiguous_packages.contains(&name) {
            return;
        }
        if self.packages.insert(name.clone(), package).is_some() {
            self.packages.remove(&name);
            self.ambiguous_packages.insert(name);
            self.record_config_error(relative);
        }
    }

    fn record_config_error(&mut self, path: String) {
        self.config_errors = self.config_errors.saturating_add(1);
        let count = self.config_errors_by_path.entry(path).or_insert(0);
        *count = count.saturating_add(1);
    }

    pub(super) fn resolve(
        &self,
        importer_rel: &str,
        spec: &str,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        if is_js_non_graph_specifier(spec) {
            return ImportResolution::NonGraph;
        }
        if spec.starts_with("./") || spec.starts_with("../") {
            return resolve_relative_import(importer_rel, spec, nodes);
        }
        if let Some(resolution) = self.resolve_ts_config(importer_rel, spec, nodes) {
            return resolution;
        }
        if spec.starts_with('#') {
            return self.resolve_package_import(importer_rel, spec, nodes);
        }
        if let Some(resolution) = self.resolve_package(spec, nodes) {
            return resolution;
        }
        if let Some(stripped) = spec.strip_prefix("@/") {
            return resolve_heuristic_alias(stripped, nodes);
        }
        ImportResolution::External
    }

    fn resolve_ts_config(
        &self,
        importer: &str,
        specifier: &str,
        nodes: &HashSet<String>,
    ) -> Option<ImportResolution> {
        let mut directory = path_parent(importer);
        loop {
            if let Some(config) = self.configs.get(&directory) {
                if let Some(resolution) = resolve_ts_paths(config, specifier, nodes) {
                    return Some(resolution);
                }
                if let Some(base_url) = &config.base_url
                    && let Some(target) =
                        try_resolve_js(&join_graph_path(base_url, specifier), nodes)
                {
                    return Some(ImportResolution::Resolved {
                        target,
                        resolver: "tsconfig-base-url",
                    });
                }
            }
            if directory.is_empty() {
                return None;
            }
            directory = path_parent(&directory);
        }
    }

    fn resolve_package_import(
        &self,
        importer: &str,
        specifier: &str,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        let mut directory = path_parent(importer);
        loop {
            if let Some(package) = self.packages_by_directory.get(&directory) {
                return resolve_package_mappings(
                    package,
                    &package.imports,
                    specifier,
                    nodes,
                    "package-imports",
                )
                .unwrap_or(ImportResolution::Unresolved);
            }
            if directory.is_empty() {
                return ImportResolution::Unresolved;
            }
            directory = path_parent(&directory);
        }
    }

    fn resolve_package(
        &self,
        specifier: &str,
        nodes: &HashSet<String>,
    ) -> Option<ImportResolution> {
        let (name, subpath) = split_package_specifier(specifier)?;
        if self.ambiguous_packages.contains(name) {
            return Some(ImportResolution::Unresolved);
        }
        let package = self.packages.get(name)?;
        Some(resolve_package_target(package, subpath, nodes))
    }
}

fn collect_resolver_candidates(
    graph_files: &[String],
    access: &ConfigAccess<'_>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut configs = BTreeSet::new();
    let mut packages = BTreeSet::new();
    for path in graph_files.iter().filter(|path| is_javascript_path(path)) {
        let mut directory = path_parent(path);
        loop {
            for name in ["tsconfig.json", "jsconfig.json"] {
                let relative = join_graph_path(&directory, name);
                if access.exists(&relative) {
                    configs.insert(relative);
                }
            }
            let package = join_graph_path(&directory, "package.json");
            if access.exists(&package) {
                packages.insert(package);
            }
            if directory.is_empty() {
                break;
            }
            directory = path_parent(&directory);
        }
    }
    (configs, packages)
}

fn is_javascript_path(path: &str) -> bool {
    matches!(
        detect(Path::new(path)).and_then(|info| info.first_class),
        Some(FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx)
    )
}

fn resolve_relative_import(
    importer: &str,
    specifier: &str,
    nodes: &HashSet<String>,
) -> ImportResolution {
    let parent = path_parent(importer);
    let joined = if parent.is_empty() {
        specifier.to_string()
    } else {
        format!("{parent}/{specifier}")
    };
    try_resolve_js(&normalize_path(&joined), nodes).map_or(ImportResolution::Unresolved, |target| {
        ImportResolution::Resolved {
            target,
            resolver: "relative",
        }
    })
}

fn resolve_package_target(
    package: &PackageConfig,
    subpath: Option<&str>,
    nodes: &HashSet<String>,
) -> ImportResolution {
    let requested = subpath.map_or_else(|| ".".to_string(), |value| format!("./{value}"));
    if package.has_exports {
        return resolve_package_mappings(
            package,
            &package.exports,
            &requested,
            nodes,
            "package-exports",
        )
        .unwrap_or(ImportResolution::Unresolved);
    }
    if let Some(subpath) = subpath {
        return resolve_package_path(package, subpath, nodes, "package-subpath");
    }
    resolve_package_entrypoint(package, nodes)
}

fn resolve_package_entrypoint(
    package: &PackageConfig,
    nodes: &HashSet<String>,
) -> ImportResolution {
    package
        .entrypoints
        .iter()
        .find_map(|entrypoint| {
            try_resolve_js(&join_graph_path(&package.directory, entrypoint), nodes)
        })
        .map_or_else(
            || {
                ["src/index", "index"]
                    .into_iter()
                    .find_map(|entrypoint| {
                        try_resolve_js(&join_graph_path(&package.directory, entrypoint), nodes)
                    })
                    .map_or(ImportResolution::Unresolved, |target| {
                        ImportResolution::Resolved {
                            target,
                            resolver: "package-index",
                        }
                    })
            },
            |target| ImportResolution::Resolved {
                target,
                resolver: "package-entrypoint",
            },
        )
}

fn resolve_package_path(
    package: &PackageConfig,
    subpath: &str,
    nodes: &HashSet<String>,
    resolver: &'static str,
) -> ImportResolution {
    try_resolve_js(&join_graph_path(&package.directory, subpath), nodes)
        .map_or(ImportResolution::Unresolved, |target| {
            ImportResolution::Resolved { target, resolver }
        })
}

fn resolve_heuristic_alias(stripped: &str, nodes: &HashSet<String>) -> ImportResolution {
    ["src", "app", ""]
        .into_iter()
        .find_map(|base| try_resolve_js(&join_graph_path(base, stripped), nodes))
        .map_or(ImportResolution::Unresolved, |target| {
            ImportResolution::Resolved {
                target,
                resolver: "heuristic-alias",
            }
        })
}

pub(super) fn resolve_ts_paths(
    config: &TsConfig,
    specifier: &str,
    nodes: &HashSet<String>,
) -> Option<ImportResolution> {
    for mapping in &config.paths {
        let Some(path_match) = match_path_pattern(&mapping.pattern, specifier) else {
            continue;
        };
        for target in &mapping.targets {
            let target = apply_path_capture(target, path_match.capture.as_deref());
            let joined = join_graph_path(&mapping.base, &target);
            if let Some(target) = try_resolve_js(&joined, nodes) {
                return Some(ImportResolution::Resolved {
                    target,
                    resolver: "tsconfig-paths",
                });
            }
        }
        return Some(ImportResolution::Unresolved);
    }
    None
}

pub(super) fn is_js_non_graph_specifier(spec: &str) -> bool {
    let path = spec.split(['?', '#']).next().unwrap_or(spec);
    let Some(extension) = Path::new(path).extension().and_then(|value| value.to_str()) else {
        return false;
    };
    !matches!(
        extension.to_ascii_lowercase().as_str(),
        "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs"
    )
}

pub(super) fn split_package_specifier(spec: &str) -> Option<(&str, Option<&str>)> {
    if spec.starts_with('.') || spec.starts_with('/') || spec.starts_with('#') {
        return None;
    }
    if spec.starts_with('@') {
        let mut separators = spec.match_indices('/');
        separators.next()?;
        let (second, _) = separators.next().unwrap_or((spec.len(), ""));
        let name = &spec[..second];
        let subpath = spec.get(second + usize::from(second < spec.len())..);
        return Some((name, subpath.filter(|value| !value.is_empty())));
    }
    let (name, subpath) = spec
        .split_once('/')
        .map_or((spec, None), |(name, subpath)| (name, Some(subpath)));
    (!name.is_empty()).then_some((name, subpath.filter(|value| !value.is_empty())))
}

pub(super) fn resolve_package_mappings(
    package: &PackageConfig,
    mappings: &[PackageMapping],
    requested: &str,
    nodes: &HashSet<String>,
    resolver: &'static str,
) -> Option<ImportResolution> {
    for mapping in mappings {
        let Some(path_match) = match_path_pattern(&mapping.pattern, requested) else {
            continue;
        };
        for target in &mapping.targets {
            if !target.starts_with("./") {
                return Some(ImportResolution::External);
            }
            let target = apply_path_capture(target, path_match.capture.as_deref());
            let joined = join_graph_path(&package.directory, &target);
            if let Some(target) = try_resolve_js(&joined, nodes) {
                return Some(ImportResolution::Resolved { target, resolver });
            }
        }
        // Package maps choose one most-specific key. An explicit `null`, an
        // unsupported condition object, or a missing target under that key
        // blocks broader wildcard mappings instead of falling through.
        return Some(ImportResolution::Unresolved);
    }
    None
}

pub(super) fn read_ts_config(
    access: &mut ConfigAccess<'_>,
    relative: &str,
) -> Option<Result<ParsedTsConfig, ()>> {
    let content = access.read(relative)?;
    let value: Value = match serde_json::from_str(&sanitize_jsonc(&content)) {
        Ok(value) => value,
        Err(_) => return Some(Err(())),
    };
    let compiler = value
        .get("compilerOptions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let directory = path_parent(relative);
    let mut related = value
        .get("references")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|reference| reference.get("path").and_then(Value::as_str))
        .filter_map(|reference| resolve_config_reference(access, &directory, reference))
        .collect::<Vec<_>>();
    if let Some(extended) = value.get("extends").and_then(Value::as_str)
        && let Some(extended) = resolve_config_reference(access, &directory, extended)
    {
        related.push(extended);
    }
    related.sort();
    related.dedup();
    let base_url = compiler
        .get("baseUrl")
        .and_then(Value::as_str)
        .map(|base| join_graph_path(&directory, base));
    let mapping_base = base_url.clone().unwrap_or_else(|| directory.clone());
    let mut paths = compiler
        .get("paths")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(pattern, targets)| {
            let targets = targets
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>();
            (!targets.is_empty()).then(|| PathMapping {
                pattern: pattern.clone(),
                targets,
                base: mapping_base.clone(),
            })
        })
        .collect::<Vec<_>>();
    paths.sort_by(|a, b| a.pattern.cmp(&b.pattern));
    Some(Ok(ParsedTsConfig {
        config: TsConfig {
            directory,
            base_url,
            paths,
        },
        related,
    }))
}

pub(super) fn read_package_config(
    access: &mut ConfigAccess<'_>,
    relative: &str,
) -> Option<Result<PackageConfig, ()>> {
    let content = access.read(relative)?;
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(_) => return Some(Err(())),
    };
    let directory = path_parent(relative);
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let has_exports = value.get("exports").is_some();
    let exports = value
        .get("exports")
        .map(package_exports)
        .unwrap_or_default();
    let has_imports = value.get("imports").is_some();
    let imports = value
        .get("imports")
        .and_then(Value::as_object)
        .map(|imports| {
            imports
                .iter()
                .filter(|(pattern, _)| pattern.starts_with('#'))
                .map(|(pattern, value)| PackageMapping {
                    pattern: pattern.clone(),
                    targets: package_targets(value),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let entrypoints = ["source", "module", "main", "types", "typings"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    Some(Ok(PackageConfig {
        directory,
        name,
        has_exports,
        has_imports,
        exports: sorted_package_mappings(exports),
        imports: sorted_package_mappings(imports),
        entrypoints,
    }))
}

pub(super) fn package_exports(value: &Value) -> Vec<PackageMapping> {
    if let Some(exports) = value.as_object()
        && exports.keys().any(|key| key.starts_with('.'))
    {
        return exports
            .iter()
            .filter(|(pattern, _)| pattern.starts_with('.'))
            .map(|(pattern, value)| PackageMapping {
                pattern: pattern.clone(),
                targets: package_targets(value),
            })
            .collect();
    }
    let targets = package_targets(value);
    vec![PackageMapping {
        pattern: ".".to_string(),
        targets,
    }]
}

pub(super) fn package_targets(value: &Value) -> Vec<String> {
    match value {
        Value::String(target) => vec![target.clone()],
        Value::Array(values) => values.iter().flat_map(package_targets).collect(),
        Value::Object(conditions) => {
            let mut targets = Vec::new();
            for condition in ["source", "import", "default", "node", "require", "types"] {
                if let Some(value) = conditions.get(condition) {
                    targets.extend(package_targets(value));
                }
            }
            targets
        }
        _ => Vec::new(),
    }
}

pub(super) fn sorted_package_mappings(mut mappings: Vec<PackageMapping>) -> Vec<PackageMapping> {
    for mapping in &mut mappings {
        let mut seen = HashSet::new();
        mapping.targets.retain(|target| seen.insert(target.clone()));
    }
    mappings.sort_by(|left, right| {
        left.pattern
            .contains('*')
            .cmp(&right.pattern.contains('*'))
            .then_with(|| right.pattern.len().cmp(&left.pattern.len()))
            .then_with(|| left.pattern.cmp(&right.pattern))
    });
    mappings
}

pub(super) fn resolve_config_reference(
    access: &ConfigAccess<'_>,
    directory: &str,
    reference: &str,
) -> Option<String> {
    if !reference.starts_with('.') {
        return None;
    }
    let mut candidate = join_graph_path(directory, reference);
    if access.snapshot.is_some() {
        // Snapshot mode: only preloaded paths exist. Prefer exact key, then .json.
        if access.exists(&candidate) {
            return Some(candidate);
        }
        if Path::new(&candidate).extension().is_none() {
            candidate.push_str(".json");
            if access.exists(&candidate) {
                return Some(candidate);
            }
            let dir_ts = join_graph_path(candidate.trim_end_matches(".json"), "tsconfig.json");
            if access.exists(&dir_ts) {
                return Some(dir_ts);
            }
        }
        return None;
    }
    let absolute = access.root.join(&candidate);
    let metadata = std::fs::symlink_metadata(&absolute).ok();
    if metadata
        .as_ref()
        .is_some_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
    {
        candidate = join_graph_path(&candidate, "tsconfig.json");
    } else if !metadata
        .as_ref()
        .is_some_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
        && Path::new(&candidate).extension().is_none()
    {
        candidate.push_str(".json");
    }
    access.exists(&candidate).then_some(candidate)
}

pub(super) fn sanitize_jsonc(content: &str) -> String {
    String::from_utf8(strip_trailing_commas(&strip_jsonc_comments(content))).unwrap_or_default()
}

fn strip_jsonc_comments(content: &str) -> Vec<u8> {
    let bytes = content.as_bytes();
    let mut stripped = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            stripped.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            stripped.push(byte);
            index += 1;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            stripped.extend_from_slice(b"  ");
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                stripped.push(b' ');
                index += 1;
            }
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            stripped.extend_from_slice(b"  ");
            index += 2;
            while index < bytes.len() {
                if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    stripped.extend_from_slice(b"  ");
                    index += 2;
                    break;
                }
                stripped.push(if bytes[index] == b'\n' { b'\n' } else { b' ' });
                index += 1;
            }
        } else {
            stripped.push(byte);
            index += 1;
        }
    }

    stripped
}

fn strip_trailing_commas(content: &[u8]) -> Vec<u8> {
    let mut sanitized = Vec::with_capacity(content.len());
    let mut in_string = false;
    let mut escaped = false;
    for (index, &byte) in content.iter().enumerate() {
        if in_string {
            sanitized.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        if byte == b'"' {
            in_string = true;
            sanitized.push(byte);
            continue;
        }
        if byte == b',' {
            let next = content[index + 1..]
                .iter()
                .copied()
                .find(|candidate| !candidate.is_ascii_whitespace());
            if matches!(next, Some(b'}' | b']')) {
                sanitized.push(b' ');
                continue;
            }
        }
        sanitized.push(byte);
    }
    sanitized
}

pub(super) struct PathPatternMatch {
    capture: Option<String>,
}

pub(super) fn match_path_pattern(pattern: &str, spec: &str) -> Option<PathPatternMatch> {
    let Some(star) = pattern.find('*') else {
        return (pattern == spec).then_some(PathPatternMatch { capture: None });
    };
    let prefix = &pattern[..star];
    let suffix = &pattern[star + 1..];
    (spec.starts_with(prefix)
        && spec.ends_with(suffix)
        && spec.len() >= prefix.len() + suffix.len())
    .then(|| PathPatternMatch {
        capture: Some(spec[prefix.len()..spec.len() - suffix.len()].to_string()),
    })
}

pub(super) fn apply_path_capture(target: &str, capture: Option<&str>) -> String {
    match capture {
        Some(capture) => target.replacen('*', capture, 1),
        None => target.to_string(),
    }
}

pub(super) fn join_graph_path(base: &str, path: &str) -> String {
    if base.is_empty() || base == "." {
        normalize_path(path)
    } else {
        normalize_path(&format!("{base}/{path}"))
    }
}
