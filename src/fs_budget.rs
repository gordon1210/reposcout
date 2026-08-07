//! Bounded, symlink-safe readers for repository-controlled inputs.
//!
//! Scan discovery, graph resolvers, ignore loaders, and caches share these
//! helpers so secondary analysis paths cannot bypass resource limits.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::time::Instant;

use crate::numeric::{u64_to_usize, usize_to_u64};

/// Default maximum size for a single ignore rule file.
pub const DEFAULT_MAX_IGNORE_FILE_BYTES: u64 = 1024 * 1024;
/// Default maximum number of non-empty pattern lines per ignore file.
pub const DEFAULT_MAX_IGNORE_LINES: usize = 50_000;
/// Default maximum length of one ignore pattern line.
pub const DEFAULT_MAX_IGNORE_LINE_BYTES: usize = 8_192;
/// Hard ceiling for analysis-cache files loaded from disk.
pub const DEFAULT_MAX_CACHE_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Cooperative budget shared by repository file reads.
#[derive(Debug, Clone)]
pub struct ReadBudget {
    pub max_file_bytes: u64,
    pub remaining_total_bytes: u64,
    pub remaining_files: usize,
    pub deadline: Option<Instant>,
}

impl ReadBudget {
    #[must_use]
    pub fn new(
        max_file_bytes: u64,
        max_total_bytes: u64,
        max_files: usize,
        deadline: Option<Instant>,
    ) -> Self {
        Self {
            max_file_bytes: max_file_bytes.max(1),
            remaining_total_bytes: max_total_bytes.max(1),
            remaining_files: max_files.max(1),
            deadline,
        }
    }

    #[must_use]
    pub fn from_limits(max_file_bytes: u64, max_total_bytes: u64, max_files: usize) -> Self {
        Self::new(max_file_bytes, max_total_bytes, max_files, None)
    }

    #[must_use]
    pub fn exhausted(&self) -> bool {
        self.remaining_files == 0
            || self.remaining_total_bytes == 0
            || self
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
    }

    fn consume(&mut self, bytes: u64) {
        self.remaining_files = self.remaining_files.saturating_sub(1);
        self.remaining_total_bytes = self.remaining_total_bytes.saturating_sub(bytes);
    }
}

/// Outcome of a bounded repository read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    Content(String),
    Oversized(u64),
    BudgetExceeded,
    DeadlineExceeded,
    NotRegularFile,
    Unreadable,
}

/// Limits applied when loading ignore rule files.
#[derive(Debug, Clone, Copy)]
pub struct IgnoreLimits {
    pub max_file_bytes: u64,
    pub max_lines: usize,
    pub max_line_bytes: usize,
}

impl Default for IgnoreLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_IGNORE_FILE_BYTES,
            max_lines: DEFAULT_MAX_IGNORE_LINES,
            max_line_bytes: DEFAULT_MAX_IGNORE_LINE_BYTES,
        }
    }
}

/// True only for non-symlink regular files.
#[must_use]
pub fn is_regular_file(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) => metadata.is_file() && !metadata.file_type().is_symlink(),
        Err(_) => false,
    }
}

/// Read a regular file with a shared budget, rejecting symlinks and caps.
pub fn read_text(path: &Path, budget: &mut ReadBudget) -> ReadOutcome {
    if budget
        .deadline
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        return ReadOutcome::DeadlineExceeded;
    }
    if budget.remaining_files == 0 || budget.remaining_total_bytes == 0 {
        return ReadOutcome::BudgetExceeded;
    }

    let Ok(metadata) = fs::symlink_metadata(path) else {
        return ReadOutcome::Unreadable;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return ReadOutcome::NotRegularFile;
    }

    let file_bytes = metadata.len();
    if file_bytes > budget.max_file_bytes {
        return ReadOutcome::Oversized(file_bytes);
    }
    if file_bytes > budget.remaining_total_bytes {
        return ReadOutcome::BudgetExceeded;
    }

    match read_regular_file_bytes(path, file_bytes, budget.max_file_bytes) {
        Ok(bytes) => {
            // Charge the budget for every successful byte read, including invalid
            // UTF-8. Otherwise hostile non-UTF-8 blobs can bypass total/file caps.
            let len = usize_to_u64(bytes.len());
            budget.consume(len);
            match String::from_utf8(bytes) {
                Ok(content) => ReadOutcome::Content(content),
                Err(_) => ReadOutcome::Unreadable,
            }
        }
        Err(ReadOutcome::Oversized(size)) => ReadOutcome::Oversized(size),
        Err(other) => other,
    }
}

/// Read a regular file with a single-file size cap (no shared total budget).
#[must_use]
pub fn read_text_limited(path: &Path, max_file_bytes: u64) -> ReadOutcome {
    let mut budget = ReadBudget::from_limits(max_file_bytes, max_file_bytes, 1);
    read_text(path, &mut budget)
}

/// Read raw bytes from a regular file with a size ceiling.
///
/// # Errors
///
/// Returns a [`ReadOutcome`] error when the path is unreadable, is not a
/// regular non-symlink file, or exceeds `max_file_bytes`.
pub fn read_bytes_limited(path: &Path, max_file_bytes: u64) -> Result<Vec<u8>, ReadOutcome> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Err(ReadOutcome::Unreadable);
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ReadOutcome::NotRegularFile);
    }
    let file_bytes = metadata.len();
    if file_bytes > max_file_bytes {
        return Err(ReadOutcome::Oversized(file_bytes));
    }
    read_regular_file_bytes(path, file_bytes, max_file_bytes)
}

/// Load an ignore file, enforce size/line limits, and reject symlinks.
///
/// # Errors
///
/// Returns a [`ReadOutcome`] error when the file cannot be read within the
/// supplied byte, line-count, or line-length limits.
pub fn read_ignore_file(path: &Path, limits: IgnoreLimits) -> Result<String, ReadOutcome> {
    let outcome = read_text_limited(path, limits.max_file_bytes);
    let ReadOutcome::Content(content) = outcome else {
        return Err(outcome);
    };

    let mut kept = String::with_capacity(content.len().min(u64_to_usize(limits.max_file_bytes)));
    let mut patterns = 0usize;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.len() > limits.max_line_bytes {
            return Err(ReadOutcome::Oversized(usize_to_u64(trimmed.len())));
        }
        patterns = patterns.saturating_add(1);
        if patterns > limits.max_lines {
            return Err(ReadOutcome::BudgetExceeded);
        }
        kept.push_str(line);
        kept.push('\n');
    }
    Ok(kept)
}

fn read_regular_file_bytes(
    path: &Path,
    metadata_bytes: u64,
    max_file_bytes: u64,
) -> Result<Vec<u8>, ReadOutcome> {
    let mut file = match open_nofollow(path) {
        Ok(file) => file,
        Err(ReadOutcome::NotRegularFile) => return Err(ReadOutcome::NotRegularFile),
        Err(_) => return Err(ReadOutcome::Unreadable),
    };
    // Re-check after open: reject if the opened handle is larger than expected.
    if let Ok(opened) = file.metadata()
        && opened.len() > max_file_bytes
    {
        return Err(ReadOutcome::Oversized(opened.len()));
    }

    let capacity = usize::try_from(metadata_bytes.min(max_file_bytes)).unwrap_or(usize::MAX);
    let mut bytes = Vec::with_capacity(capacity);
    if file
        .by_ref()
        .take(max_file_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Err(ReadOutcome::Unreadable);
    }
    if bytes.len() as u64 > max_file_bytes {
        return Err(ReadOutcome::Oversized(
            (bytes.len() as u64).max(metadata_bytes),
        ));
    }
    Ok(bytes)
}

/// Open a path without following the final path component when the platform
/// supports `O_NOFOLLOW`.
fn open_nofollow(path: &Path) -> Result<File, ReadOutcome> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        match fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => {
                // Defend against racey replacement: require a regular file handle.
                if file.metadata().is_ok_and(|meta| meta.is_file()) {
                    Ok(file)
                } else {
                    Err(ReadOutcome::NotRegularFile)
                }
            }
            Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
                Err(ReadOutcome::NotRegularFile)
            }
            Err(_) => Err(ReadOutcome::Unreadable),
        }
    }
    #[cfg(not(unix))]
    {
        // Windows lacks a portable O_NOFOLLOW equivalent in std; keep the
        // symlink_metadata pre-check and refuse non-regular opened files.
        if !is_regular_file(path) {
            return Err(ReadOutcome::NotRegularFile);
        }
        match File::open(path) {
            Ok(file) => Ok(file),
            Err(_) => Err(ReadOutcome::Unreadable),
        }
    }
}

/// Atomically replace `path` with `bytes` inside the same directory.
///
/// Rejects an existing symlink target and uses owner-only permissions where the
/// platform supports them. Permission failures are propagated so callers cannot
/// silently leave a world-readable cache artifact behind.
///
/// # Errors
///
/// Returns an error when the destination has no parent, an existing target is
/// a symlink, permissions cannot be secured, or the atomic write/rename fails.
pub fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cache path has no parent directory",
        ));
    };
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }

    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "refusing to overwrite a symlink cache path",
        ));
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cache.json");
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        fs::write(&temporary, bytes)?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if path.exists() {
            // Another process may have created a regular file; replace only if
            // it is not a symlink.
            if let Ok(metadata) = fs::symlink_metadata(path)
                && metadata.file_type().is_symlink()
            {
                let _ = fs::remove_file(&temporary);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "refusing to overwrite a symlink cache path",
                ));
            }
            fs::remove_file(path)?;
            fs::rename(&temporary, path)?;
        } else {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Atomically replace a user-selected output file without following symlinks.
///
/// Existing regular-file permissions are preserved. Existing destination
/// symlinks and symlinked parent components below `untrusted_root` or the
/// current directory are rejected; a destination that becomes a symlink after
/// validation is atomically replaced rather than opened, so its target is
/// never modified. On Unix, staging and replacement stay relative to an opened
/// parent-directory handle, so replacing the validated parent path cannot
/// redirect the write.
///
/// # Errors
///
/// Returns an error when the destination has no file name, its parent cannot
/// be resolved without symlinks, an existing destination is not a regular
/// file, or staging, synchronization, or atomic replacement fails.
pub fn write_output_atomic(
    path: &Path,
    bytes: &[u8],
    untrusted_root: &Path,
) -> std::io::Result<()> {
    write_output_atomic_impl(path, bytes, untrusted_root, |_| Ok(()))
}

fn write_output_atomic_impl<F>(
    path: &Path,
    bytes: &[u8],
    untrusted_root: &Path,
    after_parent_validation: F,
) -> std::io::Result<()>
where
    F: FnOnce(&Path) -> std::io::Result<()>,
{
    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "output path has no file name",
        )
    })?;
    let parent = output_parent_without_symlinks(path, untrusted_root)?;

    #[cfg(unix)]
    {
        let directory = rustix::fs::open(
            &parent,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::empty(),
        )?;
        after_parent_validation(&parent)?;
        write_output_in_directory(&directory, file_name, bytes, || Ok(()))
    }

    #[cfg(not(unix))]
    {
        after_parent_validation(&parent)?;
        write_output_portable(&parent, file_name, bytes)
    }
}

#[cfg(unix)]
fn write_output_in_directory<F>(
    directory: &rustix::fd::OwnedFd,
    file_name: &std::ffi::OsStr,
    bytes: &[u8],
    after_destination_validation: F,
) -> std::io::Result<()>
where
    F: FnOnce() -> std::io::Result<()>,
{
    use std::io::Write as _;

    let existing_mode =
        match rustix::fs::statat(directory, file_name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => match rustix::fs::FileType::from_raw_mode(stat.st_mode) {
                rustix::fs::FileType::Symlink => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "refusing to overwrite an output symlink",
                    ));
                }
                rustix::fs::FileType::RegularFile => {
                    Some(rustix::fs::Mode::from_raw_mode(stat.st_mode))
                }
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "output path is not a regular file",
                    ));
                }
            },
            Err(rustix::io::Errno::NOENT) => None,
            Err(error) => return Err(error.into()),
        };
    after_destination_validation()?;

    let (temporary_name, mut temporary) = create_output_temporary(directory)?;
    let result = (|| {
        temporary.write_all(bytes)?;
        if let Some(mode) = existing_mode {
            rustix::fs::fchmod(&temporary, mode)?;
        }
        temporary.sync_all()?;
        rustix::fs::renameat(directory, &temporary_name, directory, file_name)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = rustix::fs::unlinkat(directory, &temporary_name, rustix::fs::AtFlags::empty());
    }
    result
}

#[cfg(unix)]
fn create_output_temporary(
    directory: &rustix::fd::OwnedFd,
) -> std::io::Result<(std::ffi::OsString, File)> {
    const MAX_ATTEMPTS: usize = 16;

    for _ in 0..MAX_ATTEMPTS {
        let nonce = getrandom::u64().map_err(|error| std::io::Error::other(error.to_string()))?;
        let name = std::ffi::OsString::from(format!(
            ".reposcout-output-{}-{nonce:016x}.tmp",
            std::process::id()
        ));
        match rustix::fs::openat(
            directory,
            &name,
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        ) {
            Ok(file) => return Ok((name, File::from(file))),
            Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(error.into()),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique output staging file",
    ))
}

#[cfg(not(unix))]
fn write_output_portable(
    parent: &Path,
    file_name: &std::ffi::OsStr,
    bytes: &[u8],
) -> std::io::Result<()> {
    use std::io::Write as _;

    let destination = parent.join(file_name);
    let existing_permissions = match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "refusing to overwrite an output symlink",
            ));
        }
        Ok(metadata) if metadata.is_file() => Some(metadata.permissions()),
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "output path is not a regular file",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    let mut temporary = tempfile::Builder::new()
        .prefix(".reposcout-output-")
        .tempfile_in(parent)?;
    temporary.write_all(bytes)?;
    if let Some(permissions) = existing_permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.as_file_mut().sync_all()?;
    persist_output(temporary, &destination)
}

fn output_parent_without_symlinks(
    path: &Path,
    untrusted_root: &Path,
) -> std::io::Result<std::path::PathBuf> {
    let requested = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let absolute = std::path::absolute(requested)?;

    if fs::symlink_metadata(&absolute)?.file_type().is_symlink() {
        return Err(symlinked_output_parent_error());
    }
    reject_symlinked_descendants(&std::env::current_dir()?, &absolute)?;
    reject_symlinked_descendants(untrusted_root, &absolute)?;
    absolute.canonicalize()
}

fn reject_symlinked_descendants(boundary: &Path, parent: &Path) -> std::io::Result<()> {
    let boundary = std::path::absolute(boundary)?;
    let Ok(relative) = parent.strip_prefix(&boundary) else {
        return Ok(());
    };
    if fs::symlink_metadata(&boundary)?.file_type().is_symlink() {
        return Err(symlinked_output_parent_error());
    }
    let mut candidate = boundary;
    for component in relative.components() {
        candidate.push(component);
        if fs::symlink_metadata(&candidate)?.file_type().is_symlink() {
            return Err(symlinked_output_parent_error());
        }
    }
    Ok(())
}

fn symlinked_output_parent_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "refusing to write output through a symlinked parent directory",
    )
}

#[cfg(not(unix))]
fn persist_output(temporary: tempfile::NamedTempFile, destination: &Path) -> std::io::Result<()> {
    temporary
        .persist(destination)
        .map(|_| ())
        .map_err(|error| error.error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;
    use std::io::Write;

    #[test]
    fn rejects_symlinks_and_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");
        fs::write(&target, "hello").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert!(!is_regular_file(&link));
            assert_eq!(read_text_limited(&link, 1024), ReadOutcome::NotRegularFile);
        }

        let big = dir.path().join("big.txt");
        let mut file = File::create(&big).unwrap();
        file.write_all(&[b'a'; 64]).unwrap();
        assert_eq!(read_text_limited(&big, 32), ReadOutcome::Oversized(64));
    }

    #[test]
    fn shared_budget_tracks_total_bytes_and_file_count() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "12345").unwrap();
        fs::write(&b, "12345").unwrap();
        let mut budget = ReadBudget::from_limits(100, 6, 10);
        assert!(matches!(
            read_text(&a, &mut budget),
            ReadOutcome::Content(_)
        ));
        assert_eq!(read_text(&b, &mut budget), ReadOutcome::BudgetExceeded);
    }

    #[test]
    fn invalid_utf8_still_consumes_budget() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary.bin");
        fs::write(&path, [0xff, 0xfe, 0xfd, 0xfc, 0xfb]).unwrap();
        let mut budget = ReadBudget::from_limits(100, 10, 10);
        assert_eq!(read_text(&path, &mut budget), ReadOutcome::Unreadable);
        assert_eq!(budget.remaining_total_bytes, 5);
        assert_eq!(budget.remaining_files, 9);
    }

    #[test]
    fn ignore_loader_rejects_too_many_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".gitignore");
        let mut body = String::new();
        for index in 0..10 {
            let _ = writeln!(body, "pattern-{index}");
        }
        fs::write(&path, body).unwrap();
        let limits = IgnoreLimits {
            max_file_bytes: 1024,
            max_lines: 5,
            max_line_bytes: 128,
        };
        assert_eq!(
            read_ignore_file(&path, limits).unwrap_err(),
            ReadOutcome::BudgetExceeded
        );
    }

    #[test]
    fn atomic_output_replaces_regular_files() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("report.json");
        fs::write(&output, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            fs::set_permissions(&output, fs::Permissions::from_mode(0o640)).unwrap();
        }

        write_output_atomic(&output, b"new", dir.path()).unwrap();

        assert_eq!(fs::read(&output).unwrap(), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            assert_eq!(
                fs::metadata(output).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn atomic_output_rejects_destination_and_parent_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        let output = dir.path().join("report.json");
        fs::write(&victim, "sentinel").unwrap();
        symlink(&victim, &output).unwrap();

        let error = write_output_atomic(&output, b"replacement", dir.path()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(fs::read_to_string(&victim).unwrap(), "sentinel");

        let external = tempfile::tempdir().unwrap();
        let linked_parent = dir.path().join("linked-parent");
        symlink(external.path(), &linked_parent).unwrap();
        let error = write_output_atomic(&linked_parent.join("report.json"), b"report", dir.path())
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!external.path().join("report.json").exists());

        let linked_root = dir.path().join("linked-root");
        fs::create_dir(external.path().join("nested")).unwrap();
        symlink(external.path(), &linked_root).unwrap();
        let error = write_output_atomic(
            &linked_root.join("nested/report.json"),
            b"report",
            &linked_root,
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!external.path().join("nested/report.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_output_replaces_a_raced_symlink_instead_of_its_target() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim");
        let output = dir.path().join("report.json");
        fs::write(&victim, "sentinel").unwrap();
        let directory = rustix::fs::open(
            dir.path(),
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::CLOEXEC
                | rustix::fs::OFlags::NOFOLLOW,
            rustix::fs::Mode::empty(),
        )
        .unwrap();

        write_output_in_directory(&directory, output.file_name().unwrap(), b"report", || {
            symlink(&victim, &output)
        })
        .unwrap();

        assert_eq!(fs::read_to_string(&victim).unwrap(), "sentinel");
        assert_eq!(fs::read_to_string(&output).unwrap(), "report");
        assert!(
            !fs::symlink_metadata(&output)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_output_parent_swap_cannot_redirect_the_write() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("output-parent");
        let retained_parent = root.path().join("retained-parent");
        let external = tempfile::tempdir().unwrap();
        let output = parent.join("report.json");
        let external_output = external.path().join("report.json");
        fs::create_dir(&parent).unwrap();
        fs::write(&external_output, "sentinel").unwrap();

        write_output_atomic_impl(&output, b"report", root.path(), |validated_parent| {
            fs::rename(validated_parent, &retained_parent)?;
            symlink(external.path(), validated_parent)
        })
        .unwrap();

        assert_eq!(fs::read_to_string(external_output).unwrap(), "sentinel");
        assert_eq!(
            fs::read_to_string(retained_parent.join("report.json")).unwrap(),
            "report"
        );
    }

    #[test]
    fn concurrent_atomic_outputs_never_interleave() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("report.json");
        let output_root = dir.path().to_path_buf();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let first = "a".repeat(64 * 1024);
        let second = "b".repeat(64 * 1024);

        let handles = [first.clone(), second.clone()].map(|content| {
            let output = output.clone();
            let output_root = output_root.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                write_output_atomic(&output, content.as_bytes(), &output_root).unwrap();
            })
        });
        barrier.wait();
        for handle in handles {
            handle.join().unwrap();
        }

        let rendered = fs::read_to_string(output).unwrap();
        assert!(rendered == first || rendered == second);
    }
}
