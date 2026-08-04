use super::{Config, resolve_with_global, resolve_with_options};
use crate::dup::{DuplicationFormatScope, DuplicationMode};
use crate::lang::{HealthInclude, HealthScope};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn load_project(path: &Path) -> Config {
    resolve_with_global(path, None).unwrap().config
}

#[test]
fn invalid_config_is_reported_instead_of_silently_ignored() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("reposcout.toml");
    fs::write(&config, "unknown_setting = true\n").unwrap();

    let error = resolve_with_global(dir.path(), None)
        .unwrap_err()
        .to_string();

    assert!(error.contains("failed to parse config"));
    assert!(error.contains("reposcout.toml"));
}

#[test]
fn project_configuration_cannot_disable_absolute_limits() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("reposcout.toml"),
        concat!(
            "jobs = 999999\n",
            "top = 999999\n",
            "churn_max_commits = 0\n",
            "min_dup_tokens = 0\n",
            "near_dup_min_similarity = 0.01\n",
            "max_file_bytes = 999999999999\n",
            "max_total_bytes = 999999999999\n",
            "max_files = 999999999\n",
            "max_git_blob_bytes = 999999999999\n",
            "max_scan_seconds = 999999999\n",
            "[context]\n",
            "budget = 999999999\n",
            "max_files = 999999999\n",
        ),
    )
    .unwrap();

    let config = load_project(dir.path());

    assert_eq!(config.jobs, 64);
    assert_eq!(config.top, 1_000);
    assert_eq!(config.churn_max_commits, 100_000);
    assert_eq!(config.min_dup_tokens, 8);
    assert!((config.near_dup_min_similarity - 0.5).abs() < f64::EPSILON);
    assert_eq!(config.max_file_bytes, 256 * 1024 * 1024);
    assert_eq!(config.max_total_bytes, 4 * 1024 * 1024 * 1024);
    assert_eq!(config.max_files, 500_000);
    assert_eq!(config.max_git_blob_bytes, 256 * 1024 * 1024);
    assert_eq!(config.max_scan_seconds, 7_200);
    assert_eq!(config.context_budget, 5_000_000);
    assert_eq!(config.context_max_files, 10_000);
}

#[test]
fn configuration_files_have_a_size_limit() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("reposcout.toml"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .unwrap();

    let error = resolve_with_global(dir.path(), None)
        .unwrap_err()
        .to_string();

    assert!(error.contains("1 MiB size limit"), "error was: {error}");
}

#[test]
fn duplication_options_load_from_config() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("reposcout.toml");
    fs::write(
            &config,
            "duplication_mode = \"weak\"\nduplication_format_scope = \"compatible\"\nduplication_include_artifacts = true\nduplication_report_snippets = true\n",
        )
        .unwrap();

    let loaded = load_project(dir.path());

    assert_eq!(loaded.duplication_mode, DuplicationMode::Weak);
    assert_eq!(
        loaded.duplication_format_scope,
        DuplicationFormatScope::Compatible
    );
    assert!(loaded.duplication_include_artifacts);
    assert!(loaded.duplication_report_snippets);
}

#[test]
fn health_file_policy_loads_from_config() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("reposcout.toml");
    fs::write(
        &config,
        concat!(
            "health_scope = \"all\"\n",
            "health_includes = [\"json\", \"css\"]\n",
            "health_excludes = [\"vendor/**\", \"generated/**\"]\n",
        ),
    )
    .unwrap();

    let loaded = load_project(dir.path());

    assert_eq!(loaded.health_scope, HealthScope::All);
    assert_eq!(
        loaded.health_includes,
        vec![HealthInclude::Json, HealthInclude::Css]
    );
    assert_eq!(loaded.health_excludes, vec!["vendor/**", "generated/**"]);
}

#[test]
fn function_complexity_maximum_loads_from_config() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("reposcout.toml"), "max_complexity = 12\n").unwrap();

    let loaded = load_project(dir.path());

    assert_eq!(loaded.max_complexity, 12);
}

#[test]
fn project_explicit_fields_override_global_and_omissions_inherit() {
    let dir = tempdir().unwrap();
    let global = dir.path().join("global.toml");
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(
            &global,
            "jobs = 3\ntop = 20\nmarkers = [\"GLOBAL\"]\nexcludes = [\"global/**\"]\n\n[context]\nenabled = true\nbudget = 9000\n",
        )
        .unwrap();
    fs::write(
        project.join("reposcout.toml"),
        "top = 7\nmarkers = [\"PROJECT\"]\n\n[context]\nmax_files = 8\n",
    )
    .unwrap();

    let resolved = resolve_with_global(&project, Some(global)).unwrap();

    assert_eq!(resolved.config.jobs, 3, "omitted project field inherits");
    assert_eq!(resolved.config.top, 7, "project field wins");
    assert_eq!(resolved.config.markers, ["PROJECT"]);
    assert_eq!(resolved.config.extra_excludes, ["global/**"]);
    assert!(resolved.config.context);
    assert_eq!(resolved.config.context_budget, 9000);
    assert_eq!(resolved.config.context_max_files, 8);
}

#[test]
fn explicitly_defined_project_lists_replace_global_lists() {
    let dir = tempdir().unwrap();
    let global = dir.path().join("global.toml");
    fs::write(&global, "excludes = [\"global/**\"]\n").unwrap();
    fs::write(
        dir.path().join("reposcout.toml"),
        "excludes = [\"project/**\"]\n",
    )
    .unwrap();

    let loaded = resolve_with_global(dir.path(), Some(global))
        .unwrap()
        .config;

    assert_eq!(loaded.extra_excludes, ["project/**"]);
}

#[test]
fn nearest_project_config_is_discovered_from_a_nested_path() {
    let dir = tempdir().unwrap();
    let nested = dir.path().join("workspace/crate/src");
    fs::create_dir_all(&nested).unwrap();
    fs::write(dir.path().join("reposcout.toml"), "top = 30\n").unwrap();
    fs::write(dir.path().join("workspace/reposcout.toml"), "top = 12\n").unwrap();

    let resolved = resolve_with_global(&nested, None).unwrap();

    assert_eq!(resolved.config.top, 12);
    assert_eq!(
        resolved.sources.project.unwrap().path,
        dir.path()
            .join("workspace/reposcout.toml")
            .canonicalize()
            .unwrap()
    );
}

#[test]
fn ignored_project_config_is_discovered_but_never_parsed_or_applied() {
    let dir = tempdir().unwrap();
    let global = dir.path().join("global.toml");
    fs::write(&global, "jobs = 2\n").unwrap();
    let project = dir.path().join("reposcout.toml");
    fs::write(&project, "this is deliberately invalid TOML = [\n").unwrap();

    let resolved = resolve_with_options(dir.path(), Some(global), false).unwrap();

    assert_eq!(resolved.config.jobs, 2);
    assert_eq!(resolved.config.config_mode, "user");
    let canonical_project = project.canonicalize().unwrap();
    assert_eq!(
        resolved.config.project_config_path.as_deref(),
        Some(canonical_project.as_path())
    );
    let source = resolved.sources.project.unwrap();
    assert!(source.ignored);
    assert!(!source.loaded);
    assert!(source.keys.is_empty());
}

#[test]
fn config_mode_distinguishes_defaults_user_and_project_sources() {
    let defaults = tempdir().unwrap();
    assert_eq!(
        resolve_with_options(defaults.path(), None, true)
            .unwrap()
            .config
            .config_mode,
        "defaults"
    );

    let user = tempdir().unwrap();
    let global = user.path().join("global.toml");
    fs::write(&global, "jobs = 2\n").unwrap();
    assert_eq!(
        resolve_with_options(user.path(), Some(global), true)
            .unwrap()
            .config
            .config_mode,
        "user"
    );

    let project = tempdir().unwrap();
    fs::write(project.path().join("reposcout.toml"), "jobs = 2\n").unwrap();
    assert_eq!(
        resolve_with_options(project.path(), None, true)
            .unwrap()
            .config
            .config_mode,
        "project"
    );
}

#[test]
fn invalid_global_config_identifies_the_file_and_setting() {
    let dir = tempdir().unwrap();
    let global = dir.path().join("global.toml");
    fs::write(&global, "unknown_global_setting = true\n").unwrap();

    let error = resolve_with_global(dir.path(), Some(global.clone()))
        .unwrap_err()
        .to_string();

    assert!(error.contains(global.to_string_lossy().as_ref()));
    assert!(error.contains("unknown_global_setting"));
}

#[test]
fn missing_global_config_is_reported_but_not_an_error() {
    let dir = tempdir().unwrap();
    let global = dir.path().join("missing-global.toml");

    let resolved = resolve_with_global(dir.path(), Some(global.clone())).unwrap();
    let source = resolved.sources.global.unwrap();

    assert_eq!(source.path, global);
    assert!(!source.loaded);
    assert!(source.keys.is_empty());
    assert_eq!(resolved.config.context_budget, 32_000);
}

#[test]
fn invalid_nested_setting_identifies_its_name() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("reposcout.toml"),
        "[context]\nbudegt = 1000\n",
    )
    .unwrap();

    let error = resolve_with_global(dir.path(), None)
        .unwrap_err()
        .to_string();

    assert!(error.contains("budegt"), "error was: {error}");
    assert!(error.contains("reposcout.toml"), "error was: {error}");
}

#[test]
fn inspection_reports_loaded_layers_and_defined_keys() {
    let dir = tempdir().unwrap();
    let global = dir.path().join("global.toml");
    fs::write(&global, "jobs = 2\n").unwrap();
    fs::write(dir.path().join("reposcout.toml"), "top = 4\n").unwrap();

    let inspection = resolve_with_global(dir.path(), Some(global))
        .unwrap()
        .inspection();

    assert_eq!(
        inspection.precedence,
        ["cli", "project", "global", "defaults"]
    );
    assert_eq!(inspection.sources.global.unwrap().keys, ["jobs"]);
    assert_eq!(inspection.sources.project.unwrap().keys, ["top"]);
    assert_eq!(inspection.effective.jobs, 2);
    assert_eq!(inspection.effective.top, 4);
}
