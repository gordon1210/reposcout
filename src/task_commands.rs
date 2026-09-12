use super::{
    Config, ExecutionProfile, ExitCode, Format, Path, ReadArgs, Result, apply_common_overrides,
    apply_execution_profile, command_exclusions, enforce_absolute_limits, enforce_safe_limits,
    log_configuration, parse_source_revision, read_selector_paths, require_json_for_pretty,
    rooted_read_path, source_query_targets, usage_error, validate_read_output_path, walk,
    write_file_output, write_stdout,
};
use reposcout::cli::{Cli, Command, CommonArgs, ConsumersArgs, FindArgs, PlanArgs, ScanArgs};
use std::io::IsTerminal;

fn query_configuration(
    operation: &'static str,
    path: &Path,
    common: &CommonArgs,
) -> Result<Config> {
    let profile = common.profile.unwrap_or(ExecutionProfile::Agent);
    let mut cfg = if common.no_project_config || profile == ExecutionProfile::Safe {
        Config::load_without_project(path)?
    } else {
        Config::load(path)?
    };
    apply_execution_profile(&mut cfg, profile);
    apply_common_overrides(&mut cfg, common);
    enforce_absolute_limits(&mut cfg);
    if profile == ExecutionProfile::Safe {
        enforce_safe_limits(&mut cfg);
    }
    log_configuration(operation, path, &cfg);
    Ok(cfg)
}

fn query_format(common: &CommonArgs, pretty: bool) -> Result<Format> {
    let format = super::choose_format(common.format, common.output.as_deref());
    if matches!(format, Format::Sarif | Format::Dot | Format::Mermaid) {
        return Err(usage_error(
            "task queries support table, JSON, Markdown, or NDJSON output",
        ));
    }
    require_json_for_pretty(pretty, format == Format::Json)?;
    Ok(format)
}

fn write_query(common: &CommonArgs, root: &Path, rendered: &str) -> Result<ExitCode> {
    match common.output.as_deref() {
        Some(path) => write_file_output(path, rendered.as_bytes(), root)?,
        None => write_stdout(rendered)?,
    }
    Ok(ExitCode::SUCCESS)
}

pub(super) fn run_find(args: &FindArgs, pretty: bool) -> Result<ExitCode> {
    let format = query_format(&args.common, pretty)?;
    if let Some(output) = &args.common.output
        && walk::exact_path_identity(output)? == walk::exact_path_identity(&args.path)?
    {
        return Err(usage_error("output path cannot be the query target"));
    }
    let cfg = query_configuration("find", &args.path, &args.common)?;
    let output = reposcout::query::find(
        &args.path,
        &cfg,
        &command_exclusions(args.common.output.as_deref()),
        &reposcout::query::FindQueryOptions {
            query: args.query.clone(),
            match_mode: args.match_mode.into(),
            kind: args.kind.clone(),
            language: args.language.clone(),
            limit: args.limit,
            token_budget: args.budget,
            byte_budget: args.max_output_bytes,
            format,
            pretty_json: pretty,
        },
    )?;
    write_query(&args.common, &args.path, &output.rendered)
}

fn plan_read_args(args: &PlanArgs) -> ReadArgs {
    ReadArgs {
        path: args.path.clone(),
        common: args.common.clone(),
        snapshot: args.snapshot.clone(),
        symbol: args.symbol.clone(),
        line: args.line.clone(),
        outline: args.file.clone(),
        expect_hash: args.expect_hash.clone(),
        budget: args.budget,
        max_output_bytes: args.max_output_bytes,
    }
}

fn consumers_read_args(args: &ConsumersArgs) -> ReadArgs {
    ReadArgs {
        path: args.path.clone(),
        common: args.common.clone(),
        snapshot: "worktree".to_string(),
        symbol: args.symbol.clone(),
        line: args.line.clone(),
        outline: Vec::new(),
        expect_hash: args.expect_hash.clone(),
        budget: args.budget,
        max_output_bytes: args.max_output_bytes,
    }
}

pub(super) fn run_consumers(args: &ConsumersArgs, pretty: bool) -> Result<ExitCode> {
    let format = query_format(&args.common, pretty)?;
    let read_args = consumers_read_args(args);
    let targets = source_query_targets(&read_args)?;
    validate_read_output_path(&read_args, &targets)?;
    let cfg = query_configuration("consumers", &args.path, &args.common)?;
    let output = reposcout::query::consumers(
        &args.path,
        &cfg,
        &command_exclusions(args.common.output.as_deref()),
        &reposcout::query::ConsumersQueryOptions {
            targets,
            direction: args.direction.into(),
            depth: args.depth,
            limit: args.limit,
            path_limit: args.path_limit,
            token_budget: args.budget,
            byte_budget: args.max_output_bytes,
            format,
            pretty_json: pretty,
        },
    )?;
    write_query(&args.common, &args.path, &output.rendered)
}

pub(super) fn run_plan(args: &PlanArgs, pretty: bool) -> Result<ExitCode> {
    let format = query_format(&args.common, pretty)?;
    let read_args = plan_read_args(args);
    let targets = if args.symbol.is_empty() && args.line.is_empty() && args.file.is_empty() {
        if !args.expect_hash.is_empty() {
            return Err(usage_error("--expect-hash requires a selected file"));
        }
        Vec::new()
    } else {
        source_query_targets(&read_args)?
    };
    validate_read_output_path(&read_args, &targets)?;
    let cfg = query_configuration("plan", &args.path, &args.common)?;
    let output = reposcout::query::plan_definitions(
        &args.path,
        &cfg,
        &command_exclusions(args.common.output.as_deref()),
        &reposcout::query::DefinitionPlanQueryOptions {
            targets,
            snapshot: parse_source_revision(&args.snapshot)?,
            context_budget: args.context_budget,
            token_budget: args.budget,
            byte_budget: args.max_output_bytes,
            max_files: args.max_plan_files,
            max_definitions: args.max_definitions,
            include_source: args.source,
            format,
            pretty_json: pretty,
        },
    )?;
    write_query(&args.common, &args.path, &output.rendered)
}

pub(super) fn diagnostic_input_path(args: &ScanArgs) -> Option<&Path> {
    args.task_diagnostics
        .as_deref()
        .filter(|path| *path != Path::new("-"))
}

pub(super) fn load_diagnostics(
    args: &ScanArgs,
    profile: ExecutionProfile,
) -> Result<Option<reposcout::task_diagnostics::ParsedTaskDiagnostics>> {
    let Some(input) = args.task_diagnostics.as_deref() else {
        return Ok(None);
    };
    let limits = reposcout::task_diagnostics::TaskDiagnosticLimits::for_safe(
        profile == ExecutionProfile::Safe,
    );
    let format = args.task_diagnostics_format.unwrap_or_default().into();
    let parsed = if input == Path::new("-") {
        if std::io::stdin().is_terminal() {
            return Err(usage_error(
                "--task-diagnostics - requires piped diagnostic input",
            ));
        }
        reposcout::task_diagnostics::read_input(std::io::stdin().lock(), format, limits)
    } else {
        reposcout::task_diagnostics::load_file(input, format, limits)
    }?;
    Ok(Some(parsed))
}

fn scan_args(cli: &Cli) -> Option<&ScanArgs> {
    match &cli.command {
        None => Some(&cli.args),
        Some(
            Command::Tokens(args)
            | Command::Complexity(args)
            | Command::Dup(args)
            | Command::Churn(args)
            | Command::Metrics(args),
        ) => Some(args),
        _ => None,
    }
}

pub(super) fn validate_debug_paths(cli: &Cli, debug_identity: &Path) -> Result<()> {
    if let Some(input) = scan_args(cli).and_then(diagnostic_input_path)
        && debug_identity == walk::exact_path_identity(input)?
    {
        return Err(usage_error(
            "debug log path cannot overwrite the diagnostic input",
        ));
    }
    if let Some(Command::Plan(args)) = &cli.command {
        let read_args = plan_read_args(args);
        for path in read_selector_paths(&read_args) {
            if debug_identity == walk::exact_path_identity(&rooted_read_path(&args.path, path))? {
                return Err(usage_error(
                    "debug log path cannot overwrite a selected source file",
                ));
            }
        }
    }
    if let Some(Command::Consumers(args)) = &cli.command {
        let read_args = consumers_read_args(args);
        for path in read_selector_paths(&read_args) {
            if debug_identity == walk::exact_path_identity(&rooted_read_path(&args.path, path))? {
                return Err(usage_error(
                    "debug log path cannot overwrite a selected source file",
                ));
            }
        }
    }
    Ok(())
}
