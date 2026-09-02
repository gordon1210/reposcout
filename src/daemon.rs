//! Live scan service used by the web UI and other local clients.

use crate::config::Config;
use crate::debug_log;
use crate::model::{DepGraph, ScanReport};
use crate::scan;
use anyhow::{Context, Result, anyhow};
use axum::extract::{Query, Request, State};
use axum::http::uri::Authority;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive};
use axum::response::{IntoResponse, Response, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use notify::event::ModifyKind;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, OwnedSemaphorePermit, RwLock, Semaphore, broadcast, mpsc, watch};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

#[derive(Debug, Clone)]
pub struct DaemonOptions {
    pub host: IpAddr,
    pub port: u16,
    pub debounce: Duration,
    pub profile: &'static str,
    pub unsafe_no_auth: bool,
    /// Permit non-loopback binding over plain HTTP (requires reverse-proxy TLS).
    pub allow_insecure_remote: bool,
}

const RESCAN_COOLDOWN: Duration = Duration::from_secs(1);
const MAX_SSE_CLIENTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonStatus {
    Starting,
    Scanning,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonSnapshot {
    pub target: PathBuf,
    pub profile: String,
    pub revision: u64,
    pub status: DaemonStatus,
    pub scan_started_at: Option<String>,
    pub scan_finished_at: Option<String>,
    pub error: Option<String>,
    pub report: Option<ScanReport>,
    #[serde(skip)]
    pub graph_facts: std::collections::BTreeMap<PathBuf, crate::graph::SourceFacts>,
    #[serde(skip)]
    pub resolver_configs: std::collections::BTreeMap<String, String>,
    #[serde(skip)]
    pub graph_limits: crate::graph::GraphReadLimits,
}

#[derive(Debug, Clone, Copy, Serialize)]
enum DaemonEventKind {
    #[serde(rename = "scan_started")]
    Started,
    #[serde(rename = "scan_completed")]
    Completed,
    #[serde(rename = "scan_failed")]
    Failed,
}

impl DaemonEventKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Started => "scan_started",
            Self::Completed => "scan_completed",
            Self::Failed => "scan_failed",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DaemonEvent {
    kind: DaemonEventKind,
    revision: u64,
    at: String,
    error: Option<String>,
}

#[derive(Clone)]
struct AppState {
    snapshot: Arc<RwLock<DaemonSnapshot>>,
    graph_cache: Arc<Mutex<Option<CachedGraph>>>,
    events: broadcast::Sender<DaemonEvent>,
    trigger: mpsc::Sender<()>,
    shutdown: watch::Receiver<bool>,
    last_rescan: Arc<Mutex<Option<Instant>>>,
    sse_slots: Arc<Semaphore>,
}

#[derive(Debug, Clone)]
struct RequestPolicy {
    loopback: bool,
    auth_token: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedGraph {
    revision: u64,
    graph: DepGraph,
}

#[derive(Debug, Default, Deserialize)]
struct GraphRequest {
    revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct DaemonGraphResponse {
    revision: u64,
    graph: DepGraph,
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

/// Run the local dashboard daemon until shutdown.
///
/// # Errors
///
/// Returns an error when the async runtime, listener, authentication token,
/// initial scan, file watcher, or HTTP server cannot be initialized or run.
pub fn run(target: PathBuf, cfg: Config, options: DaemonOptions) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start daemon runtime")?;
    let result = runtime.block_on(serve(target, cfg, options));
    runtime.shutdown_timeout(Duration::from_secs(2));
    result
}

#[expect(
    clippy::too_many_lines,
    reason = "daemon startup is one linear ownership transfer across listener, token, watcher, scan task, and graceful shutdown resources"
)]
async fn serve(target: PathBuf, cfg: Config, options: DaemonOptions) -> Result<()> {
    // Non-loopback remains explicit: plain HTTP is not safe on shared networks.
    if !options.host.is_loopback() && !options.allow_insecure_remote {
        return Err(anyhow!(
            "refusing non-loopback daemon binding over plain HTTP; keep the loopback default or pass --allow-insecure-remote explicitly"
        ));
    }
    if !options.host.is_loopback() && options.unsafe_no_auth {
        return Err(anyhow!(
            "refusing unauthenticated non-loopback daemon binding; omit --unsafe-no-auth or bind to loopback"
        ));
    }

    let target = target
        .canonicalize()
        .with_context(|| format!("failed to resolve daemon target {}", target.display()))?;
    let (trigger_tx, trigger_rx) = mpsc::channel(1);
    let (event_tx, _) = broadcast::channel(32);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let snapshot = Arc::new(RwLock::new(DaemonSnapshot {
        target: target.clone(),
        profile: options.profile.to_string(),
        revision: 0,
        status: DaemonStatus::Starting,
        scan_started_at: None,
        scan_finished_at: None,
        error: None,
        report: None,
        graph_facts: std::collections::BTreeMap::new(),
        resolver_configs: std::collections::BTreeMap::new(),
        graph_limits: crate::graph::GraphReadLimits::from_config(&cfg),
    }));
    let state = AppState {
        snapshot: Arc::clone(&snapshot),
        graph_cache: Arc::new(Mutex::new(None)),
        events: event_tx.clone(),
        trigger: trigger_tx.clone(),
        shutdown: shutdown_tx.subscribe(),
        last_rescan: Arc::new(Mutex::new(None)),
        sse_slots: Arc::new(Semaphore::new(MAX_SSE_CLIENTS)),
    };

    let exclusions = debug_log::path()
        .into_iter()
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    let mut watcher = make_watcher(&target, trigger_tx.clone(), &exclusions)?;
    let watch_target = if target.is_file() {
        target
            .parent()
            .ok_or_else(|| anyhow!("daemon target has no parent"))?
    } else {
        &target
    };
    watcher
        .watch(watch_target, RecursiveMode::Recursive)
        .with_context(|| format!("failed to watch {}", watch_target.display()))?;

    let scan_task = tokio::spawn(scan_loop(
        target.clone(),
        cfg,
        options.debounce,
        snapshot,
        event_tx,
        trigger_rx,
        shutdown_rx,
        exclusions,
    ));
    trigger_tx
        .try_send(())
        .map_err(|_| anyhow!("failed to queue initial scan"))?;

    // Bind first so a failing port claim cannot overwrite another instance's token.
    let address = SocketAddr::new(options.host, options.port);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("failed to bind daemon to {address}"))?;

    let auth_token = if options.unsafe_no_auth {
        None
    } else {
        Some(issue_daemon_token(options.port)?)
    };

    println!("reposcout daemon listening on http://{address}");
    if let Some(_token) = &auth_token {
        let path = daemon_token_path(options.port)?;
        println!("daemon auth token file: {}", path.display());
        println!(
            "daemon auth: Authorization: Bearer <token from file> (SSE may use ?token= over loopback only)"
        );
    } else {
        println!("daemon auth: disabled via --unsafe-no-auth");
    }
    if options.allow_insecure_remote && !options.host.is_loopback() {
        println!(
            "warning: non-loopback plain HTTP is enabled; place a TLS reverse proxy in front of this listener"
        );
    }

    let server = axum::serve(listener, router(state, options.host, auth_token.clone()))
        .with_graceful_shutdown(wait_for_shutdown(shutdown_tx.subscribe()));

    let shutdown_on_signal = shutdown_tx.clone();
    let signal_task = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = shutdown_on_signal.send(true);
        }
    });
    let server_result = server.await.context("daemon server failed");

    let _ = shutdown_tx.send(true);
    signal_task.abort();
    drop(watcher);
    scan_task.abort();
    let _ = scan_task.await;
    server_result
}

fn router(state: AppState, host: IpAddr, auth_token: Option<String>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/snapshot", get(snapshot))
        .route("/api/graph", get(repository_graph))
        .route("/api/events", get(events))
        .route("/api/rescan", post(rescan))
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            RequestPolicy {
                loopback: host.is_loopback(),
                auth_token,
            },
            enforce_request_policy,
        ))
}

async fn enforce_request_policy(
    State(policy): State<RequestPolicy>,
    request: Request,
    next: Next,
) -> Response {
    if !allowed_request_authority(&request, policy.loopback) {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    }
    if request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| !allowed_origin(origin, policy.loopback))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(expected) = policy.auth_token.as_deref()
        && !request_has_valid_token(&request, expected, policy.loopback)
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ApiError {
                error: "missing or invalid daemon authentication token".to_string(),
            }),
        )
            .into_response();
    }
    next.run(request).await
}

fn request_has_valid_token(request: &Request, expected: &str, loopback: bool) -> bool {
    if let Some(value) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        let bearer = value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "));
        if bearer == Some(expected) {
            return true;
        }
    }
    if request
        .headers()
        .get("x-reposcout-token")
        .and_then(|value| value.to_str().ok())
        == Some(expected)
    {
        return true;
    }
    // EventSource cannot set Authorization headers. Accept `?token=` only for the
    // SSE endpoint on loopback so the secret never authenticates other APIs or
    // remote listeners (proxy logs, history, Referer).
    if loopback && request.uri().path() == "/api/events" {
        return request.uri().query().and_then(|query| {
            query.split('&').find_map(|pair| {
                let (key, value) = pair.split_once('=')?;
                (key == "token").then_some(value)
            })
        }) == Some(expected);
    }
    false
}

/// Public path used by local tooling (e.g. the Vite proxy) to load the token.
///
/// # Errors
///
/// Returns an error when the platform exposes no suitable runtime or cache
/// directory for the daemon token.
pub fn daemon_token_path(port: u16) -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "reposcout").ok_or_else(|| {
        anyhow!("the platform does not expose a runtime directory for the daemon token file")
    })?;
    let base = dirs.runtime_dir().unwrap_or_else(|| dirs.cache_dir());
    Ok(base.join(format!("daemon-{port}.token")))
}

fn issue_daemon_token(port: u16) -> Result<String> {
    let token = generate_token()?;
    let path = daemon_token_path(port)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create daemon token directory {}",
                parent.display()
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("failed to set permissions on {}", parent.display()))?;
        }
    }
    write_token_file_atomic(&path, &token)?;
    Ok(token)
}

fn write_token_file_atomic(path: &Path, token: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("daemon token path has no parent"))?;
    let temporary = parent.join(format!(
        ".daemon-token.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));

    // Create the temporary file without following a final symlink component.
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .with_context(|| format!("failed to create {}", temporary.display()))?;
        writeln!(file, "{token}")?;
        file.sync_all()?;
        // Replace destination only if it is not a symlink.
        if let Ok(metadata) = std::fs::symlink_metadata(path)
            && metadata.file_type().is_symlink()
        {
            let _ = std::fs::remove_file(&temporary);
            return Err(anyhow!(
                "refusing to overwrite a symlink daemon token at {}",
                path.display()
            ));
        }
        if path.exists() {
            std::fs::remove_file(path)
                .with_context(|| format!("failed to replace {}", path.display()))?;
        }
        std::fs::rename(&temporary, path)
            .with_context(|| format!("failed to install daemon token at {}", path.display()))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        // Windows rename does not replace an existing destination. After a
        // successful bind no other instance can own this port, so remove-then-
        // rename is an acceptable replacement for a missing atomic ReplaceFile.
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(anyhow!(
                    "refusing to replace symbolic link {}",
                    path.display()
                ));
            }
            Ok(metadata) if metadata.is_file() => {
                std::fs::remove_file(path).with_context(|| {
                    format!("failed to remove stale token file {}", path.display())
                })?;
            }
            Ok(_) => {
                return Err(anyhow!(
                    "token path is not a regular file: {}",
                    path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        std::fs::write(&temporary, format!("{token}\n"))
            .with_context(|| format!("failed to create {}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .with_context(|| format!("failed to install daemon token at {}", path.display()))?;
    }
    Ok(())
}

fn generate_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow!("failed to draw daemon token entropy: {error}"))?;
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(token, "{byte:02x}");
    }
    Ok(token)
}

fn allowed_request_authority(request: &Request, loopback: bool) -> bool {
    match request.headers().get(header::HOST) {
        Some(value) => value
            .to_str()
            .is_ok_and(|authority| allowed_request_host(authority, loopback)),
        None => request
            .uri()
            .authority()
            .map(Authority::as_str)
            .is_some_and(|authority| allowed_request_host(authority, loopback)),
    }
}

fn allowed_request_host(value: &str, loopback: bool) -> bool {
    let Ok(authority) = value.parse::<Authority>() else {
        return false;
    };
    allowed_network_host(authority.host(), loopback)
}

fn allowed_origin(value: &str, loopback: bool) -> bool {
    let Ok(uri) = value.parse::<axum::http::Uri>() else {
        return false;
    };
    uri.host()
        .is_some_and(|host| allowed_network_host(host, loopback))
}

fn allowed_network_host(value: &str, loopback: bool) -> bool {
    if value.eq_ignore_ascii_case("localhost") {
        return loopback;
    }
    let host = value.trim_matches(['[', ']']);
    host.parse::<IpAddr>()
        .is_ok_and(|address| !loopback || address.is_loopback())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn snapshot(State(state): State<AppState>) -> Json<DaemonSnapshot> {
    Json(state.snapshot.read().await.clone())
}

async fn repository_graph(
    Query(request): Query<GraphRequest>,
    State(state): State<AppState>,
) -> std::result::Result<Json<DaemonGraphResponse>, (StatusCode, Json<ApiError>)> {
    let (revision, root, files, inputs, limits) = {
        let snapshot = state.snapshot.read().await;
        if let Some(expected) = request.revision
            && expected != snapshot.revision
        {
            return Err(api_error(
                StatusCode::CONFLICT,
                format!(
                    "report revision changed from {expected} to {}",
                    snapshot.revision
                ),
            ));
        }
        let report = snapshot.report.as_ref().ok_or_else(|| {
            api_error(
                StatusCode::CONFLICT,
                "no completed report is available".to_string(),
            )
        })?;
        (
            snapshot.revision,
            report.root.clone(),
            report.files.clone(),
            crate::graph::GraphInputs {
                source_facts: snapshot.graph_facts.clone(),
                resolver_configs: snapshot.resolver_configs.clone(),
            },
            snapshot.graph_limits,
        )
    };

    let mut cache = state.graph_cache.lock().await;
    if let Some(cached) = cache.as_ref()
        && cached.revision == revision
    {
        return Ok(Json(DaemonGraphResponse {
            revision,
            graph: cached.graph.clone(),
        }));
    }

    let graph = tokio::task::spawn_blocking(move || {
        // Build only from revision-scoped source facts and resolver configs.
        crate::graph::build_with_inputs(&files, &root, limits, &inputs)
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("graph analysis task failed: {error}"),
        )
    })?;
    *cache = Some(CachedGraph {
        revision,
        graph: graph.clone(),
    });

    Ok(Json(DaemonGraphResponse { revision, graph }))
}

fn api_error(status: StatusCode, error: String) -> (StatusCode, Json<ApiError>) {
    (status, Json(ApiError { error }))
}

async fn rescan(headers: HeaderMap, State(state): State<AppState>) -> impl IntoResponse {
    if headers
        .get("x-reposcout-request")
        .and_then(|value| value.to_str().ok())
        != Some("rescan")
    {
        return StatusCode::BAD_REQUEST;
    }
    let mut last_rescan = state.last_rescan.lock().await;
    let now = Instant::now();
    if last_rescan.is_some_and(|previous| now.duration_since(previous) < RESCAN_COOLDOWN) {
        return StatusCode::TOO_MANY_REQUESTS;
    }
    *last_rescan = Some(now);
    drop(last_rescan);
    match state.trigger.try_send(()) {
        Ok(()) | Err(mpsc::error::TrySendError::Full(())) => StatusCode::ACCEPTED,
        Err(mpsc::error::TrySendError::Closed(())) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn events(
    State(state): State<AppState>,
) -> std::result::Result<
    Sse<impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>>>,
    StatusCode,
> {
    let permit = state
        .sse_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    Ok(Sse::new(event_stream(
        state.events.subscribe(),
        state.shutdown,
        permit,
    ))
    .keep_alive(KeepAlive::default()))
}

fn event_stream(
    events: broadcast::Receiver<DaemonEvent>,
    shutdown: watch::Receiver<bool>,
    permit: OwnedSemaphorePermit,
) -> impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>> {
    // A lagged receiver has missed an unknown state transition. End that SSE
    // connection so EventSource reconnects and the client reconciles from the
    // canonical snapshot instead of remaining silently stale.
    let stream = BroadcastStream::new(events)
        .take_while(Result::is_ok)
        .filter_map(|message| {
            message.ok().and_then(|event| {
                serde_json::to_string(&event).ok().map(|data| {
                    Ok(Event::default()
                        .event(event.kind.as_str())
                        .id(event.revision.to_string())
                        .data(data))
                })
            })
        });
    let guarded = stream.map(move |event| {
        let _permit = &permit;
        event
    });
    futures_util::StreamExt::take_until(guarded, wait_for_shutdown(shutdown))
}

async fn wait_for_shutdown(mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() || shutdown.changed().await.is_err() {
            return;
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the scan task owns distinct Tokio channels and shared state with different borrow and move semantics"
)]
async fn scan_loop(
    target: PathBuf,
    cfg: Config,
    debounce: Duration,
    snapshot: Arc<RwLock<DaemonSnapshot>>,
    events: broadcast::Sender<DaemonEvent>,
    mut trigger: mpsc::Receiver<()>,
    mut shutdown: watch::Receiver<bool>,
    exclusions: Vec<PathBuf>,
) {
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            queued = trigger.recv() => {
                if queued.is_none() {
                    break;
                }
                if !debounce.is_zero() {
                    tokio::time::sleep(debounce).await;
                }
                while trigger.try_recv().is_ok() {}
                scan_once(&target, &cfg, &snapshot, &events, &exclusions).await;
            }
        }
    }
}

async fn scan_once(
    target: &Path,
    cfg: &Config,
    snapshot: &Arc<RwLock<DaemonSnapshot>>,
    events: &broadcast::Sender<DaemonEvent>,
    exclusions: &[PathBuf],
) {
    let started_at = chrono::Utc::now().to_rfc3339();
    let revision = {
        let mut current = snapshot.write().await;
        current.status = DaemonStatus::Scanning;
        current.scan_started_at = Some(started_at.clone());
        current.error = None;
        current.revision
    };
    emit(events, DaemonEventKind::Started, revision, started_at, None);

    let scan_target = target.to_path_buf();
    let scan_cfg = cfg.clone();
    let scan_exclusions = exclusions.to_vec();
    let graph_limits = crate::graph::GraphReadLimits::from_config(cfg);
    let result = tokio::task::spawn_blocking(move || {
        // Always extract graph source facts so /api/graph can rebuild topology
        // from the completed revision without re-reading live source files.
        scan::run_with_artifacts(
            &scan_target,
            &scan_cfg,
            &scan_exclusions,
            scan::ArtifactRequirements {
                symbol_outlines: false,
                graph_facts: true,
            },
        )
    })
    .await;
    let finished_at = chrono::Utc::now().to_rfc3339();

    match result {
        Ok(Ok(artifacts)) => {
            let revision = {
                let mut current = snapshot.write().await;
                current.revision += 1;
                current.status = DaemonStatus::Ready;
                current.scan_finished_at = Some(finished_at.clone());
                current.error = None;
                current.report = Some(artifacts.report);
                current.graph_facts = artifacts.graph_facts;
                current.resolver_configs = artifacts.resolver_configs;
                current.graph_limits = graph_limits;
                // Graph cache entries are keyed by revision, so older graphs are ignored.
                current.revision
            };
            emit(
                events,
                DaemonEventKind::Completed,
                revision,
                finished_at,
                None,
            );
        }
        Ok(Err(error)) => {
            record_scan_error(snapshot, events, error.to_string(), finished_at).await;
        }
        Err(error) => {
            record_scan_error(snapshot, events, error.to_string(), finished_at).await;
        }
    }
}

async fn record_scan_error(
    snapshot: &Arc<RwLock<DaemonSnapshot>>,
    events: &broadcast::Sender<DaemonEvent>,
    error: String,
    finished_at: String,
) {
    let revision = {
        let mut current = snapshot.write().await;
        current.status = DaemonStatus::Error;
        current.scan_finished_at = Some(finished_at.clone());
        current.error = Some(error.clone());
        current.revision
    };
    emit(
        events,
        DaemonEventKind::Failed,
        revision,
        finished_at,
        Some(error),
    );
}

fn emit(
    events: &broadcast::Sender<DaemonEvent>,
    kind: DaemonEventKind,
    revision: u64,
    at: String,
    error: Option<String>,
) {
    let _ = events.send(DaemonEvent {
        kind,
        revision,
        at,
        error,
    });
}

fn make_watcher(
    target: &Path,
    trigger: mpsc::Sender<()>,
    exclusions: &[PathBuf],
) -> Result<RecommendedWatcher> {
    let target = target.to_path_buf();
    let exclusions = exclusions.to_vec();
    notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && event_requires_rescan(&event, &target, &exclusions)
        {
            let _ = trigger.try_send(());
        }
    })
    .context("failed to initialize filesystem watcher")
}

fn event_requires_rescan(event: &notify::Event, target: &Path, exclusions: &[PathBuf]) -> bool {
    if !matches!(
        event.kind,
        EventKind::Any
            | EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Name(_))
    ) {
        return false;
    }

    event.paths.iter().any(|path| {
        let targets_file = !target.is_file() || path == target;
        targets_file
            && !exclusions.iter().any(|excluded| path == excluded)
            && !path.components().any(|component| {
                matches!(
                    component.as_os_str(),
                    name if name == OsStr::new(".git")
                        || name == OsStr::new("target")
                        || name == OsStr::new("node_modules")
                        || name == OsStr::new("dist")
                        || name == OsStr::new(".reposcout")
                )
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, DataChange, ModifyKind};

    #[test]
    fn watcher_ignores_access_and_generated_directories() {
        let target = Path::new("/repo");
        let access = notify::Event::new(EventKind::Access(AccessKind::Any))
            .add_path(PathBuf::from("/repo/src/lib.rs"));
        assert!(!event_requires_rescan(&access, target, &[]));

        for path in [
            "/repo/.git/index",
            "/repo/target/release/reposcout",
            "/repo/apps/web/node_modules/react/index.js",
            "/repo/apps/web/dist/index.js",
            "/repo/.reposcout/cache.json",
        ] {
            let event =
                notify::Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
                    .add_path(PathBuf::from(path));
            assert!(!event_requires_rescan(&event, target, &[]), "{path}");
        }
    }

    #[test]
    fn watcher_accepts_source_modifications() {
        let event = notify::Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(PathBuf::from("/repo/src/lib.rs"));
        assert!(event_requires_rescan(&event, Path::new("/repo"), &[]));
    }

    #[test]
    fn watcher_ignores_exact_runtime_exclusions() {
        let debug_log = PathBuf::from("/repo/reposcout-debug.jsonl");
        let event = notify::Event::new(EventKind::Modify(ModifyKind::Data(DataChange::Content)))
            .add_path(debug_log.clone());
        assert!(!event_requires_rescan(
            &event,
            Path::new("/repo"),
            &[debug_log]
        ));
    }

    #[test]
    fn snapshot_serializes_status_and_report_state() {
        let snapshot = DaemonSnapshot {
            target: PathBuf::from("/repo"),
            profile: "lite".to_string(),
            revision: 2,
            status: DaemonStatus::Scanning,
            scan_started_at: Some("2026-01-01T00:00:00Z".to_string()),
            scan_finished_at: None,
            error: None,
            report: None,
            graph_facts: std::collections::BTreeMap::default(),
            resolver_configs: std::collections::BTreeMap::default(),
            graph_limits: crate::graph::GraphReadLimits::default(),
        };
        let json = serde_json::to_value(snapshot).unwrap();
        assert_eq!(json["status"], "scanning");
        assert_eq!(json["profile"], "lite");
        assert_eq!(json["revision"], 2);
        assert!(json["report"].is_null());
    }

    #[tokio::test]
    async fn snapshot_handler_returns_current_state() {
        let (events, _) = broadcast::channel(1);
        let (trigger, _) = mpsc::channel(1);
        let (_, shutdown) = watch::channel(false);
        let state = AppState {
            snapshot: Arc::new(RwLock::new(DaemonSnapshot {
                target: PathBuf::from("/repo"),
                profile: "lite".to_string(),
                revision: 4,
                status: DaemonStatus::Ready,
                scan_started_at: None,
                scan_finished_at: None,
                error: None,
                report: None,
                graph_facts: std::collections::BTreeMap::default(),
                resolver_configs: std::collections::BTreeMap::default(),
                graph_limits: crate::graph::GraphReadLimits::default(),
            })),
            graph_cache: Arc::new(Mutex::new(None)),
            events,
            trigger,
            shutdown,
            last_rescan: Arc::new(Mutex::new(None)),
            sse_slots: Arc::new(Semaphore::new(MAX_SSE_CLIENTS)),
        };

        let Json(snapshot) = snapshot(State(state)).await;
        assert_eq!(snapshot.revision, 4);
        assert_eq!(snapshot.status, DaemonStatus::Ready);
    }

    #[tokio::test]
    async fn rescan_handler_coalesces_when_a_scan_is_already_queued() {
        let (events, _) = broadcast::channel(1);
        let (trigger, mut queued) = mpsc::channel(1);
        let (_, shutdown) = watch::channel(false);
        trigger.try_send(()).unwrap();
        let state = AppState {
            snapshot: Arc::new(RwLock::new(DaemonSnapshot {
                target: PathBuf::from("/repo"),
                profile: "lite".to_string(),
                revision: 0,
                status: DaemonStatus::Starting,
                scan_started_at: None,
                scan_finished_at: None,
                error: None,
                report: None,
                graph_facts: std::collections::BTreeMap::default(),
                resolver_configs: std::collections::BTreeMap::default(),
                graph_limits: crate::graph::GraphReadLimits::default(),
            })),
            graph_cache: Arc::new(Mutex::new(None)),
            events,
            trigger,
            shutdown,
            last_rescan: Arc::new(Mutex::new(None)),
            sse_slots: Arc::new(Semaphore::new(MAX_SSE_CLIENTS)),
        };

        assert_eq!(
            rescan(HeaderMap::new(), State(state.clone()))
                .await
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        let mut headers = HeaderMap::new();
        headers.insert("x-reposcout-request", "rescan".parse().unwrap());
        assert_eq!(
            rescan(headers.clone(), State(state.clone()))
                .await
                .into_response()
                .status(),
            StatusCode::ACCEPTED
        );
        assert_eq!(
            rescan(headers, State(state)).await.into_response().status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert!(queued.try_recv().is_ok());
        assert!(queued.try_recv().is_err());
    }

    #[tokio::test]
    async fn graph_handler_builds_once_for_the_requested_report_revision() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample");
        let mut config = Config::default();
        config.enabled = crate::config::Enabled::none();
        config.use_cache = false;
        config.quiet_progress = true;
        let report = scan::run(&root, &config).unwrap();
        assert!(report.graph.is_none(), "regular scans stay graph-free");
        let expected_nodes = report
            .files
            .iter()
            .filter(|file| {
                crate::lang::detect(&file.path).is_some_and(crate::lang::LangInfo::is_first_class)
            })
            .count();

        let (events, _) = broadcast::channel(1);
        let (trigger, _) = mpsc::channel(1);
        let (_, shutdown) = watch::channel(false);
        let state = AppState {
            snapshot: Arc::new(RwLock::new(DaemonSnapshot {
                target: root,
                profile: "lite".to_string(),
                revision: 7,
                status: DaemonStatus::Ready,
                scan_started_at: None,
                scan_finished_at: None,
                error: None,
                report: Some(report),
                graph_facts: std::collections::BTreeMap::default(),
                resolver_configs: std::collections::BTreeMap::default(),
                graph_limits: crate::graph::GraphReadLimits::default(),
            })),
            graph_cache: Arc::new(Mutex::new(None)),
            events,
            trigger,
            shutdown,
            last_rescan: Arc::new(Mutex::new(None)),
            sse_slots: Arc::new(Semaphore::new(MAX_SSE_CLIENTS)),
        };

        let Json(first) = repository_graph(
            Query(GraphRequest { revision: Some(7) }),
            State(state.clone()),
        )
        .await
        .unwrap();
        assert_eq!(first.revision, 7);
        assert_eq!(first.graph.nodes, expected_nodes);
        {
            let mut cache = state.graph_cache.lock().await;
            let cached = cache.as_mut().unwrap();
            assert_eq!(cached.revision, 7);
            cached.graph.nodes = 42;
        }

        let Json(second) = repository_graph(
            Query(GraphRequest { revision: Some(7) }),
            State(state.clone()),
        )
        .await
        .unwrap();
        assert_eq!(second.graph.nodes, 42, "same revision reuses the cache");

        let (status, _) = repository_graph(Query(GraphRequest { revision: Some(6) }), State(state))
            .await
            .unwrap_err();
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn daemon_graph_uses_revision_inputs_instead_of_live_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{"@app/*":["src/*"]}}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/dependency.ts"),
            "export const dependency = 1;\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/importer.ts"),
            "import { dependency } from '@app/dependency';\nvoid dependency;\n",
        )
        .unwrap();
        let mut config = Config::default();
        config.enabled = crate::config::Enabled::none();
        config.use_cache = false;
        config.quiet_progress = true;
        let (events, _) = broadcast::channel(4);
        let snapshot = Arc::new(RwLock::new(DaemonSnapshot {
            target: dir.path().to_path_buf(),
            profile: "lite".to_string(),
            revision: 0,
            status: DaemonStatus::Starting,
            scan_started_at: None,
            scan_finished_at: None,
            error: None,
            report: None,
            graph_facts: std::collections::BTreeMap::default(),
            resolver_configs: std::collections::BTreeMap::default(),
            graph_limits: crate::graph::GraphReadLimits::default(),
        }));

        scan_once(dir.path(), &config, &snapshot, &events, &[]).await;
        {
            let completed = snapshot.read().await;
            assert_eq!(completed.revision, 1);
            assert_eq!(completed.status, DaemonStatus::Ready);
            assert!(completed.report.as_ref().unwrap().graph.is_none());
            assert_eq!(completed.graph_facts.len(), 2);
            assert!(completed.resolver_configs.contains_key("tsconfig.json"));
        }

        std::fs::write(
            dir.path().join("tsconfig.json"),
            r#"{"compilerOptions":{"baseUrl":".","paths":{}}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/importer.ts"),
            "export const importer = 1;\n",
        )
        .unwrap();

        let (trigger, _) = mpsc::channel(1);
        let (_, shutdown) = watch::channel(false);
        let state = AppState {
            snapshot,
            graph_cache: Arc::new(Mutex::new(None)),
            events,
            trigger,
            shutdown,
            last_rescan: Arc::new(Mutex::new(None)),
            sse_slots: Arc::new(Semaphore::new(MAX_SSE_CLIENTS)),
        };
        let Json(response) =
            repository_graph(Query(GraphRequest { revision: Some(1) }), State(state))
                .await
                .unwrap();

        assert!(response.graph.edge_list.iter().any(|edge| {
            edge.source == "src/importer.ts"
                && edge.target == "src/dependency.ts"
                && edge.resolver == "tsconfig-paths"
        }));
    }

    #[tokio::test]
    async fn event_stream_closes_when_shutdown_begins() {
        let (events, _) = broadcast::channel(1);
        let (shutdown_tx, shutdown) = watch::channel(false);
        let permit = Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap();
        let mut stream = std::pin::pin!(event_stream(events.subscribe(), shutdown, permit));

        shutdown_tx.send(true).unwrap();

        assert!(
            tokio::time::timeout(Duration::from_millis(100), stream.next())
                .await
                .expect("SSE stream should close promptly")
                .is_none()
        );
    }

    #[tokio::test]
    async fn event_stream_closes_when_the_receiver_lags() {
        let (events, receiver) = broadcast::channel(1);
        let (_, shutdown) = watch::channel(false);
        let permit = Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap();
        events
            .send(DaemonEvent {
                kind: DaemonEventKind::Started,
                revision: 1,
                at: "2026-01-01T00:00:00Z".to_string(),
                error: None,
            })
            .unwrap();
        events
            .send(DaemonEvent {
                kind: DaemonEventKind::Completed,
                revision: 2,
                at: "2026-01-01T00:00:01Z".to_string(),
                error: None,
            })
            .unwrap();
        let mut stream = std::pin::pin!(event_stream(receiver, shutdown, permit));

        assert!(
            tokio::time::timeout(Duration::from_millis(100), stream.next())
                .await
                .expect("lagged SSE stream should close promptly")
                .is_none()
        );
    }

    #[test]
    fn request_policy_rejects_dns_rebinding_hosts_and_remote_origins() {
        assert!(allowed_request_host("localhost:5173", true));
        assert!(allowed_request_host("127.0.0.1:7331", true));
        assert!(!allowed_request_host("attacker.example:7331", true));
        assert!(allowed_origin("http://localhost:5173", true));
        assert!(!allowed_origin("https://attacker.example", true));
        assert!(allowed_request_host("192.0.2.10:7331", false));
        assert!(!allowed_request_host("daemon.example:7331", false));
    }

    #[test]
    fn request_policy_requires_a_valid_host_or_uri_authority() {
        let missing = Request::builder()
            .uri("/api/health")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(!allowed_request_authority(&missing, true));

        let local_header = Request::builder()
            .uri("/api/health")
            .header(header::HOST, "127.0.0.1:7331")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(allowed_request_authority(&local_header, true));

        let local_authority = Request::builder()
            .uri("http://localhost:7331/api/health")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(allowed_request_authority(&local_authority, true));
    }

    #[test]
    fn bearer_and_query_tokens_authenticate_requests() {
        let request = Request::builder()
            .uri("/api/health")
            .header(header::HOST, "127.0.0.1:7331")
            .header(header::AUTHORIZATION, "Bearer secret-token")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(request_has_valid_token(&request, "secret-token", true));
        assert!(!request_has_valid_token(&request, "other", true));

        let query = Request::builder()
            .uri("/api/events?token=secret-token")
            .header(header::HOST, "127.0.0.1:7331")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(request_has_valid_token(&query, "secret-token", true));
        assert!(
            !request_has_valid_token(&query, "secret-token", false),
            "query tokens must not authenticate non-loopback listeners"
        );

        let rescan = Request::builder()
            .uri("/api/rescan?token=secret-token")
            .header(header::HOST, "127.0.0.1:7331")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(
            !request_has_valid_token(&rescan, "secret-token", true),
            "query tokens must not authenticate non-SSE endpoints"
        );
    }
}
