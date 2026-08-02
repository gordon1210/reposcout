use super::{
    BTreeMap, BTreeSet, ConfigAccess, FirstClass, HashSet, ImportResolution, Path, StaticInclude,
    Value, detect, go_up, join_graph_path, normalize_path, path_parent, try_resolve_php,
};

#[derive(Default)]
pub(super) struct GoResolver {
    modules: Vec<GoModule>,
    pub(super) packages: BTreeMap<String, String>,
    pub(super) config_files: Vec<String>,
    pub(super) config_errors: usize,
    pub(super) config_errors_by_path: BTreeMap<String, usize>,
}

pub(super) struct GoModule {
    prefix: String,
    directory: String,
}

impl GoResolver {
    pub(super) fn discover(graph_files: &[String], access: &mut ConfigAccess<'_>) -> Self {
        let go_files = graph_files
            .iter()
            .filter(|path| {
                detect(Path::new(path)).and_then(|info| info.first_class) == Some(FirstClass::Go)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut package_files = BTreeMap::<String, Vec<String>>::new();
        for path in &go_files {
            package_files
                .entry(path_parent(path))
                .or_default()
                .push(path.clone());
        }
        let packages = package_files
            .into_iter()
            .map(|(directory, mut paths)| {
                paths.sort_by(|left, right| {
                    go_representative_rank(left, &directory)
                        .cmp(&go_representative_rank(right, &directory))
                        .then_with(|| left.cmp(right))
                });
                (directory, paths.remove(0))
            })
            .collect();
        if access.root.is_file() {
            return Self {
                packages,
                ..Self::default()
            };
        }

        let mut candidates = BTreeSet::new();
        for path in &go_files {
            let mut directory = path_parent(path);
            loop {
                let relative = join_graph_path(&directory, "go.mod");
                if access.exists(&relative) {
                    candidates.insert(relative);
                }
                if directory.is_empty() {
                    break;
                }
                directory = path_parent(&directory);
            }
        }

        let mut resolver = Self {
            packages,
            ..Self::default()
        };
        let mut prefixes = HashSet::new();
        for relative in candidates {
            match read_go_module(access, &relative) {
                Some(Ok(prefix)) => {
                    resolver.config_files.push(relative.clone());
                    if prefixes.insert(prefix.clone()) {
                        resolver.modules.push(GoModule {
                            prefix,
                            directory: path_parent(&relative),
                        });
                    } else {
                        resolver.config_errors = resolver.config_errors.saturating_add(1);
                        *resolver
                            .config_errors_by_path
                            .entry(relative.clone())
                            .or_insert(0) += 1;
                    }
                }
                Some(Err(())) => {
                    resolver.config_errors = resolver.config_errors.saturating_add(1);
                    *resolver.config_errors_by_path.entry(relative).or_insert(0) += 1;
                }
                None => {}
            }
        }
        resolver.modules.sort_by(|left, right| {
            right
                .prefix
                .len()
                .cmp(&left.prefix.len())
                .then_with(|| left.prefix.cmp(&right.prefix))
        });
        resolver.config_files.sort();
        resolver.config_files.dedup();
        resolver
    }

    pub(super) fn resolve(&self, importer_rel: &str, package: &str) -> ImportResolution {
        if package.starts_with("./") || package.starts_with("../") {
            let directory = normalize_path(&join_graph_path(&path_parent(importer_rel), package));
            return self
                .packages
                .get(&directory)
                .map_or(ImportResolution::Unresolved, |target| {
                    ImportResolution::Resolved {
                        target: target.clone(),
                        resolver: "go-relative",
                    }
                });
        }
        let Some(module) = self.modules.iter().find(|module| {
            package == module.prefix
                || package
                    .strip_prefix(&module.prefix)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }) else {
            return ImportResolution::External;
        };
        let suffix = package
            .strip_prefix(&module.prefix)
            .unwrap_or_default()
            .trim_start_matches('/');
        let directory = join_graph_path(&module.directory, suffix);
        self.packages
            .get(&directory)
            .map_or(ImportResolution::Unresolved, |target| {
                ImportResolution::Resolved {
                    target: target.clone(),
                    resolver: "go-module",
                }
            })
    }
}

pub(super) fn read_go_module(
    access: &mut ConfigAccess<'_>,
    relative: &str,
) -> Option<Result<String, ()>> {
    let content = access.read(relative)?;
    let module = content.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        (parts.next() == Some("module"))
            .then(|| parts.next())
            .flatten()
    });
    Some(module.map(str::to_string).ok_or(()))
}

pub(super) fn go_representative_rank(path: &str, directory: &str) -> (bool, bool, bool) {
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let directory_name = Path::new(directory)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    (
        name.ends_with("_test.go"),
        name != format!("{directory_name}.go"),
        name != "doc.go",
    )
}

pub(super) fn path_in_scope(path: &str, directory: &str) -> bool {
    directory.is_empty()
        || path
            .strip_prefix(directory)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(super) fn strip_graph_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        Some(path)
    } else {
        path.strip_prefix(prefix)?.strip_prefix('/')
    }
}

#[derive(Default)]
pub(super) struct PhpResolver {
    mappings: Vec<PhpMapping>,
    pub(super) config_files: Vec<String>,
    pub(super) config_errors: usize,
    pub(super) config_errors_by_path: BTreeMap<String, usize>,
}

pub(super) struct PhpMapping {
    prefix: String,
    directories: Vec<String>,
    config_directory: String,
    kind: PhpMappingKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PhpMappingKind {
    Psr4,
    Psr0,
}

impl PhpResolver {
    pub(super) fn discover(graph_files: &[String], access: &mut ConfigAccess<'_>) -> Self {
        if access.root.is_file() {
            return Self::default();
        }

        let mut candidates = BTreeSet::new();
        for path in graph_files {
            if detect(Path::new(path)).and_then(|info| info.first_class) != Some(FirstClass::Php) {
                continue;
            }
            let mut directory = path_parent(path);
            loop {
                let relative = join_graph_path(&directory, "composer.json");
                if access.exists(&relative) {
                    candidates.insert(relative);
                }
                if directory.is_empty() {
                    break;
                }
                directory = path_parent(&directory);
            }
        }

        let mut resolver = Self::default();
        for relative in candidates {
            match read_composer_config(access, &relative) {
                Some(Ok(mappings)) if !mappings.is_empty() => {
                    resolver.config_files.push(relative);
                    resolver.mappings.extend(mappings);
                }
                Some(Ok(_)) | None => {}
                Some(Err(())) => {
                    resolver.config_errors += 1;
                    *resolver.config_errors_by_path.entry(relative).or_insert(0) += 1;
                }
            }
        }
        resolver.mappings.sort_by(|left, right| {
            php_scope_rank(&right.config_directory)
                .cmp(&php_scope_rank(&left.config_directory))
                .then_with(|| right.prefix.len().cmp(&left.prefix.len()))
                .then_with(|| php_mapping_rank(left.kind).cmp(&php_mapping_rank(right.kind)))
                .then_with(|| left.prefix.cmp(&right.prefix))
        });
        resolver.config_files.sort();
        resolver.config_files.dedup();
        resolver
    }

    pub(super) fn resolve_namespace(
        &self,
        importer_rel: &str,
        symbol: &str,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        let symbol = symbol.trim().trim_start_matches('\\');
        let best_scope = self
            .mappings
            .iter()
            .filter(|mapping| {
                php_importer_in_scope(importer_rel, &mapping.config_directory)
                    && symbol.starts_with(&mapping.prefix)
            })
            .map(|mapping| php_scope_rank(&mapping.config_directory))
            .max();
        let best_prefix = best_scope.and_then(|scope| {
            self.mappings
                .iter()
                .filter(|mapping| {
                    php_scope_rank(&mapping.config_directory) == scope
                        && php_importer_in_scope(importer_rel, &mapping.config_directory)
                        && symbol.starts_with(&mapping.prefix)
                })
                .map(|mapping| mapping.prefix.len())
                .max()
        });

        if let (Some(scope), Some(prefix_len)) = (best_scope, best_prefix) {
            for mapping in self.mappings.iter().filter(|mapping| {
                php_scope_rank(&mapping.config_directory) == scope
                    && mapping.prefix.len() == prefix_len
                    && php_importer_in_scope(importer_rel, &mapping.config_directory)
                    && symbol.starts_with(&mapping.prefix)
            }) {
                let class_path = match mapping.kind {
                    PhpMappingKind::Psr4 => symbol
                        .strip_prefix(&mapping.prefix)
                        .unwrap_or(symbol)
                        .replace('\\', "/"),
                    PhpMappingKind::Psr0 => symbol.replace(['\\', '_'], "/"),
                };
                for directory in &mapping.directories {
                    let base = join_graph_path(directory, &class_path);
                    if let Some(target) = try_resolve_php(&base, nodes) {
                        return ImportResolution::Resolved {
                            target,
                            resolver: match mapping.kind {
                                PhpMappingKind::Psr4 => "composer-psr-4",
                                PhpMappingKind::Psr0 => "composer-psr-0",
                            },
                        };
                    }
                }
            }
            return ImportResolution::Unresolved;
        }

        let parts = symbol.split('\\').collect::<Vec<_>>();
        let without_vendor = (parts.len() > 1).then(|| parts[1..].join("/"));
        let full = parts.join("/");
        let mut candidates = Vec::new();
        for directory in ["src", "app", "lib"] {
            for class_path in std::iter::once(full.as_str()).chain(without_vendor.as_deref()) {
                if let Some(target) =
                    try_resolve_php(&join_graph_path(directory, class_path), nodes)
                {
                    candidates.push(target);
                }
            }
        }
        candidates.sort();
        candidates.dedup();
        match candidates.as_slice() {
            [target] => ImportResolution::Resolved {
                target: target.clone(),
                resolver: "php-namespace-heuristic",
            },
            [] => ImportResolution::External,
            _ => ImportResolution::Unresolved,
        }
    }

    pub(super) fn resolve_include(
        importer_rel: &str,
        include: &StaticInclude,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        let include_path = match include {
            StaticInclude::Literal(path) | StaticInclude::DirectoryRelative { path, .. } => path,
        };
        if is_composer_autoloader(include_path) {
            return ImportResolution::External;
        }
        let mut bases = Vec::new();
        match include {
            StaticInclude::Literal(path) => {
                if Path::new(path).is_absolute() {
                    return ImportResolution::External;
                }
                bases.push(join_graph_path(&path_parent(importer_rel), path));
                bases.push(normalize_path(path));
            }
            StaticInclude::DirectoryRelative { parents, path } => {
                let directory = go_up(&path_parent(importer_rel), *parents);
                bases.push(join_graph_path(&directory, path));
            }
        }
        let mut candidates = bases
            .into_iter()
            .filter_map(|base| try_resolve_php(&base, nodes))
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        match candidates.as_slice() {
            [target] => ImportResolution::Resolved {
                target: target.clone(),
                resolver: "php-include",
            },
            _ => ImportResolution::Unresolved,
        }
    }
}

pub(super) fn is_composer_autoloader(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let normalized = normalized.trim_start_matches(['.', '/']);
    normalized == "vendor/autoload.php" || normalized.ends_with("/vendor/autoload.php")
}

pub(super) fn read_composer_config(
    access: &mut ConfigAccess<'_>,
    relative: &str,
) -> Option<Result<Vec<PhpMapping>, ()>> {
    let content = access.read(relative)?;
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(_) => return Some(Err(())),
    };
    let config_directory = path_parent(relative);
    let mut mappings = Vec::new();
    for section in ["autoload", "autoload-dev"] {
        let Some(autoload) = value.get(section).and_then(Value::as_object) else {
            continue;
        };
        for (field, kind) in [
            ("psr-4", PhpMappingKind::Psr4),
            ("psr-0", PhpMappingKind::Psr0),
        ] {
            let Some(prefixes) = autoload.get(field).and_then(Value::as_object) else {
                continue;
            };
            for (prefix, directories) in prefixes {
                let directories = composer_paths(directories)
                    .into_iter()
                    .map(|directory| join_graph_path(&config_directory, &directory))
                    .collect::<Vec<_>>();
                if !directories.is_empty() {
                    mappings.push(PhpMapping {
                        prefix: prefix.trim_start_matches('\\').to_string(),
                        directories,
                        config_directory: config_directory.clone(),
                        kind,
                    });
                }
            }
        }
    }
    Some(Ok(mappings))
}

pub(super) fn composer_paths(value: &Value) -> Vec<String> {
    match value {
        Value::String(path) => vec![path.clone()],
        Value::Array(paths) => paths
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn php_importer_in_scope(importer_rel: &str, directory: &str) -> bool {
    directory.is_empty()
        || importer_rel
            .strip_prefix(directory)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(super) fn php_scope_rank(directory: &str) -> usize {
    directory.split('/').filter(|part| !part.is_empty()).count()
}

pub(super) fn php_mapping_rank(kind: PhpMappingKind) -> usize {
    match kind {
        PhpMappingKind::Psr4 => 0,
        PhpMappingKind::Psr0 => 1,
    }
}

pub(super) fn combined_config_files<'a>(
    groups: impl IntoIterator<Item = &'a [String]>,
) -> Vec<String> {
    let mut files = groups
        .into_iter()
        .flat_map(|files| files.iter().cloned())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

pub(super) fn combined_config_errors<'a>(
    groups: impl IntoIterator<Item = &'a BTreeMap<String, usize>>,
) -> BTreeMap<String, usize> {
    let mut combined = BTreeMap::new();
    for group in groups {
        for (path, count) in group {
            let entry = combined.entry(path.clone()).or_insert(0usize);
            *entry = entry.saturating_add(*count);
        }
    }
    combined
}

pub(super) fn python_root_rank(importer_rel: &str, root: &str) -> usize {
    if root.is_empty() {
        return 1;
    }
    let importer_directory = path_parent(importer_rel);
    let shared_prefix = importer_directory
        .split('/')
        .zip(root.split('/'))
        .take_while(|(importer, candidate)| importer == candidate)
        .count();
    let depth = root.split('/').count();
    if importer_rel == root
        || importer_rel
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
    {
        return depth.saturating_add(10_000);
    }
    if shared_prefix == 0 {
        2
    } else {
        shared_prefix.saturating_mul(100).saturating_add(depth)
    }
}
