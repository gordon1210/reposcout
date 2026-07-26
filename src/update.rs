//! Installer-receipt-backed updates from stable GitHub Releases.

use anyhow::{Context, Result, anyhow, bail};
use semver::Version;
use serde::Deserialize;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const INSTALL_URL: &str = "https://getreposcout.vercel.app/install.sh";
const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/gordon1210/reposcout/releases/latest";

pub fn run() -> Result<String> {
    let receipt = load_receipt()?;
    validate_receipt(&receipt)?;
    let latest = latest_release()?;
    let latest_version = latest
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&latest.tag_name);
    let current_version = Version::parse(&receipt.version)
        .context("the install receipt contains an invalid version")?;
    let latest_version =
        Version::parse(latest_version).context("GitHub returned an invalid release version")?;

    if latest_version <= current_version {
        return Ok(format!(
            "RepoScout {} is already up to date.\n",
            receipt.version
        ));
    }

    install_release(&receipt, &latest)?;
    Ok(format!(
        "Updated RepoScout from {} to {}.\n",
        current_version, latest_version
    ))
}

#[derive(Debug, Deserialize)]
struct InstallReceipt {
    install_prefix: PathBuf,
    binaries: Vec<String>,
    source: ReceiptSource,
    version: String,
    #[serde(default = "default_true")]
    modify_path: bool,
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
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

fn load_receipt() -> Result<InstallReceipt> {
    let path = receipt_path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(unmanaged_install_error());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("could not read install receipt {}", path.display()));
        }
    };
    serde_json::from_str(&contents)
        .with_context(|| format!("could not parse install receipt {}", path.display()))
}

fn validate_receipt(receipt: &InstallReceipt) -> Result<()> {
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
            "the RepoScout install receipt does not match this executable; install or update it with `curl -fsSL {INSTALL_URL} | sh`"
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

    Ok(())
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn default_true() -> bool {
    true
}

fn latest_release() -> Result<GithubRelease> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--header",
            "Accept: application/vnd.github+json",
            "--header",
            "X-GitHub-Api-Version: 2022-11-28",
            "--user-agent",
            concat!("reposcout/", env!("CARGO_PKG_VERSION")),
            LATEST_RELEASE_API,
        ])
        .output()
        .context("could not run curl to check for RepoScout updates")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "could not check the latest RepoScout release with curl{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }

    let release: GithubRelease =
        serde_json::from_slice(&output.stdout).context("GitHub returned an invalid release")?;
    if release.prerelease {
        bail!("GitHub's latest stable RepoScout release is marked as a prerelease");
    }
    let Some(installer) = release
        .assets
        .iter()
        .find(|asset| asset.name == "reposcout-installer.sh")
    else {
        bail!("the latest RepoScout release does not contain reposcout-installer.sh");
    };
    if installer.browser_download_url.is_empty() {
        bail!("the latest RepoScout release installer has no download URL");
    }

    Ok(release)
}

fn install_release(receipt: &InstallReceipt, release: &GithubRelease) -> Result<()> {
    #[cfg(not(unix))]
    bail!("built-in RepoScout updates are currently supported only on macOS and Linux");

    let installer = release
        .assets
        .iter()
        .find(|asset| asset.name == "reposcout-installer.sh")
        .ok_or_else(|| anyhow!("the latest RepoScout release has no shell installer"))?;
    let expected_prefix = format!(
        "https://github.com/gordon1210/reposcout/releases/download/{}/",
        release.tag_name
    );
    if !installer.browser_download_url.starts_with(&expected_prefix)
        || !installer
            .browser_download_url
            .ends_with("/reposcout-installer.sh")
    {
        bail!("GitHub returned an unexpected RepoScout installer URL");
    }

    let installer_file = tempfile::Builder::new()
        .prefix("reposcout-installer-")
        .suffix(".sh")
        .tempfile()
        .context("could not create a temporary installer file")?;
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--output",
        ])
        .arg(installer_file.path())
        .arg(&installer.browser_download_url)
        .status()
        .context("could not run curl to download the RepoScout installer")?;
    if !status.success() {
        bail!("curl could not download the RepoScout release installer");
    }

    let mut installer_command = Command::new("sh");
    installer_command
        .arg(installer_file.path())
        .env("CARGO_DIST_FORCE_INSTALL_DIR", &receipt.install_prefix)
        .env("REPOSCOUT_INSTALL_DIR", &receipt.install_prefix);
    if !receipt.modify_path {
        installer_command.env("REPOSCOUT_NO_MODIFY_PATH", "1");
    }
    let status = installer_command
        .status()
        .context("could not run the RepoScout release installer")?;
    if !status.success() {
        bail!("the RepoScout release installer failed");
    }

    Ok(())
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
        "this RepoScout executable was not installed with the release installer; install or update it with `curl -fsSL {INSTALL_URL} | sh`"
    )
}
