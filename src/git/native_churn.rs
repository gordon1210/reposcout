use super::ChurnLimits;
use super::churn_cache::{CachedDelta, CommitEvent, DeltaKind};
use anyhow::{Context, Result, bail};
use git2::{Oid, Repository};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub(super) struct NativeStreamStats {
    pub partial: bool,
    pub deltas_omitted: usize,
    pub output_bytes: u64,
}

pub(super) struct NativeGit {
    executable: Option<OsString>,
}

impl Default for NativeGit {
    fn default() -> Self {
        Self {
            executable: Some(OsString::from("git")),
        }
    }
}

impl NativeGit {
    #[cfg(test)]
    pub fn with_executable(executable: impl Into<OsString>) -> Self {
        Self {
            executable: Some(executable.into()),
        }
    }

    pub fn collect_events(
        &self,
        repo: &Repository,
        oids: &[Oid],
        limits: &ChurnLimits,
    ) -> Result<(HashMap<String, CommitEvent>, NativeStreamStats)> {
        let executable = self
            .executable
            .as_deref()
            .context("native Git history streaming is disabled")?;
        let commits = prepare_commits(repo, oids)?;
        if commits.is_empty() {
            return Ok((HashMap::new(), NativeStreamStats::default()));
        }

        let mut child = Command::new(executable)
            .arg("--git-dir")
            .arg(repo.path())
            .args([
                "diff-tree",
                "--stdin",
                "-r",
                "--name-status",
                "-z",
                "--no-renames",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--ignore-submodules=none",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to start {}", executable.to_string_lossy()))?;

        let mut stdin = child
            .stdin
            .take()
            .context("native Git stdin was unavailable")?;
        let headers = commits.iter().map(NativeCommit::header).collect::<Vec<_>>();
        let writer = std::thread::spawn(move || -> std::io::Result<()> {
            for header in headers {
                stdin.write_all(&header)?;
            }
            Ok(())
        });

        let stdout = child
            .stdout
            .take()
            .context("native Git stdout was unavailable")?;

        // Independent watchdog: if Git blocks inside a read past the deadline,
        // kill the child so the reader unblocks instead of waiting forever.
        let child_id = child.id();
        let watchdog_deadline = limits.deadline.unwrap_or_else(|| {
            Instant::now()
                .checked_add(Duration::from_secs(300))
                .unwrap_or_else(Instant::now)
        });
        let (watch_tx, watch_rx) = std::sync::mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            loop {
                let now = Instant::now();
                if now >= watchdog_deadline {
                    let _ = kill_process(child_id);
                    return true;
                }
                let remaining = watchdog_deadline.saturating_duration_since(now);
                let slice = remaining.min(Duration::from_millis(50));
                match watch_rx.recv_timeout(slice) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return false;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });

        let parse_result = parse_stream(BufReader::new(stdout), &commits, limits);
        let _ = watch_tx.send(());
        let timed_out = watchdog.join().unwrap_or(false);
        let (events, mut stats) = match parse_result {
            Ok((events, stats)) => (events, stats),
            // A deadline kill can leave a truncated stream; surface partial work.
            Err(_) if timed_out => (Vec::new(), NativeStreamStats::default()),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = writer.join();
                return Err(error);
            }
        };
        if timed_out {
            stats.partial = true;
        }

        if stats.partial || timed_out {
            let _ = child.kill();
        }
        let status = child.wait().context("failed to wait for native Git")?;
        let written = writer
            .join()
            .map_err(|_| anyhow::anyhow!("native Git input writer panicked"))?;
        if !stats.partial && !status.success() {
            bail!("native Git exited with {status}");
        }
        // Writer may fail after kill; ignore write errors once we already partial.
        if !stats.partial {
            written.context("failed to stream revisions to native Git")?;
        }
        Ok((
            events
                .into_iter()
                .map(|event| (event.oid.clone(), event))
                .collect(),
            stats,
        ))
    }
}

fn kill_process(pid: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // SAFETY: pid comes from a child we spawned in this process.
        let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        status.map(|_| ()).map_err(|error| error)
    }
}

struct NativeCommit {
    oid: String,
    author: Option<String>,
    seconds: i64,
    old_tree: String,
    new_tree: String,
}

impl NativeCommit {
    fn header(&self) -> Vec<u8> {
        format!("{} {}\n", self.old_tree, self.new_tree).into_bytes()
    }
}

fn prepare_commits(repo: &Repository, oids: &[Oid]) -> Result<Vec<NativeCommit>> {
    let mut commits = Vec::with_capacity(oids.len());
    for &oid in oids {
        let commit = repo.find_commit(oid)?;
        // Native Git cannot diff a missing parent tree without first materializing
        // the repository's empty-tree hash. The one root event stays on the
        // libgit2 fallback path; every ordinary commit is still handled in one batch.
        if commit.parent_count() == 0 {
            continue;
        }
        let parent = commit.parent(0)?;
        commits.push(NativeCommit {
            oid: oid.to_string(),
            author: commit.author().email().ok().map(str::to_owned),
            seconds: commit.time().seconds(),
            old_tree: parent.tree_id().to_string(),
            new_tree: commit.tree_id().to_string(),
        });
    }
    Ok(commits)
}

fn parse_stream(
    mut reader: impl BufRead,
    commits: &[NativeCommit],
    limits: &ChurnLimits,
) -> Result<(Vec<CommitEvent>, NativeStreamStats)> {
    let mut events = Vec::with_capacity(commits.len());
    let mut stats = NativeStreamStats::default();
    let mut total_deltas = 0usize;
    let started = Instant::now();
    let hard_timeout = limits
        .deadline
        .map(|deadline| deadline.saturating_duration_since(started))
        .unwrap_or(Duration::from_secs(300));

    for commit in commits {
        if Instant::now().duration_since(started) >= hard_timeout
            || limits
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
        {
            stats.partial = true;
            break;
        }
        if total_deltas >= limits.max_total_deltas {
            stats.partial = true;
            break;
        }
        if stats.output_bytes >= limits.max_output_bytes {
            stats.partial = true;
            break;
        }

        let expected_header = commit.header();
        let mut actual_header = Vec::with_capacity(expected_header.len());
        let header_read = read_until_budget(
            &mut reader,
            b'\n',
            &mut actual_header,
            limits.max_output_bytes.saturating_sub(stats.output_bytes),
        )?;
        stats.output_bytes = stats.output_bytes.saturating_add(header_read);
        if actual_header != expected_header {
            if stats.output_bytes >= limits.max_output_bytes {
                stats.partial = true;
                break;
            }
            bail!("native Git returned an unexpected diff header");
        }

        let mut deltas = Vec::new();
        loop {
            if Instant::now().duration_since(started) >= hard_timeout
                || limits
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
            {
                stats.partial = true;
                break;
            }
            if stats.output_bytes >= limits.max_output_bytes {
                stats.partial = true;
                break;
            }
            if total_deltas.saturating_add(deltas.len()) >= limits.max_total_deltas
                || deltas.len() >= limits.max_deltas_per_commit
            {
                // Drain remaining deltas for this commit without retaining them.
                if drain_commit_deltas(&mut reader, limits, &mut stats)? {
                    break;
                }
                stats.partial = true;
                break;
            }

            let available = reader.fill_buf()?;
            if available.is_empty() || !available[0].is_ascii_uppercase() {
                break;
            }
            let status = read_nul_field_budgeted(&mut reader, limits, &mut stats)?;
            if stats.partial {
                break;
            }
            let path_bytes = read_nul_field_budgeted(&mut reader, limits, &mut stats)?;
            if stats.partial {
                break;
            }
            if path_bytes.len() > limits.max_path_bytes {
                stats.partial = true;
                stats.deltas_omitted = stats.deltas_omitted.saturating_add(1);
                continue;
            }
            let path = path_from_git(path_bytes)?;
            let kind = match status.first().copied() {
                Some(b'A') => DeltaKind::Added,
                Some(b'D') => DeltaKind::Deleted,
                Some(b'M' | b'T' | b'U' | b'X' | b'B') => DeltaKind::Other,
                _ => bail!("native Git returned an unsupported status"),
            };
            let (old_path, new_path) = match kind {
                DeltaKind::Added => (None, Some(path)),
                DeltaKind::Deleted => (Some(path), None),
                DeltaKind::Other => (Some(path.clone()), Some(path)),
                DeltaKind::Renamed => unreachable!("rename detection is disabled in the stream"),
            };
            deltas.push(CachedDelta {
                kind,
                old_path,
                new_path,
            });
        }
        total_deltas = total_deltas.saturating_add(deltas.len());
        events.push(CommitEvent {
            oid: commit.oid.clone(),
            author: commit.author.clone(),
            seconds: commit.seconds,
            deltas,
            renames_resolved: false,
        });
        if stats.partial {
            break;
        }
    }
    if !stats.partial && !reader.fill_buf()?.is_empty() {
        bail!("native Git returned trailing diff data");
    }
    Ok((events, stats))
}

fn drain_commit_deltas(
    reader: &mut impl BufRead,
    limits: &ChurnLimits,
    stats: &mut NativeStreamStats,
) -> Result<bool> {
    loop {
        if stats.output_bytes >= limits.max_output_bytes {
            stats.partial = true;
            return Ok(true);
        }
        let available = reader.fill_buf()?;
        if available.is_empty() || !available[0].is_ascii_uppercase() {
            return Ok(true);
        }
        let _ = read_nul_field_budgeted(reader, limits, stats)?;
        let _ = read_nul_field_budgeted(reader, limits, stats)?;
        stats.deltas_omitted = stats.deltas_omitted.saturating_add(1);
        stats.partial = true;
        if stats.partial && stats.output_bytes >= limits.max_output_bytes {
            return Ok(true);
        }
    }
}

fn read_nul_field_budgeted(
    reader: &mut impl BufRead,
    limits: &ChurnLimits,
    stats: &mut NativeStreamStats,
) -> Result<Vec<u8>> {
    let mut field = Vec::new();
    let remaining = limits.max_output_bytes.saturating_sub(stats.output_bytes);
    let read = read_until_budget(reader, 0, &mut field, remaining)?;
    stats.output_bytes = stats.output_bytes.saturating_add(read);
    if read == 0 || field.pop() != Some(0) {
        if stats.output_bytes >= limits.max_output_bytes {
            stats.partial = true;
            return Ok(Vec::new());
        }
        bail!("native Git returned a truncated NUL-delimited field");
    }
    Ok(field)
}

fn read_until_budget(
    reader: &mut impl BufRead,
    delimiter: u8,
    out: &mut Vec<u8>,
    max_bytes: u64,
) -> Result<u64> {
    let mut total = 0u64;
    loop {
        if total >= max_bytes {
            return Ok(total);
        }
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(total);
        }
        let take = (max_bytes - total) as usize;
        let end = available.len().min(take);
        if let Some(pos) = available[..end].iter().position(|&b| b == delimiter) {
            out.extend_from_slice(&available[..=pos]);
            let consumed = pos + 1;
            reader.consume(consumed);
            total = total.saturating_add(consumed as u64);
            return Ok(total);
        }
        let consumed = end;
        let hit_budget = consumed < available.len();
        out.extend_from_slice(&available[..consumed]);
        reader.consume(consumed);
        total = total.saturating_add(consumed as u64);
        if hit_budget {
            // Hit the budget before finding the delimiter.
            return Ok(total);
        }
    }
}

#[cfg(unix)]
fn path_from_git(bytes: Vec<u8>) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(windows)]
fn path_from_git(bytes: Vec<u8>) -> Result<PathBuf> {
    let text =
        String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("invalid Git path encoding"))?;
    Ok(PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_stream_stops_at_delta_budget() {
        let commits = vec![NativeCommit {
            oid: "abc".into(),
            author: None,
            seconds: 1,
            old_tree: "old".into(),
            new_tree: "new".into(),
        }];
        let mut payload = b"old new\n".to_vec();
        for index in 0..5 {
            payload.extend_from_slice(format!("A\0file{index}.rs\0").as_bytes());
        }
        let limits = ChurnLimits {
            max_deltas_per_commit: 2,
            max_total_deltas: 2,
            max_output_bytes: 1024 * 1024,
            max_path_bytes: 256,
            ..ChurnLimits::default()
        };
        let (events, stats) =
            parse_stream(Cursor::new(payload), &commits, &limits).expect("parse stream");
        assert!(stats.partial);
        assert_eq!(events.len(), 1);
        assert!(events[0].deltas.len() <= 2);
        assert!(stats.deltas_omitted >= 1);
    }
}
