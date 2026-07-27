use crate::fs_budget::{self, DEFAULT_MAX_CACHE_FILE_BYTES, ReadOutcome};
use crate::model::Churn;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::xxh3_64;

/// Bumped when event/view cache identity or semantics change.
const CACHE_VERSION: &str = "2";
const MAX_VIEWS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum DeltaKind {
    Added,
    Deleted,
    Renamed,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CachedDelta {
    pub kind: DeltaKind,
    #[serde(default, with = "optional_path")]
    pub old_path: Option<PathBuf>,
    #[serde(default, with = "optional_path")]
    pub new_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CommitEvent {
    pub oid: String,
    pub author: Option<String>,
    pub seconds: i64,
    pub deltas: Vec<CachedDelta>,
    pub renames_resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ViewIdentity {
    pub head: String,
    pub history_state: String,
    pub max_commits: usize,
    pub wanted_paths: Vec<String>,
    /// Resource limits that affect which deltas are retained.
    #[serde(default)]
    pub max_deltas_per_commit: usize,
    #[serde(default)]
    pub max_total_deltas: usize,
    #[serde(default)]
    pub max_output_bytes: u64,
    #[serde(default)]
    pub max_path_bytes: usize,
    #[serde(default)]
    pub max_cache_bytes: u64,
}

impl ViewIdentity {
    pub fn new(
        head: String,
        history_state: String,
        limits: &super::ChurnLimits,
        wanted: impl Iterator<Item = PathBuf>,
    ) -> Self {
        let mut wanted_paths = wanted.map(|path| encode_path(&path)).collect::<Vec<_>>();
        wanted_paths.sort();
        wanted_paths.dedup();
        ViewIdentity {
            head,
            history_state,
            max_commits: limits.max_commits,
            wanted_paths,
            max_deltas_per_commit: limits.max_deltas_per_commit,
            max_total_deltas: limits.max_total_deltas,
            max_output_bytes: limits.max_output_bytes,
            max_path_bytes: limits.max_path_bytes,
            max_cache_bytes: limits.max_cache_bytes,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ViewResult {
    #[serde(with = "path")]
    path: PathBuf,
    churn: Churn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ViewEntry {
    identity: ViewIdentity,
    results: Vec<ViewResult>,
}

#[derive(Default, Serialize, Deserialize)]
struct EventsData {
    version: String,
    history_state: String,
    events: Vec<CommitEvent>,
}

#[derive(Default, Serialize, Deserialize)]
struct ViewsData {
    version: String,
    views: Vec<ViewEntry>,
}

pub(super) struct ChurnCache {
    enabled: bool,
    events_path: PathBuf,
    views_path: PathBuf,
    events: Option<HashMap<String, CommitEvent>>,
    history_state: String,
    events_dirty: bool,
    views: Vec<ViewEntry>,
    views_dirty: bool,
    max_cache_bytes: u64,
}

impl ChurnCache {
    pub fn for_repo(root: &Path, enabled: bool, max_cache_bytes: u64) -> Self {
        let Some(base) = cache_directory(root) else {
            return Self::disabled();
        };
        Self::at(base, enabled, max_cache_bytes.max(1))
    }

    fn at(base: PathBuf, enabled: bool, max_cache_bytes: u64) -> Self {
        if !enabled {
            return Self::disabled();
        }
        let views_path = base.join("views.json");
        // Load views with the configured bound so Safe mode never materializes a
        // larger on-disk artifact than the active profile permits.
        let views = load_json::<ViewsData>(&views_path, max_cache_bytes)
            .filter(|data| data.version == CACHE_VERSION)
            .map(|data| data.views)
            .unwrap_or_default();
        ChurnCache {
            enabled: true,
            events_path: base.join("events.json"),
            views_path,
            events: None,
            history_state: String::new(),
            events_dirty: false,
            views,
            views_dirty: false,
            max_cache_bytes,
        }
    }

    #[cfg(test)]
    pub fn for_test(base: &Path) -> Self {
        Self::at(base.to_path_buf(), true, DEFAULT_MAX_CACHE_FILE_BYTES)
    }

    fn disabled() -> Self {
        ChurnCache {
            enabled: false,
            events_path: PathBuf::new(),
            views_path: PathBuf::new(),
            events: None,
            history_state: String::new(),
            events_dirty: false,
            views: Vec::new(),
            views_dirty: false,
            max_cache_bytes: DEFAULT_MAX_CACHE_FILE_BYTES,
        }
    }

    pub fn get_view(&mut self, identity: &ViewIdentity) -> Option<HashMap<PathBuf, Churn>> {
        if !self.enabled {
            return None;
        }
        let index = self
            .views
            .iter()
            .position(|entry| entry.identity == *identity)?;
        let entry = self.views.remove(index);
        let results = entry
            .results
            .iter()
            .map(|result| (result.path.clone(), result.churn.clone()))
            .collect();
        self.views.insert(0, entry);
        self.views_dirty = index != 0;
        Some(results)
    }

    pub fn put_view(&mut self, identity: ViewIdentity, churn: &HashMap<PathBuf, Churn>) {
        if !self.enabled {
            return;
        }
        self.views.retain(|entry| entry.identity != identity);
        let mut results = churn
            .iter()
            .map(|(path, churn)| ViewResult {
                path: path.clone(),
                churn: churn.clone(),
            })
            .collect::<Vec<_>>();
        results.sort_by(|a, b| a.path.cmp(&b.path));
        self.views.insert(0, ViewEntry { identity, results });
        self.views.truncate(MAX_VIEWS);
        self.views_dirty = true;
    }

    pub fn load_events(&mut self, history_state: &str) {
        if !self.enabled || self.events.is_some() {
            return;
        }
        self.history_state = history_state.to_string();
        let events = load_json::<EventsData>(&self.events_path, self.max_cache_bytes)
            .filter(|data| {
                data.version == CACHE_VERSION && data.history_state == self.history_state
            })
            .map(|data| {
                data.events
                    .into_iter()
                    .map(|event| (event.oid.clone(), event))
                    .collect()
            })
            .unwrap_or_default();
        self.events = Some(events);
    }

    pub fn event(&self, oid: &str) -> Option<&CommitEvent> {
        self.events.as_ref()?.get(oid)
    }

    pub fn put_event(&mut self, event: CommitEvent) {
        if !self.enabled {
            return;
        }
        self.events
            .get_or_insert_with(HashMap::new)
            .insert(event.oid.clone(), event);
        self.events_dirty = true;
    }

    pub fn save(&mut self) {
        if !self.enabled {
            return;
        }
        if self.events_dirty {
            let mut events = self
                .events
                .as_ref()
                .map(|events| events.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            events.sort_by(|a, b| a.oid.cmp(&b.oid));
            let data = EventsData {
                version: CACHE_VERSION.to_string(),
                history_state: self.history_state.clone(),
                events,
            };
            if save_json(&self.events_path, &data, self.max_cache_bytes).is_ok() {
                self.events_dirty = false;
            }
        }
        if self.views_dirty {
            let data = ViewsData {
                version: CACHE_VERSION.to_string(),
                views: self.views.clone(),
            };
            if save_json(&self.views_path, &data, self.max_cache_bytes).is_ok() {
                self.views_dirty = false;
            }
        }
    }
}

pub(super) fn cache_directory(root: &Path) -> Option<PathBuf> {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let dirs = ProjectDirs::from("", "", "reposcout")?;
    let encoded = encode_path(&canonical);
    let id = xxh3_64(encoded.as_bytes());
    Some(dirs.cache_dir().join("churn").join(format!("{id:016x}")))
}

fn load_json<T: for<'de> Deserialize<'de>>(path: &Path, max_bytes: u64) -> Option<T> {
    let bytes = match fs_budget::read_bytes_limited(path, max_bytes) {
        Ok(bytes) => bytes,
        Err(ReadOutcome::NotRegularFile | ReadOutcome::Oversized(_) | ReadOutcome::Unreadable) => {
            return None;
        }
        Err(_) => return None,
    };
    serde_json::from_slice(&bytes).ok()
}

fn save_json(path: &Path, value: &impl Serialize, max_bytes: u64) -> std::io::Result<()> {
    let encoded = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    if encoded.len() as u64 > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "churn cache payload exceeds configured size limit",
        ));
    }
    fs_budget::write_atomic_bytes(path, &encoded)
}

pub(super) fn encode_path(path: &Path) -> String {
    encode_os_str(path.as_os_str())
}

#[cfg(unix)]
fn encode_os_str(value: &OsStr) -> String {
    use std::os::unix::ffi::OsStrExt;
    encode_bytes(value.as_bytes())
}

#[cfg(unix)]
fn decode_os_string(value: &str) -> Result<OsString, String> {
    use std::os::unix::ffi::OsStringExt;
    decode_bytes(value).map(OsString::from_vec)
}

#[cfg(windows)]
fn encode_os_str(value: &OsStr) -> String {
    use std::os::windows::ffi::OsStrExt;
    let bytes = value
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    encode_bytes(&bytes)
}

#[cfg(windows)]
fn decode_os_string(value: &str) -> Result<OsString, String> {
    use std::os::windows::ffi::OsStringExt;
    let bytes = decode_bytes(value)?;
    if !bytes.len().is_multiple_of(2) {
        return Err("cached Windows path has an odd byte count".to_string());
    }
    let wide = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    Ok(OsString::from_wide(&wide))
}

#[cfg(not(any(unix, windows)))]
fn encode_os_str(value: &OsStr) -> String {
    encode_bytes(value.to_string_lossy().as_bytes())
}

#[cfg(not(any(unix, windows)))]
fn decode_os_string(value: &str) -> Result<OsString, String> {
    String::from_utf8(decode_bytes(value)?)
        .map(OsString::from)
        .map_err(|error| error.to_string())
}

fn encode_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_bytes(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err("cached path has an odd hex length".to_string());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = decode_nibble(pair[0])?;
            let low = decode_nibble(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn decode_nibble(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err("cached path contains non-hex data".to_string()),
    }
}

mod path {
    use super::{Path, PathBuf, decode_os_string, encode_path};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Path, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&encode_path(value))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        decode_os_string(&encoded)
            .map(PathBuf::from)
            .map_err(serde::de::Error::custom)
    }
}

mod optional_path {
    use super::{PathBuf, decode_os_string, encode_path};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Option<PathBuf>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.as_deref().map(encode_path).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer)?
            .map(|encoded| decode_os_string(&encoded).map(PathBuf::from))
            .transpose()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_os_string, encode_path};
    use std::path::PathBuf;

    #[cfg(unix)]
    #[test]
    fn cached_paths_round_trip_non_utf8_bytes() {
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(std::ffi::OsString::from_vec(vec![
            b's', b'r', b'c', b'/', 0xff, b'.', b'r', b's',
        ]));
        let encoded = encode_path(&path);
        let decoded = PathBuf::from(decode_os_string(&encoded).unwrap());

        assert_eq!(decoded, path);
    }
}
