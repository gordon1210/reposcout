use super::super::support::Fixture;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

/// The driver can execute the public CLI, but cannot inspect or mutate its fixture directly.
pub(crate) struct Journey<'fixture> {
    fixture: &'fixture Fixture,
    transcript: PathBuf,
    steps: usize,
    step_limit: Option<usize>,
}

/// An observed process result, without a dependency on internal report models.
pub(crate) struct Step {
    label: String,
    prefix: PathBuf,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit: Option<i32>,
    elapsed_seconds: f64,
}

impl<'fixture> Journey<'fixture> {
    pub(crate) fn new(fixture: &'fixture Fixture) -> Self {
        // The outer fixture owns cleanup, including optional retention during assertion failure.
        let transcript = tempfile::Builder::new()
            .prefix("journey-")
            .tempdir_in(fixture.state_path())
            .expect("private journey transcript directory")
            .keep();
        eprintln!("[journey] transcript: {}", transcript.display());
        Self {
            fixture,
            transcript,
            steps: 0,
            step_limit: None,
        }
    }

    pub(crate) fn bounded(fixture: &'fixture Fixture, step_limit: usize) -> Self {
        Self {
            step_limit: Some(step_limit),
            ..Self::new(fixture)
        }
    }

    /// Arguments are passed unchanged; format, profile, tokenization and budgets remain explicit.
    pub(crate) fn step(&mut self, label: &str, args: &[&str], expected_exit: i32) -> Step {
        let step = self.execute(label, args, Some(expected_exit));
        step.assert_exit(expected_exit);
        step
    }

    /// Returns every process outcome so callers can account for errors before asserting status.
    pub(crate) fn observe(&mut self, label: &str, args: &[&str]) -> Step {
        self.execute(label, args, None)
    }

    fn execute(&mut self, label: &str, args: &[&str], expected_exit: Option<i32>) -> Step {
        assert!(
            self.step_limit.is_none_or(|limit| self.steps < limit),
            "CLI route exceeded its invocation safeguard before {label:?}"
        );
        self.steps += 1;
        let prefix = self.transcript.join(format!("{:03}", self.steps));
        let command = json!({
            "label": label,
            "arguments": args,
            "expected_exit": expected_exit,
        });
        fs::write(
            prefix.with_extension("command.json"),
            serde_json::to_vec_pretty(&command).unwrap(),
        )
        .expect("record journey command before execution");
        eprintln!("[journey step {}: {label}] reposcout {args:?}", self.steps);

        let started = Instant::now();
        let output = self.fixture.command(args).output().unwrap_or_else(|error| {
            panic!(
                "journey step {label:?} could not execute: {error}; transcript {}",
                prefix.display()
            )
        });
        let elapsed_seconds = started.elapsed().as_secs_f64();
        fs::write(prefix.with_extension("stdout"), &output.stdout).expect("record journey stdout");
        fs::write(prefix.with_extension("stderr"), &output.stderr).expect("record journey stderr");
        let result = json!({
            "exit": output.status.code(),
            "status": output.status.to_string(),
            "elapsed_seconds": elapsed_seconds,
            "stdout_bytes": output.stdout.len(),
            "stderr_bytes": output.stderr.len(),
        });
        fs::write(
            prefix.with_extension("result.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .expect("record journey result");

        Step {
            label: label.to_owned(),
            prefix,
            stdout: output.stdout,
            stderr: output.stderr,
            exit: output.status.code(),
            elapsed_seconds,
        }
    }
}

impl Step {
    pub(crate) fn assert_exit(&self, expected_exit: i32) {
        assert_eq!(
            self.exit,
            Some(expected_exit),
            "journey step {:?}; transcript {}; stdout: {}; stderr: {}",
            self.label,
            self.prefix.display(),
            preview(&self.stdout),
            preview(&self.stderr)
        );
    }

    pub(crate) fn exit_code(&self) -> Option<i32> {
        self.exit
    }

    pub(crate) fn stderr_bytes(&self) -> &[u8] {
        &self.stderr
    }

    pub(crate) fn elapsed_seconds(&self) -> f64 {
        self.elapsed_seconds
    }

    pub(crate) fn transcript_prefix(&self) -> &std::path::Path {
        &self.prefix
    }

    pub(crate) fn stdout_json(&self) -> Value {
        self.parse_json("stdout", &self.stdout)
    }

    pub(crate) fn stdout_bytes(&self) -> &[u8] {
        &self.stdout
    }

    fn parse_json(&self, stream: &str, bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).unwrap_or_else(|error| {
            panic!(
                "journey step {:?} returned invalid {stream} JSON: {error}; transcript {}; {}",
                self.label,
                self.prefix.with_extension(stream).display(),
                preview(bytes)
            )
        })
    }
}

fn preview(bytes: &[u8]) -> String {
    const LIMIT: usize = 4_096;
    let shown = String::from_utf8_lossy(&bytes[..bytes.len().min(LIMIT)]);
    if bytes.len() > LIMIT {
        format!("{shown} … ({} bytes total)", bytes.len())
    } else {
        shown.into_owned()
    }
}
