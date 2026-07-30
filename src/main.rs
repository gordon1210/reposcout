use anyhow::{Result, anyhow};
use clap::{Parser, error::ErrorKind};
use reposcout::cli::{
    CacheArgs, CacheCommand, CapabilitiesArgs, Cli, Command, CommonArgs, ConfigArgs,
    ConfigOutputFormat, DaemonArgs, DaemonProfile, ErrorFormat, ExecutionProfile, ExplainArgs,
    LocateArgs, OutputFormat, ScanArgs,
};
use reposcout::config::{Config, Enabled};
use reposcout::debug_log;
use reposcout::lang::{HealthInclude, HealthScope};
use reposcout::model::Summary;
use reposcout::report::{self, Format};
use reposcout::scan;
use reposcout::walk;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

#[derive(Debug)]
struct UsageError(String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

fn usage_error(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(UsageError(message.into()))
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            let code = error.exit_code();
            if json_errors_requested() {
                print_json_error("usage", &error.to_string(), code);
            } else {
                let _ = error.print();
            }
            return exit_code(code);
        }
    };
    let error_format = cli.error_format;
    if let Err(error) = validate_debug_log_paths(&cli) {
        let message = format!("{error:#}");
        if error_format == ErrorFormat::Json {
            print_json_error("runtime", &message, 1);
        } else {
            eprintln!("reposcout: error: {message}");
        }
        return ExitCode::FAILURE;
    }
    let mut debug_session = match debug_log::Session::start(cli.debug_log.as_deref()) {
        Ok(session) => session,
        Err(error) => {
            let message = format!("{error:#}");
            if error_format == ErrorFormat::Json {
                print_json_error("runtime", &message, 1);
            } else {
                eprintln!("reposcout: error: {message}");
            }
            return ExitCode::FAILURE;
        }
    };
    match real_main(cli) {
        Ok(code) => {
            debug_session.finish(if code == ExitCode::SUCCESS {
                "completed"
            } else {
                "nonzero"
            });
            code
        }
        Err(e) => {
            let message = format!("{e:#}");
            let usage = e.downcast_ref::<UsageError>().is_some();
            let category = if usage { "usage" } else { "runtime" };
            let code = if usage { 2 } else { 1 };
            debug_log::event(
                "runtime_error",
                || serde_json::json!({ "message": &message }),
            );
            debug_session.finish("error");
            if error_format == ErrorFormat::Json {
                print_json_error(category, &message, code);
            } else {
                eprintln!("reposcout: error: {message}");
            }
            exit_code(code)
        }
    }
}

fn validate_debug_log_paths(cli: &Cli) -> Result<()> {
    let Some(debug_log) = cli.debug_log.as_deref() else {
        return Ok(());
    };
    let debug_identity = walk::exact_path_identity(debug_log)?;
    if let Some(target) = command_target(cli)
        && debug_identity == walk::exact_path_identity(target)?
    {
        return Err(anyhow!("debug log path cannot be the command target"));
    }
    if let Some(output) = command_output(cli)
        && debug_identity == walk::exact_path_identity(output)?
    {
        return Err(anyhow!("debug log path cannot also be the output path"));
    }
    Ok(())
}

fn command_target(cli: &Cli) -> Option<&Path> {
    match &cli.command {
        None => Some(&cli.args.path),
        Some(
            Command::Tokens(args)
            | Command::Complexity(args)
            | Command::Dup(args)
            | Command::Churn(args)
            | Command::Metrics(args),
        ) => Some(&args.path),
        Some(Command::Explain(args)) => Some(&args.file),
        Some(Command::Locate(args)) => Some(&args.path),
        Some(Command::Config(args)) => Some(&args.path),
        Some(Command::Daemon(args)) => Some(&args.path),
        Some(Command::Cache(args)) => {
            let CacheCommand::Clear(args) = &args.command;
            args.path.as_deref()
        }
        Some(Command::Capabilities(_) | Command::Update) => None,
    }
}

fn command_output(cli: &Cli) -> Option<&Path> {
    match &cli.command {
        None => cli.args.common.output.as_deref(),
        Some(
            Command::Tokens(args)
            | Command::Complexity(args)
            | Command::Dup(args)
            | Command::Churn(args)
            | Command::Metrics(args),
        ) => args.common.output.as_deref(),
        Some(Command::Explain(args)) => args.common.output.as_deref(),
        Some(Command::Locate(args)) => args.common.output.as_deref(),
        Some(
            Command::Capabilities(_)
            | Command::Cache(_)
            | Command::Config(_)
            | Command::Daemon(_)
            | Command::Update,
        ) => None,
    }
}

fn real_main(cli: Cli) -> Result<ExitCode> {
    match cli {
        Cli {
            command: Some(Command::Capabilities(args)),
            ..
        } => run_capabilities(args),
        Cli {
            command: Some(Command::Cache(args)),
            ..
        } => run_cache(args),
        Cli {
            command: Some(Command::Daemon(args)),
            ..
        } => run_daemon(args),
        Cli {
            command: Some(Command::Explain(args)),
            ..
        } => run_explain(args),
        Cli {
            command: Some(Command::Config(args)),
            ..
        } => run_config(args),
        Cli {
            command: Some(Command::Locate(args)),
            ..
        } => run_locate(args),
        Cli {
            command: Some(Command::Update),
            ..
        } => {
            let rendered = reposcout::update::run()?;
            write_stdout(&rendered)?;
            Ok(ExitCode::SUCCESS)
        }
        cli => run_scan(cli),
    }
}

fn run_cache(args: CacheArgs) -> Result<ExitCode> {
    let CacheCommand::Clear(args) = args.command;
    let result = if args.all {
        reposcout::cache::clear_all()?
    } else {
        reposcout::cache::clear_for_target(args.path.as_deref().unwrap_or_else(|| Path::new(".")))?
    };

    let rendered = match &result.scope {
        reposcout::cache::CacheClearScope::All(directory) if result.removed.is_empty() => {
            format!(
                "RepoScout cache is already empty at {}.\n",
                directory.display()
            )
        }
        reposcout::cache::CacheClearScope::All(directory) => {
            format!("Cleared all RepoScout caches at {}.\n", directory.display())
        }
        reposcout::cache::CacheClearScope::ScanRoot(root) if result.removed.is_empty() => {
            format!("RepoScout cache is already empty for {}.\n", root.display())
        }
        reposcout::cache::CacheClearScope::ScanRoot(root) => {
            let mut output = format!("Cleared RepoScout cache for {}:\n", root.display());
            for location in &result.removed {
                output.push_str(&format!(
                    "  {}: {}\n",
                    location.kind.label(),
                    location.path.display()
                ));
            }
            output
        }
    };
    write_stdout(&rendered)?;
    Ok(ExitCode::SUCCESS)
}

fn json_errors_requested() -> bool {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    args.iter().any(|arg| arg == "--error-format=json")
        || args
            .windows(2)
            .any(|pair| pair[0] == "--error-format" && pair[1] == "json")
}

fn print_json_error(category: &str, message: &str, code: i32) {
    eprintln!(
        "{}",
        serde_json::json!({
            "kind": "error",
            "category": category,
            "message": message,
            "exit_code": code,
        })
    );
}

fn exit_code(code: i32) -> ExitCode {
    u8::try_from(code)
        .map(ExitCode::from)
        .unwrap_or(ExitCode::FAILURE)
}

fn run_capabilities(args: CapabilitiesArgs) -> Result<ExitCode> {
    let format = args.format.unwrap_or_else(|| {
        if std::io::stdout().is_terminal() {
            ConfigOutputFormat::Table
        } else {
            ConfigOutputFormat::Json
        }
    });
    let rendered = report::render_capabilities(&reposcout::query::capabilities(), format)?;
    write_stdout(&rendered)?;
    Ok(ExitCode::SUCCESS)
}

fn run_config(args: ConfigArgs) -> Result<ExitCode> {
    let mut resolved = if args.no_project_config || args.profile == ExecutionProfile::Safe {
        Config::resolve_without_project(&args.path)?
    } else {
        Config::resolve(&args.path)?
    };
    apply_execution_profile(&mut resolved.config, args.profile);
    enforce_absolute_limits(&mut resolved.config);
    if args.profile == ExecutionProfile::Safe {
        enforce_safe_limits(&mut resolved.config);
    }
    let format = args.format.unwrap_or_else(|| {
        if std::io::stdout().is_terminal() {
            ConfigOutputFormat::Table
        } else {
            ConfigOutputFormat::Json
        }
    });
    let rendered = report::config::render(&resolved.inspection(), format)?;
    write_stdout(&rendered)?;
    Ok(ExitCode::SUCCESS)
}

fn run_daemon(args: DaemonArgs) -> Result<ExitCode> {
    let safe = args.profile == DaemonProfile::Safe;
    let mut cfg = if args.no_project_config || safe {
        Config::load_without_project(&args.path)?
    } else {
        Config::load(&args.path)?
    };
    cfg.quiet_progress = true;
    cfg.execution_profile = match args.profile {
        DaemonProfile::Lite => "lite",
        DaemonProfile::Full => "full",
        DaemonProfile::Safe => "safe",
    }
    .to_string();
    if matches!(args.profile, DaemonProfile::Lite | DaemonProfile::Safe) {
        cfg.enabled.duplication = false;
        cfg.enabled.churn = false;
    }
    enforce_absolute_limits(&mut cfg);
    if safe {
        enforce_safe_limits(&mut cfg);
    }
    log_configuration("daemon", &args.path, &cfg);
    reposcout::daemon::run(
        args.path,
        cfg,
        reposcout::daemon::DaemonOptions {
            host: args.host,
            port: args.port,
            debounce: std::time::Duration::from_millis(args.debounce_ms),
            profile: match args.profile {
                DaemonProfile::Lite => "lite",
                DaemonProfile::Full => "full",
                DaemonProfile::Safe => "safe",
            },
            unsafe_no_auth: args.unsafe_no_auth,
            allow_insecure_remote: args.allow_insecure_remote,
        },
    )?;
    Ok(ExitCode::SUCCESS)
}

fn run_scan(cli: Cli) -> Result<ExitCode> {
    let (args, sub_enabled) = split(cli);
    if args.change_summary && args.since.is_none() && !args.staged && !args.working {
        return Err(usage_error(
            "--change-summary requires exactly one of --since, --staged, or --working",
        ));
    }
    if args.change_summary && sub_enabled.is_some() {
        return Err(usage_error(
            "--change-summary is available only on the default scan command",
        ));
    }
    if args.change_summary && args.baseline_ready {
        return Err(usage_error(
            "--change-summary cannot be combined with --baseline-ready",
        ));
    }
    let requested_format = choose_format(args.common.format, args.common.output.as_deref());
    if args.change_summary
        && matches!(
            requested_format,
            Format::Sarif | Format::Dot | Format::Mermaid
        )
    {
        return Err(usage_error(
            "--change-summary supports table, JSON, Markdown, or NDJSON output",
        ));
    }
    let cli_requested_context = args.context
        || args.context_budget.is_some()
        || args.context_max_files.is_some()
        || !args.focus.is_empty()
        || args.change_summary;
    if args
        .graph_depth
        .is_some_and(|depth| depth > reposcout::query::MAX_GRAPH_DEPTH)
    {
        return Err(anyhow!(
            "--graph-depth must be between 0 and {}",
            reposcout::query::MAX_GRAPH_DEPTH
        ));
    }

    if sub_enabled.is_some() && !args.only.is_empty() {
        return Err(anyhow!("--only cannot be used with an analyzer subcommand"));
    }

    let profile = args.common.profile.unwrap_or(if args.change_summary {
        ExecutionProfile::Agent
    } else {
        ExecutionProfile::Full
    });
    let mut cfg = if args.common.no_project_config || profile == ExecutionProfile::Safe {
        Config::load_without_project(&args.path)?
    } else {
        Config::load(&args.path)?
    };
    apply_execution_profile(&mut cfg, profile);
    apply_overrides(&mut cfg, &args);
    enforce_absolute_limits(&mut cfg);
    if let Some(en) = sub_enabled {
        cfg.enabled = en;
    } else if !args.only.is_empty() {
        cfg.enabled = parse_only(&args.only)?;
    }
    if profile == ExecutionProfile::Safe {
        enforce_safe_limits(&mut cfg);
    }
    if cfg.context && !cfg.enabled.tokens {
        if cli_requested_context {
            return Err(anyhow!(
                "context planning requires the tokens analyzer; include tokens in --only or use a full scan"
            ));
        }
        cfg.context = false;
    }
    log_configuration("scan", &args.path, &cfg);

    let fail_conditions = args
        .fail_on
        .as_deref()
        .map(|expr| parse_fail_on(expr, cfg.enabled))
        .transpose()?
        .unwrap_or_default();

    if let Some(output) = args.common.output.as_deref()
        && walk::exact_path_identity(&args.path)? == walk::exact_path_identity(output)?
    {
        return Err(anyhow!("output path cannot be the scan target"));
    }
    let exclusions = command_exclusions(args.common.output.as_deref());
    let report = scan::run_with_exclusions(&args.path, &cfg, &exclusions)?;

    if args.baseline_ready
        && let Some(format) = args.common.format
        && format != OutputFormat::Json
    {
        return Err(anyhow!("--baseline-ready requires JSON output"));
    }
    let format = if args.baseline_ready {
        Format::Json
    } else {
        requested_format
    };
    let color = matches!(format, Format::Table)
        && args.common.output.is_none()
        && std::io::stdout().is_terminal();
    let render_started = Instant::now();
    debug_log::event("render_start", || {
        serde_json::json!({
            "format": format!("{format:?}").to_lowercase(),
            "summary_only": args.summary,
            "baseline_ready": args.baseline_ready,
            "change_summary": args.change_summary,
        })
    });
    let rendered = report::render_with_options(
        &report,
        format,
        color,
        report::RenderOptions {
            summary_only: args.summary,
            baseline_ready: args.baseline_ready,
            change_summary: args.change_summary,
            duplication_details: args.duplication_details,
        },
    )?;
    debug_log::event("render_end", || {
        serde_json::json!({
            "duration_ms": u64::try_from(render_started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "bytes": rendered.len(),
        })
    });

    let output_started = Instant::now();
    debug_log::event("output_start", || {
        serde_json::json!({
            "destination": args
                .common
                .output
                .as_deref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| "stdout".to_string()),
            "bytes": rendered.len(),
        })
    });
    match args.common.output.as_deref() {
        Some(path) => std::fs::write(path, rendered.as_bytes())?,
        None => write_stdout(&rendered)?,
    }
    debug_log::event("output_end", || {
        serde_json::json!({
            "duration_ms": u64::try_from(output_started.elapsed().as_millis()).unwrap_or(u64::MAX),
        })
    });

    if evaluate_fail_on(&fail_conditions, &report.summary) {
        return Ok(ExitCode::from(2));
    }
    if args.fail_on_regression
        && report
            .baseline
            .as_ref()
            .map(|b| b.regressed)
            .unwrap_or(false)
    {
        return Ok(ExitCode::from(2));
    }
    if args.fail_on_review
        && report
            .review
            .as_ref()
            .map(|review| review.fails_gate())
            .unwrap_or(false)
    {
        return Ok(ExitCode::from(2));
    }
    Ok(ExitCode::SUCCESS)
}

fn run_explain(args: ExplainArgs) -> Result<ExitCode> {
    if let Some(output) = args.common.output.as_deref()
        && walk::exact_path_identity(&args.file)? == walk::exact_path_identity(output)?
    {
        return Err(anyhow!("output path cannot be the explained file"));
    }
    let format = choose_format(args.common.format, args.common.output.as_deref());
    if format == Format::Sarif {
        return Err(anyhow!("explain does not support SARIF output"));
    }
    if matches!(format, Format::Dot | Format::Mermaid) {
        return Err(anyhow!("explain does not support graph-only output"));
    }

    let profile = args.common.profile.unwrap_or(ExecutionProfile::Full);
    let mut cfg = if args.common.no_project_config || profile == ExecutionProfile::Safe {
        Config::load_without_project(&args.file)?
    } else {
        Config::load(&args.file)?
    };
    apply_execution_profile(&mut cfg, profile);
    apply_common_overrides(&mut cfg, &args.common);
    enforce_absolute_limits(&mut cfg);
    if profile == ExecutionProfile::Safe {
        enforce_safe_limits(&mut cfg);
    }
    cfg.context = false;
    log_configuration("explain", &args.file, &cfg);
    let exclusions = command_exclusions(args.common.output.as_deref());
    let report = reposcout::explain::run(&args.file, &cfg, &exclusions)?;
    let color =
        format == Format::Table && args.common.output.is_none() && std::io::stdout().is_terminal();
    let rendered = report::render_explain(&report, format, color)?;
    match args.common.output.as_deref() {
        Some(path) => std::fs::write(path, rendered.as_bytes())?,
        None => write_stdout(&rendered)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn run_locate(args: LocateArgs) -> Result<ExitCode> {
    if let Some(output) = args.common.output.as_deref()
        && walk::exact_path_identity(&args.path)? == walk::exact_path_identity(output)?
    {
        return Err(anyhow!("output path cannot be the query target"));
    }
    let format = choose_format(args.common.format, args.common.output.as_deref());
    if matches!(format, Format::Sarif | Format::Dot | Format::Mermaid) {
        return Err(anyhow!(
            "locate supports table, JSON, Markdown, or NDJSON output"
        ));
    }

    let profile = args.common.profile.unwrap_or(ExecutionProfile::Full);
    let mut cfg = if args.common.no_project_config || profile == ExecutionProfile::Safe {
        Config::load_without_project(&args.path)?
    } else {
        Config::load(&args.path)?
    };
    apply_execution_profile(&mut cfg, profile);
    apply_common_overrides(&mut cfg, &args.common);
    enforce_absolute_limits(&mut cfg);
    if profile == ExecutionProfile::Safe {
        enforce_safe_limits(&mut cfg);
    }
    log_configuration("locate", &args.path, &cfg);
    let exclusions = command_exclusions(args.common.output.as_deref());
    let report = reposcout::query::locate(
        &args.path,
        &cfg,
        &exclusions,
        &reposcout::query::LocateOptions {
            query: args.symbol,
            exact: args.exact,
            kind: args.kind,
            language: args.language,
            limit: args.limit,
        },
    )?;
    let color =
        format == Format::Table && args.common.output.is_none() && std::io::stdout().is_terminal();
    let rendered = report::render_symbol_query(&report, format, color)?;
    match args.common.output.as_deref() {
        Some(path) => std::fs::write(path, rendered.as_bytes())?,
        None => write_stdout(&rendered)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn write_stdout(rendered: &str) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if let Err(error) = out.write_all(rendered.as_bytes()) {
        if error.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(error.into());
    }
    if !rendered.ends_with('\n')
        && let Err(error) = writeln!(out)
    {
        if error.kind() == std::io::ErrorKind::BrokenPipe {
            return Ok(());
        }
        return Err(error.into());
    }
    Ok(())
}

fn command_exclusions(output: Option<&Path>) -> Vec<PathBuf> {
    output
        .into_iter()
        .chain(debug_log::path())
        .map(Path::to_path_buf)
        .collect()
}

fn log_configuration(operation: &'static str, target: &Path, cfg: &Config) {
    debug_log::event("configuration", || {
        serde_json::json!({
            "operation": operation,
            "target": target.to_string_lossy(),
            "profile": cfg.execution_profile.as_str(),
            "config_mode": cfg.config_mode.as_str(),
            "jobs": cfg.jobs,
            "cache_enabled": cfg.use_cache,
            "encoding": cfg.encoding.as_str(),
            "analyzers": cfg.enabled,
            "health_scope": cfg.health_scope.to_string(),
            "health_includes": cfg.health_includes.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "health_excludes": cfg.health_excludes.as_slice(),
            "context": cfg.context,
            "graph": cfg.graph,
            "impact": cfg.impact,
            "review": cfg.review.map(|mode| format!("{mode:?}").to_lowercase()),
            "diff_scope": cfg.diff_scope.as_ref().map(|scope| format!("{scope:?}")),
        })
    });
}

/// Split the parsed CLI into scan args and an optional analyzer restriction
/// implied by the chosen subcommand.
fn split(cli: Cli) -> (ScanArgs, Option<Enabled>) {
    match cli.command {
        None => (cli.args, None),
        Some(Command::Tokens(a)) => (
            a,
            Some(Enabled {
                tokens: true,
                ..Enabled::none()
            }),
        ),
        Some(Command::Complexity(a)) => (
            a,
            Some(Enabled {
                complexity: true,
                ..Enabled::none()
            }),
        ),
        Some(Command::Dup(a)) => (
            a,
            Some(Enabled {
                duplication: true,
                ..Enabled::none()
            }),
        ),
        Some(Command::Churn(a)) => (
            a,
            Some(Enabled {
                churn: true,
                ..Enabled::none()
            }),
        ),
        Some(Command::Metrics(a)) => (
            a,
            Some(Enabled {
                tokens: true,
                lines: true,
                markers: true,
                imports: true,
                ..Enabled::none()
            }),
        ),
        Some(Command::Explain(_)) => unreachable!("explain is dispatched before scan splitting"),
        Some(Command::Locate(_)) => unreachable!("locate is dispatched before scan splitting"),
        Some(Command::Capabilities(_)) => {
            unreachable!("capabilities is dispatched before scan splitting")
        }
        Some(Command::Cache(_)) => unreachable!("cache is dispatched before scan splitting"),
        Some(Command::Config(_)) => unreachable!("config is dispatched before scan splitting"),
        Some(Command::Daemon(_)) => unreachable!("daemon is dispatched before scan splitting"),
        Some(Command::Update) => unreachable!("update is dispatched before scan splitting"),
    }
}

fn apply_overrides(cfg: &mut Config, args: &ScanArgs) {
    apply_common_overrides(cfg, &args.common);
    if let Some(t) = args.top {
        cfg.top = t;
    }
    if args.duplication_report_snippets {
        cfg.duplication_report_snippets = true;
    }
    cfg.by_dir = args.by_dir;
    cfg.diff_scope = if let Some(r) = &args.since {
        Some(reposcout::git::DiffScope::Since(r.clone()))
    } else if args.staged {
        Some(reposcout::git::DiffScope::Staged)
    } else if args.working {
        Some(reposcout::git::DiffScope::Working)
    } else {
        None
    };
    cfg.baseline_path = args.baseline.clone();
    cfg.fail_on_regression = args.fail_on_regression;
    cfg.graph = args.graph
        || !args.graph_focus.is_empty()
        || args.graph_depth.is_some()
        || args.graph_direction.is_some()
        || matches!(
            args.common.format,
            Some(OutputFormat::Dot | OutputFormat::Mermaid)
        )
        || args
            .common
            .output
            .as_deref()
            .and_then(Path::extension)
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "dot" | "gv" | "mmd" | "mermaid"));
    cfg.graph_focus = args.graph_focus.clone();
    if let Some(depth) = args.graph_depth {
        cfg.graph_depth = depth;
    }
    if let Some(direction) = args.graph_direction {
        cfg.graph_direction = direction;
    }
    if args.context
        || args.change_summary
        || args.context_budget.is_some()
        || args.context_max_files.is_some()
        || !args.focus.is_empty()
    {
        cfg.context = true;
    }
    if args.no_context {
        cfg.context = false;
    }
    if let Some(budget) = args.context_budget {
        cfg.context_budget = budget;
    }
    if let Some(max_files) = args.context_max_files {
        cfg.context_max_files = max_files;
    }
    cfg.context_focus = args.focus.clone();
    cfg.impact = args.impact || args.change_summary;
    cfg.change_summary = args.change_summary;
    cfg.review = args.review;
}

fn apply_common_overrides(cfg: &mut Config, args: &CommonArgs) {
    if let Some(encoding) = &args.encoding {
        cfg.encoding = encoding.clone();
    }
    if let Some(jobs) = args.jobs {
        cfg.jobs = jobs.max(1);
    }
    if let Some(bytes) = args.max_file_bytes {
        cfg.max_file_bytes = bytes;
    }
    if let Some(bytes) = args.max_total_bytes {
        cfg.max_total_bytes = bytes;
    }
    if let Some(files) = args.max_files {
        cfg.max_files = files;
    }
    if let Some(bytes) = args.max_git_blob_bytes {
        cfg.max_git_blob_bytes = bytes;
    }
    if let Some(seconds) = args.max_scan_seconds {
        cfg.max_scan_seconds = seconds;
    }
    if let Some(maximum) = args.max_complexity {
        cfg.max_complexity = maximum;
    }
    if args.hidden {
        cfg.include_hidden = true;
    }
    if args.no_ignore {
        cfg.respect_gitignore = false;
    }
    if args.include_lockfiles {
        cfg.exclude_lockfiles = false;
    }
    if args.no_cache {
        cfg.use_cache = false;
    }
    if let Some(mode) = args.duplication_mode {
        cfg.duplication_mode = mode;
    }
    if let Some(scope) = args.duplication_format_scope {
        cfg.duplication_format_scope = scope;
    }
    if let Some(scope) = args.health_scope {
        cfg.health_scope = scope;
    }
    extend_health_includes(&mut cfg.health_includes, &args.health_includes);
    extend_excludes(&mut cfg.health_excludes, &args.health_excludes);
    if args.quiet {
        cfg.quiet_progress = true;
    }
    if !args.exclude.is_empty() {
        extend_excludes(&mut cfg.extra_excludes, &args.exclude);
    }
}

fn extend_excludes(existing: &mut Vec<String>, additional: &[String]) {
    for pattern in additional {
        if !existing.contains(pattern) {
            existing.push(pattern.clone());
        }
    }
}

fn extend_health_includes(existing: &mut Vec<HealthInclude>, additional: &[HealthInclude]) {
    for format in additional {
        if !existing.contains(format) {
            existing.push(*format);
        }
    }
}

fn apply_execution_profile(cfg: &mut Config, profile: ExecutionProfile) {
    cfg.execution_profile = profile.as_str().to_string();
    if matches!(profile, ExecutionProfile::Agent | ExecutionProfile::Safe) {
        cfg.enabled.duplication = false;
        cfg.enabled.churn = false;
    }
}

fn enforce_absolute_limits(cfg: &mut Config) {
    cfg.enforce_absolute_limits();
}

fn enforce_safe_limits(cfg: &mut Config) {
    const SAFE_MAX_JOBS: usize = 2;
    const SAFE_MAX_CHURN_COMMITS: usize = 1_000;
    const SAFE_MAX_CHURN_DELTAS_PER_COMMIT: usize = 10_000;
    const SAFE_MAX_CHURN_TOTAL_DELTAS: usize = 50_000;
    const SAFE_MAX_CHURN_OUTPUT_BYTES: u64 = 8 * 1024 * 1024;
    const SAFE_MAX_GIT_PATH_BYTES: usize = 4_096;
    const SAFE_MAX_CHURN_CACHE_BYTES: u64 = 8 * 1024 * 1024;
    const SAFE_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
    const SAFE_MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
    const SAFE_MAX_FILES: usize = 20_000;
    const SAFE_MAX_GIT_BLOB_BYTES: u64 = 4 * 1024 * 1024;
    const SAFE_MAX_SCAN_SECONDS: u64 = 120;
    const SAFE_MAX_CONTEXT_TOKENS: usize = 32_000;
    const SAFE_MAX_CONTEXT_FILES: usize = 25;
    const SAFE_MAX_TOP: usize = 25;
    const SAFE_MAX_IGNORE_FILE_BYTES: u64 = 256 * 1024;
    const SAFE_MAX_IGNORE_LINES: usize = 10_000;
    const SAFE_MAX_IGNORE_LINE_BYTES: usize = 4_096;

    cfg.jobs = cfg.jobs.clamp(1, SAFE_MAX_JOBS);
    cfg.include_hidden = false;
    // Safe scans do not load repository-owned ignore files. Resource caps
    // already bound discovery, and untrusted ignore content is outside those caps.
    cfg.respect_gitignore = false;
    cfg.load_repository_ignores = false;
    cfg.exclude_lockfiles = true;
    cfg.top = cfg.top.min(SAFE_MAX_TOP);
    cfg.min_dup_tokens = cfg.min_dup_tokens.max(50);
    cfg.min_dup_lines = cfg.min_dup_lines.max(3);
    cfg.near_dup_min_similarity = cfg.near_dup_min_similarity.max(0.85);
    cfg.health_scope = HealthScope::Source;
    cfg.health_includes.clear();
    cfg.duplication_format_scope = reposcout::dup::DuplicationFormatScope::Exact;
    cfg.duplication_report_snippets = false;
    if cfg.churn_max_commits == 0 || cfg.churn_max_commits > SAFE_MAX_CHURN_COMMITS {
        cfg.churn_max_commits = SAFE_MAX_CHURN_COMMITS;
    }
    cfg.max_churn_deltas_per_commit = cfg
        .max_churn_deltas_per_commit
        .min(SAFE_MAX_CHURN_DELTAS_PER_COMMIT);
    cfg.max_churn_total_deltas = cfg.max_churn_total_deltas.min(SAFE_MAX_CHURN_TOTAL_DELTAS);
    cfg.max_churn_output_bytes = cfg.max_churn_output_bytes.min(SAFE_MAX_CHURN_OUTPUT_BYTES);
    cfg.max_git_path_bytes = cfg.max_git_path_bytes.min(SAFE_MAX_GIT_PATH_BYTES);
    cfg.max_churn_cache_bytes = cfg.max_churn_cache_bytes.min(SAFE_MAX_CHURN_CACHE_BYTES);
    cfg.max_file_bytes = cfg.max_file_bytes.min(SAFE_MAX_FILE_BYTES);
    cfg.max_total_bytes = cfg.max_total_bytes.min(SAFE_MAX_TOTAL_BYTES);
    cfg.max_files = cfg.max_files.min(SAFE_MAX_FILES);
    cfg.max_git_blob_bytes = cfg.max_git_blob_bytes.min(SAFE_MAX_GIT_BLOB_BYTES);
    cfg.max_scan_seconds = cfg.max_scan_seconds.min(SAFE_MAX_SCAN_SECONDS);
    cfg.max_ignore_file_bytes = cfg.max_ignore_file_bytes.min(SAFE_MAX_IGNORE_FILE_BYTES);
    cfg.max_ignore_lines = cfg.max_ignore_lines.min(SAFE_MAX_IGNORE_LINES);
    cfg.max_ignore_line_bytes = cfg.max_ignore_line_bytes.min(SAFE_MAX_IGNORE_LINE_BYTES);
    cfg.context_budget = cfg.context_budget.min(SAFE_MAX_CONTEXT_TOKENS);
    cfg.context_max_files = cfg.context_max_files.min(SAFE_MAX_CONTEXT_FILES);
    cfg.safety_limits = vec![
        format!("jobs<={SAFE_MAX_JOBS}"),
        format!("top<={SAFE_MAX_TOP}"),
        "project-config=ignored".to_string(),
        "repository-ignores=disabled".to_string(),
        "hidden-files=excluded".to_string(),
        "lockfiles=excluded".to_string(),
        "dup-min-tokens>=50".to_string(),
        "dup-min-lines>=3".to_string(),
        "dup-min-similarity>=0.85".to_string(),
        "health-scope=source".to_string(),
        "health-includes=none".to_string(),
        "dup-format-scope=exact".to_string(),
        "dup-snippets=disabled".to_string(),
        format!("churn-commits<={SAFE_MAX_CHURN_COMMITS}"),
        format!("churn-deltas-per-commit<={SAFE_MAX_CHURN_DELTAS_PER_COMMIT}"),
        format!("churn-total-deltas<={SAFE_MAX_CHURN_TOTAL_DELTAS}"),
        format!("churn-output-bytes<={SAFE_MAX_CHURN_OUTPUT_BYTES}"),
        format!("file-bytes<={SAFE_MAX_FILE_BYTES}"),
        format!("total-bytes<={SAFE_MAX_TOTAL_BYTES}"),
        format!("files<={SAFE_MAX_FILES}"),
        format!("git-blob-bytes<={SAFE_MAX_GIT_BLOB_BYTES}"),
        format!("scan-seconds<={SAFE_MAX_SCAN_SECONDS}"),
        format!("context-tokens<={SAFE_MAX_CONTEXT_TOKENS}"),
        format!("context-files<={SAFE_MAX_CONTEXT_FILES}"),
    ];
}

fn parse_only(only: &[String]) -> Result<Enabled> {
    let mut en = Enabled::none();
    for name in only {
        match name.trim().to_ascii_lowercase().as_str() {
            "tokens" => en.tokens = true,
            "lines" => en.lines = true,
            "complexity" => en.complexity = true,
            "imports" => en.imports = true,
            "markers" => en.markers = true,
            "dup" | "duplication" => en.duplication = true,
            "churn" | "git" => en.churn = true,
            other => return Err(anyhow!("unknown analyzer '{other}' in --only")),
        }
    }
    Ok(en)
}

fn choose_format(explicit: Option<OutputFormat>, output: Option<&Path>) -> Format {
    if let Some(f) = explicit {
        return match f {
            OutputFormat::Table => Format::Table,
            OutputFormat::Json => Format::Json,
            OutputFormat::Markdown => Format::Markdown,
            OutputFormat::Sarif => Format::Sarif,
            OutputFormat::Ndjson => Format::Ndjson,
            OutputFormat::Dot => Format::Dot,
            OutputFormat::Mermaid => Format::Mermaid,
        };
    }
    if let Some(path) = output {
        return match path.extension().and_then(|e| e.to_str()) {
            Some("md" | "markdown") => Format::Markdown,
            Some("txt") => Format::Table,
            Some("sarif") => Format::Sarif,
            Some("ndjson" | "jsonl") => Format::Ndjson,
            Some("dot" | "gv") => Format::Dot,
            Some("mmd" | "mermaid") => Format::Mermaid,
            _ => Format::Json,
        };
    }
    if std::io::stdout().is_terminal() {
        Format::Table
    } else {
        Format::Json
    }
}

#[derive(Debug, Clone, Copy)]
enum Comparison {
    GreaterOrEqual,
    LessOrEqual,
    Equal,
    Greater,
    Less,
}

#[derive(Debug)]
struct FailCondition {
    key: String,
    comparison: Comparison,
    threshold: f64,
}

fn parse_fail_on(expr: &str, enabled: Enabled) -> Result<Vec<FailCondition>> {
    expr.split(',')
        .map(str::trim)
        .filter(|condition| !condition.is_empty())
        .map(|condition| parse_fail_condition(condition, enabled))
        .collect()
}

fn parse_fail_condition(condition: &str, enabled: Enabled) -> Result<FailCondition> {
    let operators = [
        (">=", Comparison::GreaterOrEqual),
        ("<=", Comparison::LessOrEqual),
        ("==", Comparison::Equal),
        (">", Comparison::Greater),
        ("<", Comparison::Less),
    ];
    let (key, comparison, rhs) = operators
        .iter()
        .find_map(|(operator, comparison)| {
            condition
                .split_once(operator)
                .map(|(key, rhs)| (key.trim(), *comparison, rhs.trim()))
        })
        .ok_or_else(|| {
            anyhow!("invalid --fail-on condition '{condition}' (expected key OP number)")
        })?;

    let threshold: f64 = rhs
        .parse()
        .map_err(|_| anyhow!("invalid number in --fail-on '{condition}'"))?;
    if !threshold.is_finite() {
        return Err(anyhow!("invalid number in --fail-on '{condition}'"));
    }
    validate_metric_availability(key, enabled)?;

    Ok(FailCondition {
        key: key.to_string(),
        comparison,
        threshold,
    })
}

fn validate_metric_availability(key: &str, enabled: Enabled) -> Result<()> {
    let requirement = match key {
        "max-cyclomatic"
        | "avg-cyclomatic"
        | "max-cognitive"
        | "avg-cognitive"
        | "min-mi"
        | "min-maintainability"
        | "avg-mi"
        | "avg-maintainability" => Some((enabled.complexity, "complexity")),
        "duplicated-pct" => Some((enabled.duplication, "duplication")),
        "tokens" => Some((enabled.tokens, "tokens")),
        "files" | "sloc" => None,
        _ => return Err(anyhow!("unknown --fail-on key '{key}'")),
    };
    if let Some((available, analyzer)) = requirement
        && !available
    {
        return Err(anyhow!(
            "--fail-on metric {key} requires the {analyzer} analyzer"
        ));
    }
    Ok(())
}

fn evaluate_fail_on(conditions: &[FailCondition], summary: &Summary) -> bool {
    conditions.iter().any(|condition| {
        let lhs = metric_value(&condition.key, summary)
            .expect("validated --fail-on metric must have a summary value");
        match condition.comparison {
            Comparison::Greater => lhs > condition.threshold,
            Comparison::Less => lhs < condition.threshold,
            Comparison::GreaterOrEqual => lhs >= condition.threshold,
            Comparison::LessOrEqual => lhs <= condition.threshold,
            Comparison::Equal => (lhs - condition.threshold).abs() < f64::EPSILON,
        }
    })
}

fn metric_value(key: &str, s: &Summary) -> Option<f64> {
    Some(match key {
        "max-cyclomatic" => s.complexity.cyclomatic_max as f64,
        "avg-cyclomatic" => s.complexity.cyclomatic_avg,
        "max-cognitive" => s.complexity.cognitive_max as f64,
        "avg-cognitive" => s.complexity.cognitive_avg,
        "min-mi" | "min-maintainability" => s.complexity.mi_min,
        "avg-mi" | "avg-maintainability" => s.complexity.mi_avg,
        "duplicated-pct" => s.duplication.duplicated_pct,
        "tokens" => s.tokens as f64,
        "files" => s.files as f64,
        "sloc" => s.sloc as f64,
        _ => return None,
    })
}
