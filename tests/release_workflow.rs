#![allow(
    clippy::panic,
    reason = "workflow security assertions should fail immediately with the offending invariant"
)]

const RELEASE_WORKFLOW: &str = include_str!("../.github/workflows/release.yml");

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
