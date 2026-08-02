use super::{
    BTreeMap, BTreeSet, FirstClass, HashMap, HashSet, Path, ReadBudget, ReadOutcome, RustImport,
    detect, fs_budget, join_graph_path, normalize_path, path_in_scope, path_parent,
    python_root_rank, resolve_py, strip_graph_prefix,
};

pub(super) enum ImportResolution {
    Resolved {
        target: String,
        resolver: &'static str,
    },
    Local,
    NonGraph,
    Unresolved,
    External,
}

pub(super) struct PythonResolver {
    /// Deterministic import roots inferred from conventional `src/` layouts.
    /// The repository root is represented by the empty string and tried too.
    roots: Vec<String>,
}

impl PythonResolver {
    pub(super) fn discover(graph_files: &[String]) -> Self {
        let mut roots = BTreeSet::from([String::new()]);
        for path in graph_files {
            let parts = path.split('/').collect::<Vec<_>>();
            for (index, part) in parts.iter().enumerate() {
                if *part == "src" {
                    roots.insert(parts[..=index].join("/"));
                }
            }
        }
        Self {
            roots: roots.into_iter().collect(),
        }
    }

    pub(super) fn resolve(
        &self,
        importer_rel: &str,
        spec: &str,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        if spec.starts_with('.') {
            return resolve_py(importer_rel, spec, nodes).map_or(
                ImportResolution::Unresolved,
                |target| ImportResolution::Resolved {
                    target,
                    resolver: "python-relative",
                },
            );
        }

        let module_path = spec.replace('.', "/");
        let mut candidates = Vec::new();
        for root in &self.roots {
            let base = join_graph_path(root, &module_path);
            for target in [format!("{base}.py"), format!("{base}/__init__.py")] {
                if nodes.contains(&target) {
                    candidates.push((root.as_str(), target));
                }
            }
        }
        candidates.sort_by(|(left_root, left_path), (right_root, right_path)| {
            python_root_rank(importer_rel, right_root)
                .cmp(&python_root_rank(importer_rel, left_root))
                .then_with(|| left_path.cmp(right_path))
        });
        candidates.dedup_by(|left, right| left.1 == right.1);
        let Some((root, target)) = candidates.first() else {
            return ImportResolution::External;
        };
        let top_rank = python_root_rank(importer_rel, root);
        if candidates
            .iter()
            .skip(1)
            .any(|(candidate_root, _)| python_root_rank(importer_rel, candidate_root) == top_rank)
        {
            return ImportResolution::Unresolved;
        }
        ImportResolution::Resolved {
            target: target.clone(),
            resolver: if root.is_empty() {
                "python-absolute"
            } else {
                "python-src-root"
            },
        }
    }
}

#[derive(Default)]
pub(super) struct RustResolver {
    files: HashMap<String, RustFileModule>,
    modules: BTreeMap<(String, String), Vec<String>>,
    crates: BTreeMap<String, RustCrateTarget>,
    ambiguous_crates: HashSet<String>,
    pub(super) config_files: Vec<String>,
    pub(super) config_errors: usize,
    pub(super) config_errors_by_path: BTreeMap<String, usize>,
}

#[derive(Clone)]
pub(super) struct RustFileModule {
    source_root: String,
    module: Vec<String>,
}

#[derive(Clone)]
pub(super) struct RustCrateTarget {
    source_root: String,
    root_file: String,
}

#[derive(Clone)]
pub(super) struct RustPackage {
    config_path: String,
    directory: String,
    names: Vec<String>,
    lib_path: String,
}

struct RustUseSearch {
    source_root: String,
    module: Vec<String>,
    file_local_scope: bool,
    resolver: &'static str,
    must_resolve: bool,
}

struct RustUseCursor {
    source_root: String,
    module: Vec<String>,
    segments: Vec<String>,
    inside_inline_module: bool,
}

impl RustUseCursor {
    fn new(file: &RustFileModule, inline_modules: &[String], segments: Vec<String>) -> Self {
        let mut module = file.module.clone();
        module.extend(inline_modules.iter().cloned());
        Self {
            source_root: file.source_root.clone(),
            module,
            segments,
            inside_inline_module: !inline_modules.is_empty(),
        }
    }

    fn finish(
        mut self,
        path: &str,
        resolver: &'static str,
        file_local_scope: bool,
    ) -> RustUsePreparation {
        self.module.extend(self.segments);
        RustUsePreparation::Search(RustUseSearch {
            source_root: self.source_root,
            module: self.module,
            file_local_scope,
            resolver,
            must_resolve: resolver == "rust-workspace"
                || path.starts_with("crate::")
                || path.starts_with("self::")
                || path.starts_with("super::"),
        })
    }
}

enum RustUsePreparation {
    Search(RustUseSearch),
    Resolution(ImportResolution),
}

impl RustResolver {
    pub(super) fn discover(graph_files: &[String], access: &mut ConfigAccess<'_>) -> Self {
        if access.root.is_file() {
            return Self::from_files(graph_files, &[]);
        }

        let rust_files = graph_files
            .iter()
            .filter(|path| {
                detect(Path::new(path)).and_then(|info| info.first_class) == Some(FirstClass::Rust)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut candidates = BTreeSet::new();
        for path in &rust_files {
            let mut directory = path_parent(path);
            loop {
                let relative = join_graph_path(&directory, "Cargo.toml");
                if access.exists(&relative) {
                    candidates.insert(relative);
                }
                if directory.is_empty() {
                    break;
                }
                directory = path_parent(&directory);
            }
        }

        let mut packages = Vec::new();
        let mut config_files = Vec::new();
        let mut config_errors = 0usize;
        let mut config_errors_by_path = BTreeMap::new();
        for relative in candidates {
            match read_cargo_package(access, &relative) {
                Some(Ok(package)) => {
                    config_files.push(relative.clone());
                    if let Some(package) = package {
                        packages.push(package);
                    }
                }
                Some(Err(())) => {
                    config_errors = config_errors.saturating_add(1);
                    *config_errors_by_path.entry(relative).or_insert(0) += 1;
                }
                None => {}
            }
        }
        packages.sort_by(|left, right| {
            rust_scope_rank(&right.directory)
                .cmp(&rust_scope_rank(&left.directory))
                .then_with(|| left.directory.cmp(&right.directory))
        });

        let mut resolver = Self::from_files(&rust_files, &packages);
        resolver.config_files = config_files;
        resolver.config_files.sort();
        resolver.config_files.dedup();
        resolver.config_errors = resolver.config_errors.saturating_add(config_errors);
        resolver.config_errors_by_path = config_errors_by_path;

        let node_set = rust_files.iter().cloned().collect::<HashSet<_>>();
        for package in packages {
            if !node_set.contains(&package.lib_path) {
                continue;
            }
            let source_root = path_parent(&package.lib_path);
            let target = RustCrateTarget {
                source_root,
                root_file: package.lib_path,
            };
            for name in package.names {
                if resolver.ambiguous_crates.contains(&name) {
                    continue;
                }
                if resolver
                    .crates
                    .insert(name.clone(), target.clone())
                    .is_some()
                {
                    resolver.crates.remove(&name);
                    resolver.ambiguous_crates.insert(name);
                    resolver.config_errors = resolver.config_errors.saturating_add(1);
                    *resolver
                        .config_errors_by_path
                        .entry(package.config_path.clone())
                        .or_insert(0) += 1;
                }
            }
        }
        resolver
    }

    pub(super) fn from_files(graph_files: &[String], packages: &[RustPackage]) -> Self {
        let mut resolver = Self::default();
        for path in graph_files {
            if detect(Path::new(path)).and_then(|info| info.first_class) != Some(FirstClass::Rust) {
                continue;
            }
            let file = rust_file_module(path, packages);
            resolver
                .modules
                .entry((file.source_root.clone(), file.module.join("::")))
                .or_default()
                .push(path.clone());
            resolver.files.insert(path.clone(), file);
        }
        for paths in resolver.modules.values_mut() {
            paths.sort_by(|left, right| {
                rust_module_file_rank(left)
                    .cmp(&rust_module_file_rank(right))
                    .then_with(|| left.cmp(right))
            });
            paths.dedup();
        }
        resolver
    }

    pub(super) fn resolve(
        &self,
        importer_rel: &str,
        import: &RustImport,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        let Some(file) = self.files.get(importer_rel) else {
            return ImportResolution::Unresolved;
        };
        match import {
            RustImport::Module {
                name,
                path,
                inline_modules,
            } => {
                if let Some(path) = path {
                    let target = normalize_path(&join_graph_path(&path_parent(importer_rel), path));
                    return if nodes.contains(&target) {
                        ImportResolution::Resolved {
                            target,
                            resolver: "rust-path",
                        }
                    } else {
                        ImportResolution::Unresolved
                    };
                }
                let mut module = file.module.clone();
                module.extend(inline_modules.iter().cloned());
                module.push(name.clone());
                self.resolve_exact_module(&file.source_root, &module, importer_rel)
                    .map_or(ImportResolution::Unresolved, |target| {
                        ImportResolution::Resolved {
                            target,
                            resolver: "rust-mod",
                        }
                    })
            }
            RustImport::Use {
                path,
                inline_modules,
            } => self.resolve_use(importer_rel, file, path, inline_modules),
        }
    }

    pub(super) fn resolve_use(
        &self,
        importer_rel: &str,
        file: &RustFileModule,
        path: &str,
        inline_modules: &[String],
    ) -> ImportResolution {
        let segments = path
            .trim_start_matches("::")
            .split("::")
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        if segments.is_empty() {
            return ImportResolution::External;
        }
        match self.prepare_use(importer_rel, file, path, inline_modules, segments) {
            RustUsePreparation::Search(search) => self.finish_use_resolution(importer_rel, &search),
            RustUsePreparation::Resolution(resolution) => resolution,
        }
    }

    fn prepare_use(
        &self,
        importer_rel: &str,
        file: &RustFileModule,
        path: &str,
        inline_modules: &[String],
        segments: Vec<String>,
    ) -> RustUsePreparation {
        let cursor = RustUseCursor::new(file, inline_modules, segments);
        let first = cursor.segments.first().cloned();
        match first.as_deref() {
            Some("crate") => Self::prepare_crate_use(cursor, file, path),
            Some("super") => Self::prepare_super_use(cursor, file, path),
            Some("self") => Self::prepare_self_use(cursor, file, path),
            Some(name) => self.prepare_named_use(cursor, file, importer_rel, path, name),
            None => RustUsePreparation::Resolution(ImportResolution::External),
        }
    }

    fn prepare_crate_use(
        mut cursor: RustUseCursor,
        file: &RustFileModule,
        path: &str,
    ) -> RustUsePreparation {
        cursor.segments.remove(0);
        cursor.module.clear();
        let file_local_scope = cursor.inside_inline_module
            && cursor.segments.len() == 1
            && cursor.module.starts_with(&file.module);
        cursor.finish(path, "rust-use", file_local_scope)
    }

    fn prepare_super_use(
        mut cursor: RustUseCursor,
        file: &RustFileModule,
        path: &str,
    ) -> RustUsePreparation {
        while cursor
            .segments
            .first()
            .is_some_and(|segment| segment == "super")
        {
            cursor.segments.remove(0);
            cursor.module.pop();
        }
        let file_local_scope = cursor.inside_inline_module
            && cursor.segments.len() == 1
            && cursor.module.starts_with(&file.module);
        cursor.finish(path, "rust-use", file_local_scope)
    }

    fn prepare_self_use(
        mut cursor: RustUseCursor,
        file: &RustFileModule,
        path: &str,
    ) -> RustUsePreparation {
        cursor.segments.remove(0);
        let file_local_scope = cursor.inside_inline_module
            && cursor.segments.len() == 1
            && cursor.module.starts_with(&file.module);
        cursor.finish(path, "rust-use", file_local_scope)
    }

    fn prepare_named_use(
        &self,
        mut cursor: RustUseCursor,
        file: &RustFileModule,
        importer_rel: &str,
        path: &str,
        name: &str,
    ) -> RustUsePreparation {
        if self.ambiguous_crates.contains(name) {
            return RustUsePreparation::Resolution(ImportResolution::Unresolved);
        }
        let Some(target) = self.crates.get(name) else {
            cursor.module.clear();
            return cursor.finish(path, "rust-use", false);
        };
        cursor.segments.remove(0);
        cursor.source_root.clone_from(&target.source_root);
        cursor.module.clear();
        if cursor.segments.is_empty() {
            return RustUsePreparation::Resolution(if target.root_file == importer_rel {
                ImportResolution::Local
            } else {
                ImportResolution::Resolved {
                    target: target.root_file.clone(),
                    resolver: "rust-workspace",
                }
            });
        }
        let file_local_scope = cursor.inside_inline_module
            && target.root_file == importer_rel
            && cursor.segments.len() == 1
            && cursor.module.starts_with(&file.module);
        cursor.finish(path, "rust-workspace", file_local_scope)
    }

    fn finish_use_resolution(
        &self,
        importer_rel: &str,
        search: &RustUseSearch,
    ) -> ImportResolution {
        match self.resolve_module_prefix(&search.source_root, &search.module, importer_rel) {
            Some(target) if target == importer_rel => ImportResolution::Local,
            Some(target) => ImportResolution::Resolved {
                target,
                resolver: search.resolver,
            },
            None if search.file_local_scope => ImportResolution::Local,
            None if search.must_resolve => ImportResolution::Unresolved,
            None => ImportResolution::External,
        }
    }

    pub(super) fn resolve_exact_module(
        &self,
        source_root: &str,
        module: &[String],
        importer_rel: &str,
    ) -> Option<String> {
        self.modules
            .get(&(source_root.to_string(), module.join("::")))?
            .iter()
            .find(|target| target.as_str() != importer_rel)
            .cloned()
    }

    pub(super) fn resolve_module_prefix(
        &self,
        source_root: &str,
        module: &[String],
        importer_rel: &str,
    ) -> Option<String> {
        for length in (1..=module.len()).rev() {
            let key = (source_root.to_string(), module[..length].join("::"));
            if let Some(target) = self.modules.get(&key).and_then(|targets| {
                targets
                    .iter()
                    .find(|target| target.as_str() == importer_rel)
                    .or_else(|| targets.first())
                    .cloned()
            }) {
                return Some(target);
            }
        }
        None
    }
}

pub(super) struct ConfigAccess<'a> {
    pub(super) root: &'a Path,
    pub(super) budget: &'a mut ReadBudget,
    pub(super) snapshot: Option<&'a BTreeMap<String, String>>,
}

impl ConfigAccess<'_> {
    pub(super) fn exists(&self, relative: &str) -> bool {
        if let Some(snapshot) = self.snapshot {
            return snapshot.contains_key(relative);
        }
        fs_budget::is_regular_file(&self.root.join(relative))
    }

    pub(super) fn read(&mut self, relative: &str) -> Option<String> {
        if let Some(snapshot) = self.snapshot {
            return snapshot.get(relative).cloned();
        }
        match fs_budget::read_text(&self.root.join(relative), self.budget) {
            ReadOutcome::Content(content) => Some(content),
            _ => None,
        }
    }
}

pub(super) fn repo_is_regular_file(root: &Path, relative: &str) -> bool {
    fs_budget::is_regular_file(&root.join(relative))
}

pub(super) fn read_repo_text(
    root: &Path,
    relative: &str,
    budget: &mut ReadBudget,
) -> Option<String> {
    match fs_budget::read_text(&root.join(relative), budget) {
        ReadOutcome::Content(content) => Some(content),
        _ => None,
    }
}

pub(super) fn candidate_resolver_config_paths(
    root: &Path,
    graph_files: &[String],
) -> BTreeSet<String> {
    let mut candidates = BTreeSet::new();
    if root.is_file() {
        return candidates;
    }
    for path in graph_files {
        let fc = detect(Path::new(path)).and_then(|info| info.first_class);
        let mut directory = path_parent(path);
        loop {
            match fc {
                Some(FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx) => {
                    for name in ["tsconfig.json", "jsconfig.json", "package.json"] {
                        candidates.insert(join_graph_path(&directory, name));
                    }
                }
                Some(FirstClass::Rust) => {
                    candidates.insert(join_graph_path(&directory, "Cargo.toml"));
                }
                Some(FirstClass::Go) => {
                    candidates.insert(join_graph_path(&directory, "go.mod"));
                }
                Some(FirstClass::Php) => {
                    candidates.insert(join_graph_path(&directory, "composer.json"));
                }
                _ => {}
            }
            if directory.is_empty() {
                break;
            }
            directory = path_parent(&directory);
        }
    }
    candidates
        .into_iter()
        .filter(|relative| repo_is_regular_file(root, relative))
        .collect()
}

pub(super) fn read_cargo_package(
    access: &mut ConfigAccess<'_>,
    relative: &str,
) -> Option<Result<Option<RustPackage>, ()>> {
    let content = access.read(relative)?;
    let Ok(value) = toml::from_str::<toml::Value>(&content) else {
        return Some(Err(()));
    };
    let Some(package_name) = value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
    else {
        return Some(Ok(None));
    };
    let directory = path_parent(relative);
    let lib = value.get("lib");
    let lib_name = lib
        .and_then(|lib| lib.get("name"))
        .and_then(toml::Value::as_str)
        .unwrap_or(package_name);
    let lib_path = lib
        .and_then(|lib| lib.get("path"))
        .and_then(toml::Value::as_str)
        .unwrap_or("src/lib.rs");
    let mut names = vec![rust_crate_name(package_name), rust_crate_name(lib_name)];
    names.sort();
    names.dedup();
    Some(Ok(Some(RustPackage {
        config_path: relative.to_string(),
        directory: directory.clone(),
        names,
        lib_path: join_graph_path(&directory, lib_path),
    })))
}

pub(super) fn rust_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

pub(super) fn rust_file_module(path: &str, packages: &[RustPackage]) -> RustFileModule {
    let package = packages
        .iter()
        .find(|package| path_in_scope(path, &package.directory));
    let package_directory = package.map_or("", |package| package.directory.as_str());
    let relative = strip_graph_prefix(path, package_directory).unwrap_or(path);
    let parts = relative.split('/').collect::<Vec<_>>();

    let (source_relative, treat_top_level_as_root) = if parts.first() == Some(&"src") {
        ("src".to_string(), false)
    } else if matches!(parts.first(), Some(&"tests" | &"examples" | &"benches")) {
        (parts[0].to_string(), true)
    } else if let Some(index) = parts.iter().position(|part| *part == "src") {
        (parts[..=index].join("/"), false)
    } else {
        (String::new(), false)
    };
    let source_root = join_graph_path(package_directory, &source_relative);
    let relative_to_root = strip_graph_prefix(path, &source_root).unwrap_or(path);
    let mut module_parts = relative_to_root
        .split('/')
        .map(str::to_string)
        .collect::<Vec<_>>();
    let file_name = module_parts.pop().unwrap_or_default();
    let stem = Path::new(&file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if stem == "mod" {
        // The directory already names this module.
    } else if (matches!(stem, "lib" | "main") || treat_top_level_as_root) && module_parts.is_empty()
    {
        module_parts.clear();
    } else if !stem.is_empty() {
        module_parts.push(stem.to_string());
    }
    RustFileModule {
        source_root,
        module: module_parts,
    }
}

pub(super) fn rust_scope_rank(directory: &str) -> usize {
    directory.split('/').filter(|part| !part.is_empty()).count()
}

pub(super) fn rust_module_file_rank(path: &str) -> usize {
    match Path::new(path).file_name().and_then(|name| name.to_str()) {
        Some("lib.rs") => 0,
        Some("main.rs") => 1,
        Some("mod.rs") => 2,
        _ => 3,
    }
}
