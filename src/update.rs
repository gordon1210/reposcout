//! Receipt-backed, checksum-verified updates from immutable GitHub Releases.

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, CONTENT_LENGTH};
use semver::Version;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
#[cfg(unix)]
use std::io::Write;
use std::io::{ErrorKind, Read};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

pub const INSTALL_URL: &str = "https://getreposcout.vercel.app/install.sh";
const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/gordon1210/reposcout/releases/latest";
const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_XZ_MEMORY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 256;

pub fn run() -> Result<String> {
    let loaded = load_receipt()?;
    let executable = validate_receipt(&loaded.receipt, &loaded.path)?;
    let client = http_client()?;
    let latest = latest_release(&client)?;
    let latest_version = release_version(&latest)?;
    let current_version = Version::parse(&loaded.receipt.version)
        .context("the install receipt contains an invalid version")?;

    if latest_version <= current_version {
        return Ok(format!(
            "RepoScout {} is already up to date.\n",
            loaded.receipt.version
        ));
    }

    install_release(&client, loaded, &latest, &latest_version, &executable)?;
    Ok(format!(
        "Updated RepoScout from {} to {}.\n",
        current_version, latest_version
    ))
}

#[derive(Debug)]
struct LoadedReceipt {
    receipt: InstallReceipt,
    document: Value,
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct InstallReceipt {
    install_prefix: PathBuf,
    binaries: Vec<String>,
    source: ReceiptSource,
    version: String,
}

#[derive(Debug, Deserialize)]
struct ReceiptSource {
    app_name: String,
    name: String,
    owner: String,
    release_type: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    prerelease: bool,
    draft: bool,
    #[serde(default)]
    immutable: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
    size: u64,
}

fn load_receipt() -> Result<LoadedReceipt> {
    let path = receipt_path()?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(unmanaged_install_error());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("could not inspect install receipt {}", path.display()));
        }
    };
    if !metadata.file_type().is_file() {
        bail!("the RepoScout install receipt must be a regular file");
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o022 != 0 {
        bail!("the RepoScout install receipt must not be writable by group or other users");
    }

    let contents = fs::read_to_string(&path)
        .with_context(|| format!("could not read install receipt {}", path.display()))?;
    let document: Value = serde_json::from_str(&contents)
        .with_context(|| format!("could not parse install receipt {}", path.display()))?;
    let receipt = serde_json::from_value(document.clone())
        .with_context(|| format!("could not parse install receipt {}", path.display()))?;
    Ok(LoadedReceipt {
        receipt,
        document,
        path,
    })
}

fn validate_receipt(receipt: &InstallReceipt, path: &Path) -> Result<PathBuf> {
    if !receipt.binaries.iter().any(|binary| binary == "reposcout")
        || receipt.source.app_name != "reposcout"
        || receipt.source.name != "reposcout"
        || receipt.source.owner != "gordon1210"
        || receipt.source.release_type != "github"
    {
        return Err(unmanaged_install_error());
    }

    if receipt.version != env!("CARGO_PKG_VERSION") {
        bail!(
            "the RepoScout install receipt does not match this executable; install or update it with `curl --proto '=https' --tlsv1.2 -LsSf {INSTALL_URL} | sh`"
        );
    }

    let executable = std::env::current_exe()
        .context("could not locate the running RepoScout executable")?
        .canonicalize()
        .context("could not resolve the running RepoScout executable")?;
    let executable_dir = executable
        .parent()
        .ok_or_else(|| anyhow!("the running RepoScout executable has no parent directory"))?;
    let install_prefix = canonical_or_original(&receipt.install_prefix);
    let managed = executable_dir == install_prefix
        || (executable_dir.file_name().is_some_and(|name| name == "bin")
            && executable_dir.parent() == Some(install_prefix.as_path()));
    if !managed {
        return Err(unmanaged_install_error());
    }

    if path.parent().is_none() {
        bail!("the RepoScout install receipt has no parent directory");
    }
    Ok(executable)
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn http_client() -> Result<Client> {
    Client::builder()
        .user_agent(concat!("reposcout/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .build()
        .context("could not initialize the RepoScout update client")
}

fn latest_release(client: &Client) -> Result<GithubRelease> {
    let release = client
        .get(LATEST_RELEASE_API)
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .context("could not check the latest RepoScout release")?
        .error_for_status()
        .context("GitHub rejected the latest RepoScout release request")?
        .json()
        .context("GitHub returned an invalid RepoScout release")?;
    validate_release(&release)?;
    Ok(release)
}

fn validate_release(release: &GithubRelease) -> Result<()> {
    if release.draft || release.prerelease {
        bail!("GitHub's latest RepoScout release is not a stable published release");
    }
    if !release.immutable {
        bail!("GitHub's latest RepoScout release is not immutable");
    }
    release_version(release)?;
    release_asset(release)?;
    Ok(())
}

fn release_version(release: &GithubRelease) -> Result<Version> {
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    Version::parse(version).context("GitHub returned an invalid release version")
}

fn release_asset(release: &GithubRelease) -> Result<&GithubAsset> {
    let archive_name = archive_name_for_target(std::env::consts::OS, std::env::consts::ARCH)?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == archive_name)
        .ok_or_else(|| {
            anyhow!("the latest RepoScout release does not contain the archive {archive_name}")
        })?;
    let expected_url = format!(
        "https://github.com/gordon1210/reposcout/releases/download/{}/{archive_name}",
        release.tag_name
    );
    if asset.browser_download_url != expected_url {
        bail!("GitHub returned an unexpected RepoScout archive URL");
    }
    parse_sha256_digest(asset)?;
    if asset.size == 0 || asset.size > MAX_ARCHIVE_BYTES {
        bail!("the RepoScout release archive has an invalid size");
    }
    Ok(asset)
}

fn archive_name_for_target(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Ok("reposcout-aarch64-apple-darwin.tar.xz"),
        ("macos", "x86_64") => Ok("reposcout-x86_64-apple-darwin.tar.xz"),
        ("linux", "x86_64") => Ok("reposcout-x86_64-unknown-linux-gnu.tar.xz"),
        _ => bail!("built-in RepoScout updates are not available for {os}/{arch}"),
    }
}

fn parse_sha256_digest(asset: &GithubAsset) -> Result<&str> {
    let digest = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .ok_or_else(|| anyhow!("the RepoScout release archive has no SHA-256 digest"))?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("the RepoScout release archive has an invalid SHA-256 digest");
    }
    Ok(digest)
}

#[cfg(unix)]
fn install_release(
    client: &Client,
    mut loaded: LoadedReceipt,
    release: &GithubRelease,
    latest_version: &Version,
    executable: &Path,
) -> Result<()> {
    let asset = release_asset(release)?;
    let archive = download_archive(client, asset)?;
    verify_archive_digest(asset, &archive)?;

    let executable_dir = executable
        .parent()
        .ok_or_else(|| anyhow!("the running RepoScout executable has no parent directory"))?;
    let mut staged_binary = tempfile::Builder::new()
        .prefix(".reposcout-update-")
        .tempfile_in(executable_dir)
        .context("could not stage the RepoScout update beside the installed executable")?;
    extract_release_binary(&archive, staged_binary.as_file_mut())?;
    staged_binary
        .as_file_mut()
        .set_permissions(fs::Permissions::from_mode(0o755))
        .context("could not make the staged RepoScout update executable")?;
    staged_binary
        .as_file_mut()
        .sync_all()
        .context("could not flush the staged RepoScout update")?;

    let receipt_object = loaded
        .document
        .as_object_mut()
        .ok_or_else(|| anyhow!("the RepoScout install receipt must be a JSON object"))?;
    receipt_object.insert(
        "version".to_string(),
        Value::String(latest_version.to_string()),
    );
    let staged_receipt = stage_receipt(&loaded.path, &loaded.document)?;

    let mut backup = tempfile::Builder::new()
        .prefix(".reposcout-backup-")
        .tempfile_in(executable_dir)
        .context("could not stage a rollback copy of the installed RepoScout executable")?;
    fs::copy(executable, backup.path())
        .context("could not copy the installed RepoScout executable for rollback")?;
    backup
        .as_file_mut()
        .sync_all()
        .context("could not flush the RepoScout rollback copy")?;

    staged_binary
        .persist(executable)
        .map_err(|error| error.error)
        .context("could not replace the installed RepoScout executable")?;
    if let Err(error) = staged_receipt.persist(&loaded.path) {
        let receipt_error = error.error;
        if let Err(rollback_error) = backup.persist(executable) {
            bail!(
                "could not update the install receipt ({receipt_error}); rollback also failed ({})",
                rollback_error.error
            );
        }
        return Err(receipt_error).context(
            "could not update the install receipt; the previous RepoScout executable was restored",
        );
    }

    Ok(())
}

#[cfg(not(unix))]
fn install_release(
    _client: &Client,
    _loaded: LoadedReceipt,
    _release: &GithubRelease,
    _latest_version: &Version,
    _executable: &Path,
) -> Result<()> {
    bail!("built-in RepoScout updates are currently supported only on macOS and Linux")
}

fn download_archive(client: &Client, asset: &GithubAsset) -> Result<Vec<u8>> {
    let response = client
        .get(&asset.browser_download_url)
        .header(ACCEPT, "application/octet-stream")
        .send()
        .context("could not download the RepoScout release archive")?
        .error_for_status()
        .context("GitHub rejected the RepoScout release archive request")?;
    read_bounded_response(response)
}

fn read_bounded_response(mut response: Response) -> Result<Vec<u8>> {
    if response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > MAX_ARCHIVE_BYTES)
    {
        bail!("the RepoScout release archive exceeds the download limit");
    }

    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_ARCHIVE_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("could not read the RepoScout release archive")?;
    if bytes.len() as u64 > MAX_ARCHIVE_BYTES {
        bail!("the RepoScout release archive exceeds the download limit");
    }
    Ok(bytes)
}

fn verify_archive_digest(asset: &GithubAsset, archive: &[u8]) -> Result<()> {
    let expected = parse_sha256_digest(asset)?;
    let digest = Sha256::digest(archive);
    let mut actual = String::with_capacity(64);
    for byte in digest {
        write!(&mut actual, "{byte:02x}").expect("writing to a String cannot fail");
    }
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("the RepoScout release archive failed SHA-256 verification");
    }
    Ok(())
}

fn extract_release_binary(archive: &[u8], output: &mut fs::File) -> Result<()> {
    let stream = xz2::stream::Stream::new_stream_decoder(MAX_XZ_MEMORY_BYTES, 0)
        .context("could not initialize bounded XZ decompression")?;
    let decoder = xz2::read::XzDecoder::new_stream(archive, stream);
    let mut tar = tar::Archive::new(decoder);
    let mut found = false;
    let mut entries_seen = 0usize;
    let mut unpacked_bytes = 0u64;

    for entry in tar
        .entries()
        .context("could not read the RepoScout release archive")?
    {
        entries_seen = entries_seen.saturating_add(1);
        if entries_seen > MAX_ARCHIVE_ENTRIES {
            bail!("the RepoScout release archive contains too many entries");
        }
        let mut entry = entry.context("could not read a RepoScout release archive entry")?;
        let path = entry
            .path()
            .context("the RepoScout release archive contains an invalid path")?;
        if path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            bail!("the RepoScout release archive contains an unsafe path");
        }
        unpacked_bytes = unpacked_bytes.saturating_add(entry.size());
        if unpacked_bytes > MAX_UNPACKED_BYTES {
            bail!("the RepoScout release archive exceeds the extraction limit");
        }
        if !entry.header().entry_type().is_file()
            || path.file_name().is_none_or(|name| name != "reposcout")
        {
            continue;
        }
        if found {
            bail!("the RepoScout release archive contains multiple RepoScout binaries");
        }
        if entry.size() > MAX_BINARY_BYTES {
            bail!("the RepoScout release binary exceeds the extraction limit");
        }
        let copied = std::io::copy(&mut entry.by_ref().take(MAX_BINARY_BYTES + 1), output)
            .context("could not extract the RepoScout release binary")?;
        if copied > MAX_BINARY_BYTES {
            bail!("the RepoScout release binary exceeds the extraction limit");
        }
        found = true;
    }
    if !found {
        bail!("the RepoScout release archive does not contain the RepoScout binary");
    }
    Ok(())
}

#[cfg(unix)]
fn stage_receipt(path: &Path, document: &Value) -> Result<tempfile::NamedTempFile> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("the RepoScout install receipt has no parent directory"))?;
    let mut staged = tempfile::Builder::new()
        .prefix(".reposcout-receipt-")
        .tempfile_in(parent)
        .context("could not stage the updated RepoScout install receipt")?;
    serde_json::to_writer_pretty(staged.as_file_mut(), document)
        .context("could not serialize the updated RepoScout install receipt")?;
    staged
        .as_file_mut()
        .write_all(b"\n")
        .context("could not write the updated RepoScout install receipt")?;
    staged
        .as_file_mut()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .context("could not secure the updated RepoScout install receipt")?;
    staged
        .as_file_mut()
        .sync_all()
        .context("could not flush the updated RepoScout install receipt")?;
    Ok(staged)
}

fn receipt_path() -> Result<PathBuf> {
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(config_home)
            .join("reposcout")
            .join("reposcout-receipt.json"));
    }

    #[cfg(windows)]
    let home = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"));

    let home =
        home.ok_or_else(|| anyhow!("could not determine the user configuration directory"))?;
    Ok(home.join("reposcout").join("reposcout-receipt.json"))
}

fn unmanaged_install_error() -> anyhow::Error {
    anyhow!(
        "this RepoScout executable was not installed with the release installer; install or update it with `curl --proto '=https' --tlsv1.2 -LsSf {INSTALL_URL} | sh`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Seek;
    use xz2::write::XzEncoder;

    fn asset(name: &str, bytes: &[u8]) -> GithubAsset {
        let digest = Sha256::digest(bytes);
        let mut encoded = String::from("sha256:");
        for byte in digest {
            write!(&mut encoded, "{byte:02x}").unwrap();
        }
        GithubAsset {
            name: name.to_string(),
            browser_download_url: format!(
                "https://github.com/gordon1210/reposcout/releases/download/v0.2.0/{name}"
            ),
            digest: Some(encoded),
            size: bytes.len() as u64,
        }
    }

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let writer = XzEncoder::new(Vec::new(), 6);
        let mut builder = tar::Builder::new(writer);
        for (path, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, *body).unwrap();
        }
        let writer = builder.into_inner().unwrap();
        writer.finish().unwrap()
    }

    #[test]
    fn maps_supported_release_targets() {
        assert_eq!(
            archive_name_for_target("macos", "aarch64").unwrap(),
            "reposcout-aarch64-apple-darwin.tar.xz"
        );
        assert_eq!(
            archive_name_for_target("linux", "x86_64").unwrap(),
            "reposcout-x86_64-unknown-linux-gnu.tar.xz"
        );
        assert!(archive_name_for_target("linux", "aarch64").is_err());
    }

    #[test]
    fn verifies_the_archive_digest_independently() {
        let bytes = b"verified archive";
        let asset = asset("reposcout-aarch64-apple-darwin.tar.xz", bytes);
        verify_archive_digest(&asset, bytes).unwrap();
        assert!(verify_archive_digest(&asset, b"tampered archive").is_err());
    }

    #[test]
    fn requires_an_immutable_release_and_exact_platform_asset() {
        let name = archive_name_for_target(std::env::consts::OS, std::env::consts::ARCH).unwrap();
        let mut release = GithubRelease {
            tag_name: "v0.2.0".to_string(),
            prerelease: false,
            draft: false,
            immutable: false,
            assets: vec![asset(name, b"archive")],
        };

        assert!(validate_release(&release).is_err());
        release.immutable = true;
        validate_release(&release).unwrap();
        release.assets[0]
            .browser_download_url
            .push_str(".unexpected");
        assert!(validate_release(&release).is_err());
    }

    #[test]
    fn extracts_only_one_regular_reposcout_binary() {
        let bytes = archive(&[
            ("reposcout-v0.2.0/README.md", b"documentation"),
            ("reposcout-v0.2.0/reposcout", b"new binary"),
        ]);
        let output = tempfile::tempfile().unwrap();
        extract_release_binary(&bytes, &mut output.try_clone().unwrap()).unwrap();
        let mut actual = Vec::new();
        let mut output = output;
        output.rewind().unwrap();
        output.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, b"new binary");

        let duplicate = archive(&[("first/reposcout", b"one"), ("second/reposcout", b"two")]);
        assert!(extract_release_binary(&duplicate, &mut tempfile::tempfile().unwrap()).is_err());
    }
}
