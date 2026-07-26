use super::churn_cache::{CachedDelta, CommitEvent, DeltaKind};
use anyhow::{Context, Result, bail};
use git2::{Oid, Repository};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

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
    ) -> Result<HashMap<String, CommitEvent>> {
        let executable = self
            .executable
            .as_deref()
            .context("native Git history streaming is disabled")?;
        let commits = prepare_commits(repo, oids)?;
        if commits.is_empty() {
            return Ok(HashMap::new());
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
        let parsed = parse_stream(BufReader::new(stdout), &commits);
        let status = child.wait().context("failed to wait for native Git")?;
        let written = writer
            .join()
            .map_err(|_| anyhow::anyhow!("native Git input writer panicked"))?;
        if !status.success() {
            bail!("native Git exited with {status}");
        }
        written.context("failed to stream revisions to native Git")?;
        let events = parsed?;
        Ok(events
            .into_iter()
            .map(|event| (event.oid.clone(), event))
            .collect())
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

fn parse_stream(mut reader: impl BufRead, commits: &[NativeCommit]) -> Result<Vec<CommitEvent>> {
    let mut events = Vec::with_capacity(commits.len());
    for commit in commits {
        let expected_header = commit.header();
        let mut actual_header = Vec::with_capacity(expected_header.len());
        reader.read_until(b'\n', &mut actual_header)?;
        if actual_header != expected_header {
            bail!("native Git returned an unexpected diff header");
        }

        let mut deltas = Vec::new();
        loop {
            let available = reader.fill_buf()?;
            if available.is_empty() || !available[0].is_ascii_uppercase() {
                break;
            }
            let status = read_nul_field(&mut reader)?;
            let path = path_from_git(read_nul_field(&mut reader)?)?;
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
        events.push(CommitEvent {
            oid: commit.oid.clone(),
            author: commit.author.clone(),
            seconds: commit.seconds,
            deltas,
            renames_resolved: false,
        });
    }
    if !reader.fill_buf()?.is_empty() {
        bail!("native Git returned trailing diff data");
    }
    Ok(events)
}

fn read_nul_field(reader: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut field = Vec::new();
    let read = reader.read_until(0, &mut field)?;
    if read == 0 || field.pop() != Some(0) {
        bail!("native Git returned a truncated NUL-delimited field");
    }
    Ok(field)
}

#[cfg(unix)]
fn path_from_git(bytes: Vec<u8>) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

#[cfg(windows)]
fn path_from_git(bytes: Vec<u8>) -> Result<PathBuf> {
    Ok(PathBuf::from(String::from_utf8(bytes)?))
}

#[cfg(test)]
mod tests {
    use super::{NativeCommit, parse_stream};
    use std::io::Cursor;
    use std::path::Path;

    fn commit(oid: &str, old_tree: &str, new_tree: &str) -> NativeCommit {
        NativeCommit {
            oid: oid.to_string(),
            author: Some("dev@example.com".to_string()),
            seconds: 42,
            old_tree: old_tree.to_string(),
            new_tree: new_tree.to_string(),
        }
    }

    #[test]
    fn parses_multiple_commits_and_nul_delimited_paths() {
        let commits = [commit("one", "aaaa", "bbbb"), commit("two", "bbbb", "cccc")];
        let bytes = b"aaaa bbbb\nM\0line\nbreak.rs\0A\0new.rs\0bbbb cccc\nD\0old.rs\0";
        let events = parse_stream(Cursor::new(bytes), &commits).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].deltas.len(), 2);
        assert_eq!(
            events[0].deltas[0].new_path.as_deref(),
            Some(Path::new("line\nbreak.rs"))
        );
        assert_eq!(
            events[1].deltas[0].old_path.as_deref(),
            Some(Path::new("old.rs"))
        );
    }

    #[test]
    fn rejects_truncated_records() {
        let commits = [commit("one", "aaaa", "bbbb")];
        let error = parse_stream(Cursor::new(b"aaaa bbbb\nM\0missing-nul"), &commits).unwrap_err();
        assert!(error.to_string().contains("truncated"));
    }
}
