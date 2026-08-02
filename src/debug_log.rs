//! Opt-in, crash-resilient diagnostics for CLI runs.
//!
//! Callers emit named events through a deliberately small interface. This
//! module owns the NDJSON contract, timestamps, sequencing, synchronization,
//! flushing, file safety, and panic capture.

use anyhow::{Context, Result, anyhow};
use chrono::SecondsFormat;
use serde_json::{Value, json};
use std::backtrace::Backtrace;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Mutex, OnceLock, TryLockError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

static LOGGER: OnceLock<DebugLogger> = OnceLock::new();
const DEBUG_LOG_SCHEMA_VERSION: u32 = 1;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

struct DebugLogger {
    path: PathBuf,
    started: Instant,
    sequence: AtomicU64,
    writer: Mutex<BufWriter<File>>,
    activity: Mutex<Option<Activity>>,
    warned: AtomicBool,
}

struct Activity {
    event: String,
    sequence: u64,
    elapsed_ms: u64,
}

struct Heartbeat {
    stop: SyncSender<()>,
    handle: JoinHandle<()>,
}

/// Owns one CLI debug-log session. Event calls remain no-ops when disabled.
pub struct Session {
    enabled: bool,
    finished: bool,
    heartbeat: Option<Heartbeat>,
}

impl Session {
    /// Start a debug session when `path` is present.
    ///
    /// The file must not exist: diagnostics should never truncate an earlier
    /// log (or an accidentally selected source file).
    ///
    /// # Errors
    ///
    /// Returns an error when the requested log cannot be created securely or
    /// the initialized session cannot be registered.
    pub fn start(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self {
                enabled: false,
                finished: false,
                heartbeat: None,
            });
        };

        let file = OpenOptions::new()
            .append(true)
            .create_new(true)
            .open(path)
            .with_context(|| {
                format!(
                    "failed to create debug log {} (the path must not already exist)",
                    path.display()
                )
            })?;
        let path = path
            .canonicalize()
            .with_context(|| format!("failed to resolve debug log {}", path.display()))?;
        LOGGER
            .set(DebugLogger {
                path: path.clone(),
                started: Instant::now(),
                sequence: AtomicU64::new(0),
                writer: Mutex::new(BufWriter::new(file)),
                activity: Mutex::new(None),
                warned: AtomicBool::new(false),
            })
            .map_err(|_| anyhow!("debug log is already initialized"))?;

        install_panic_hook();
        event("session_start", || {
            json!({
                "version": env!("CARGO_PKG_VERSION"),
                "pid": std::process::id(),
                "cwd": std::env::current_dir().ok().map(|cwd| cwd.to_string_lossy().into_owned()),
                "argv": std::env::args_os()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
                "log_path": path.to_string_lossy(),
            })
        });
        let heartbeat = Heartbeat::start();

        Ok(Self {
            enabled: true,
            finished: false,
            heartbeat,
        })
    }

    /// Record the terminal outcome and flush it before the process exits.
    pub fn finish(&mut self, outcome: &'static str) {
        if !self.enabled || self.finished {
            return;
        }
        self.stop_heartbeat();
        event("session_end", || json!({ "outcome": outcome }));
        self.finished = true;
    }

    fn stop_heartbeat(&mut self) {
        if let Some(heartbeat) = self.heartbeat.take() {
            heartbeat.stop();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop_heartbeat();
        if self.enabled && !self.finished && !thread::panicking() {
            event("session_end", || json!({ "outcome": "interrupted" }));
        }
    }
}

/// Return the canonical debug-log path for exact scan/watch exclusion.
pub fn path() -> Option<&'static Path> {
    LOGGER.get().map(|logger| logger.path.as_path())
}

/// Whether diagnostic event construction is currently useful.
pub fn enabled() -> bool {
    LOGGER.get().is_some()
}

/// Emit one lazily constructed NDJSON event and flush it immediately.
pub fn event<F>(name: &'static str, data: F)
where
    F: FnOnce() -> Value,
{
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let data = data();
    if let Err(error) = logger.write_event(name, &data, false) {
        logger.warn(&error);
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(logger) = LOGGER.get() {
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|message| (*message).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic payload".to_string());
            let location = info.location().map(|location| {
                json!({
                    "file": location.file(),
                    "line": location.line(),
                    "column": location.column(),
                })
            });
            let data = json!({
                "message": message,
                "location": location,
                "backtrace": Backtrace::force_capture().to_string(),
            });
            if let Err(error) = logger.write_event("panic", &data, true) {
                logger.warn(&error);
            }
        }
        previous(info);
    }));
}

impl DebugLogger {
    fn write_event(&self, name: &str, data: &Value, emergency: bool) -> std::io::Result<()> {
        if emergency {
            match self.writer.try_lock() {
                Ok(mut writer) => self.write_locked(&mut writer, name, data, true),
                Err(TryLockError::Poisoned(error)) => {
                    let mut writer = error.into_inner();
                    self.write_locked(&mut writer, name, data, true)
                }
                Err(TryLockError::WouldBlock) => self.write_fallback(name, data),
            }
        } else {
            let mut writer = self
                .writer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.write_locked(&mut writer, name, data, false)
        }
    }

    fn write_locked(
        &self,
        writer: &mut BufWriter<File>,
        name: &str,
        data: &Value,
        sync: bool,
    ) -> std::io::Result<()> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let record = self.record(sequence, name, data);
        let mut encoded = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
        encoded.push(b'\n');
        writer.write_all(&encoded)?;
        writer.flush()?;
        self.note_activity(name, sequence);
        if sync {
            writer.get_ref().sync_data()?;
        }
        Ok(())
    }

    fn write_fallback(&self, name: &str, data: &Value) -> std::io::Result<()> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let record = self.record(sequence, name, data);
        let mut file = OpenOptions::new().append(true).open(&self.path)?;
        let mut encoded = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
        encoded.push(b'\n');
        file.write_all(&encoded)?;
        file.flush()?;
        self.note_activity(name, sequence);
        file.sync_data()
    }

    fn record(&self, sequence: u64, name: &str, data: &Value) -> Value {
        json!({
            "schema_version": DEBUG_LOG_SCHEMA_VERSION,
            "timestamp": chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            "elapsed_ms": u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "sequence": sequence,
            "thread": format!("{:?}", thread::current().id()),
            "event": name,
            "data": data,
        })
    }

    fn warn(&self, error: &std::io::Error) {
        if !self.warned.swap(true, Ordering::Relaxed) {
            eprintln!(
                "reposcout: warning: failed to write debug log {}: {error}",
                self.path.display()
            );
        }
    }

    fn note_activity(&self, name: &str, sequence: u64) {
        if name == "heartbeat" {
            return;
        }
        let mut activity = self
            .activity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *activity = Some(Activity {
            event: name.to_string(),
            sequence,
            elapsed_ms: elapsed_ms(self.started),
        });
    }

    fn heartbeat_data(&self) -> Value {
        let now = elapsed_ms(self.started);
        let activity = self
            .activity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (last_event, last_event_sequence, quiet_for_ms) =
            activity.as_ref().map_or((None, None, now), |activity| {
                (
                    Some(activity.event.clone()),
                    Some(activity.sequence),
                    now.saturating_sub(activity.elapsed_ms),
                )
            });
        json!({
            "last_event": last_event,
            "last_event_sequence": last_event_sequence,
            "quiet_for_ms": quiet_for_ms,
            "memory": process_memory_snapshot(),
        })
    }
}

impl Heartbeat {
    fn start() -> Option<Self> {
        let (stop, receiver) = mpsc::sync_channel(1);
        match thread::Builder::new()
            .name("reposcout-debug-heartbeat".to_string())
            .spawn(move || {
                heartbeat_loop(&receiver, HEARTBEAT_INTERVAL, || {
                    event("heartbeat", || {
                        LOGGER
                            .get()
                            .map_or_else(|| json!({}), DebugLogger::heartbeat_data)
                    });
                });
            }) {
            Ok(handle) => Some(Self { stop, handle }),
            Err(error) => {
                event(
                    "debug_log_warning",
                    || json!({ "message": format!("failed to start heartbeat: {error}") }),
                );
                None
            }
        }
    }

    fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.handle.join();
    }
}

fn heartbeat_loop(receiver: &Receiver<()>, interval: Duration, mut heartbeat: impl FnMut()) {
    loop {
        match receiver.recv_timeout(interval) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => heartbeat(),
        }
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(target_os = "linux")]
fn process_memory_snapshot() -> Value {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return json!({ "available": false });
    };
    let kibibytes = |label: &str| {
        status.lines().find_map(|line| {
            line.strip_prefix(label)?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
    };
    json!({
        "available": true,
        "rss_bytes": kibibytes("VmRSS:").map(|value| value.saturating_mul(1024)),
        "peak_rss_bytes": kibibytes("VmHWM:").map(|value| value.saturating_mul(1024)),
    })
}

#[cfg(not(target_os = "linux"))]
fn process_memory_snapshot() -> Value {
    json!({ "available": false })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_loop_ticks_while_work_is_quiet_and_stops_promptly() {
        let (stop, receiver) = mpsc::sync_channel(1);
        let (observed, wait_for_tick) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            heartbeat_loop(&receiver, Duration::from_millis(5), || {
                let _ = observed.try_send(());
            });
        });

        wait_for_tick
            .recv_timeout(Duration::from_millis(100))
            .expect("heartbeat tick");
        stop.send(()).unwrap();
        handle.join().unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_memory_snapshot_reports_resident_bytes() {
        let memory = process_memory_snapshot();
        assert_eq!(memory["available"], true);
        assert!(memory["rss_bytes"].as_u64().is_some());
    }
}
