use assert_cmd::Command;
use std::path::PathBuf;

pub fn test_global_config() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/test-global.toml")
}

pub fn reposcout_command() -> Command {
    let mut command = Command::cargo_bin("reposcout").expect("compiled reposcout binary");
    command.env("REPOSCOUT_GLOBAL_CONFIG", test_global_config());
    command
}
