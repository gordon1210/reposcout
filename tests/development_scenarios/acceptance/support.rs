pub(super) use crate::journeys::support::Journey;
pub(super) use crate::support::Fixture;

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::Duration;

/// Executes only the authored miniature application, independently of `RepoScout`.
pub(super) fn probe(fixture: &Fixture, script: &str) -> Value {
    let program = format!("import sys\nsys.path.insert(0, sys.argv[1])\n{script}");
    let transcript = tempfile::Builder::new()
        .prefix("truth-probe-")
        .tempdir_in(fixture.state_path())
        .unwrap()
        .keep();
    fs::write(transcript.join("probe.py"), &program).unwrap();
    let output = Command::new("python3")
        .args(["-I", "-B", "-c", &program])
        .arg(fixture.path())
        .current_dir(fixture.path())
        .timeout(Duration::from_secs(10))
        .output()
        .expect("Python 3 is required for independent acceptance fixture probes");
    fs::write(transcript.join("stdout"), &output.stdout).unwrap();
    fs::write(transcript.join("stderr"), &output.stderr).unwrap();
    fs::write(transcript.join("status"), output.status.to_string()).unwrap();
    eprintln!(
        "[independent truth] {}: {}",
        transcript.display(),
        String::from_utf8_lossy(&output.stdout).trim()
    );
    assert!(
        output.status.success(),
        "independent application probe failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("fixture probe returns one JSON value")
}

/// Counts fixture source/data only; Git objects and private transcripts are not application input.
pub(super) fn assert_fixture_bounds(fixture: &Fixture) {
    fn count(path: &Path) -> (usize, u64) {
        let mut totals = (0, 0);
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == ".git" {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path()).unwrap();
            assert!(!metadata.file_type().is_symlink());
            if metadata.is_dir() {
                let nested = count(&entry.path());
                totals.0 += nested.0;
                totals.1 += nested.1;
            } else {
                assert!(metadata.is_file());
                totals.0 += 1;
                totals.1 += metadata.len();
            }
        }
        totals
    }
    let (files, bytes) = count(fixture.path());
    assert!(files <= 20, "fixture has {files} files, limit 20");
    assert!(
        bytes <= 48 * 1024,
        "fixture has {bytes} bytes, limit 48 KiB"
    );
}
