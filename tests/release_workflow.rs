#![allow(
    clippy::panic,
    reason = "workflow security assertions should fail immediately with the offending invariant"
)]

use std::{error::Error, fs, process::Command};

const RELEASE_WORKFLOW: &str = include_str!("../.github/workflows/release.yml");
const CHECKSUM_NORMALIZER_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/.github/scripts/normalize-cargo-dist-checksums.py"
);

#[test]
fn release_tags_are_passed_to_shell_steps_only_as_environment_data() {
    for unsafe_interpolation in [
        "format('host --steps=create --tag={0}', github.ref_name)",
        "${{ needs.plan.outputs.tag-flag }}",
        "\"${{ needs.plan.outputs.tag }}\"",
    ] {
        assert!(
            !RELEASE_WORKFLOW.contains(unsafe_interpolation),
            "release workflow still embeds `{unsafe_interpolation}` in shell source"
        );
    }

    assert!(RELEASE_WORKFLOW.contains("RELEASE_TAG: ${{ github.ref_name }}"));
    assert!(RELEASE_WORKFLOW.contains("--tag=\"$RELEASE_TAG\""));
}

#[test]
fn release_publication_requires_a_valid_main_commit_and_existing_tag() {
    assert!(RELEASE_WORKFLOW.contains("git merge-base --is-ancestor"));
    assert!(RELEASE_WORKFLOW.contains("refs/remotes/origin/main"));
    assert!(RELEASE_WORKFLOW.contains("--verify-tag"));
    assert!(RELEASE_WORKFLOW.contains("    environment: release\n"));
}

#[test]
fn release_checksums_are_normalized_and_verified_before_upload() {
    assert!(RELEASE_WORKFLOW.contains("normalize-cargo-dist-checksums.py"));
    assert!(RELEASE_WORKFLOW.contains("target/distrib/source.tar.gz.sha256"));
    assert!(RELEASE_WORKFLOW.contains("target/distrib/sha256.sum"));
    assert!(RELEASE_WORKFLOW.contains("shasum -a 256 --check source.tar.gz.sha256 sha256.sum"));
}

#[test]
fn cargo_dist_checksum_normalizer_keeps_exactly_one_final_newline() -> Result<(), Box<dyn Error>> {
    const FIRST_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const SECOND_HASH: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

    let temporary = tempfile::tempdir()?;
    let source_checksum = temporary.path().join("source.tar.gz.sha256");
    let unified_checksum = temporary.path().join("sha256.sum");
    let already_normalized = temporary.path().join("fixed.sha256");
    let source_expected = format!("{FIRST_HASH} *source.tar.gz\n");
    let unified_expected =
        format!("{FIRST_HASH} *source.tar.gz\n{SECOND_HASH} *reposcout.tar.xz\n");
    let fixed_expected = format!("{SECOND_HASH} *fixed.tar.xz\n");

    fs::write(&source_checksum, format!("{source_expected}\n"))?;
    fs::write(&unified_checksum, format!("{unified_expected}\n"))?;
    fs::write(&already_normalized, &fixed_expected)?;

    for _ in 0..2 {
        let output = Command::new("python3")
            .arg(CHECKSUM_NORMALIZER_PATH)
            .args([&source_checksum, &unified_checksum, &already_normalized])
            .output()?;
        assert!(
            output.status.success(),
            "checksum normalizer failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_to_string(&source_checksum)?, source_expected);
        assert_eq!(fs::read_to_string(&unified_checksum)?, unified_expected);
        assert_eq!(fs::read_to_string(&already_normalized)?, fixed_expected);
    }

    Ok(())
}

#[test]
fn cargo_dist_checksum_normalizer_rejects_internal_blank_lines() -> Result<(), Box<dyn Error>> {
    const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    let temporary = tempfile::tempdir()?;
    let malformed = temporary.path().join("malformed.sha256");
    fs::write(
        &malformed,
        format!("{HASH} *first.tar.xz\n\n{HASH} *second.tar.xz\n"),
    )?;

    let output = Command::new("python3")
        .arg(CHECKSUM_NORMALIZER_PATH)
        .arg(malformed)
        .output()?;

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected blank line"));
    Ok(())
}
