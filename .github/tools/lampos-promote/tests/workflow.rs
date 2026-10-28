//! Deployment boundaries stay in the registered workflow; this tool has no signer.
#[test]
fn promotion_is_manual_separate_and_branch_protected() {
    let workflow = include_str!("../../../workflows/lampos-arm64.yml");
    // The ARM64 job never runs for a promotion. It needs the runner choice
    // (route), which is skipped on lamp-voice's pushes, so it and publish
    // check results themselves instead of the implicit success().
    let arm64 = workflow
        .split("\n  arm64:\n")
        .nth(1)
        .unwrap()
        .split("\n    steps:\n")
        .next()
        .unwrap();
    assert!(arm64.contains("needs: route\n"));
    assert!(arm64.contains("if: ${{ !cancelled() && !inputs.promote_sequence }}"));
    let publish = workflow
        .split("\n  publish:\n")
        .nth(1)
        .unwrap()
        .split("\n    steps:\n")
        .next()
        .unwrap();
    assert!(publish.contains("if: ${{ !cancelled() && needs.arm64.result == 'success' && github.event_name == 'push' && github.ref == 'refs/heads/lamp-voice'"));
    // The Studio may build lamp-voice's artifact; the signing key stays on GitHub-hosted.
    assert!(publish.contains("    runs-on: ubuntu-22.04-arm\n"));
    let promotion = workflow
        .split("  promote-stable:\n")
        .nth(1)
        .unwrap()
        .split("  report-full-run-failure:\n")
        .next()
        .unwrap();
    for boundary in [
        "github.event_name == 'workflow_dispatch'",
        "inputs.promote_sequence != ''",
        "github.ref == 'refs/heads/lamp-voice'",
        "github.repository == 'autonomous-ai/lampOS'",
        "vars.LAMPOS_STABLE_PROMOTION == 'on'",
        "environment: lampos-update-publish",
        "group: lampos-promote-stable",
        "cancel-in-progress: false",
        "contents: write",
        "actions: read",
        "LAMPOS_UPDATE_PUBLIC_KEY",
        "PROMOTE_SEQUENCE: ${{ inputs.promote_sequence }}",
        "test \"$PROMOTE_FULL\" = false",
        "--sequence \"$PROMOTE_SEQUENCE\"",
    ] {
        assert!(promotion.contains(boundary), "missing {boundary}");
    }
    assert!(!promotion.contains("secrets."));
    assert!(!promotion.contains("LAMPOS_UPDATE_KEY:"));
    assert!(!promotion.contains("needs: arm64"));
    assert_eq!(
        workflow
            .matches("'.github/tools/lampos-promote/**'")
            .count(),
        2
    );
    let clippy = include_str!("../../../workflows/lampos-clippy.yml");
    assert_eq!(
        clippy.matches("'.github/tools/lampos-promote/**'").count(),
        2
    );
    assert!(clippy.contains("cargo clippy --locked --manifest-path .github/tools/lampos-promote/Cargo.toml --all-targets -- -D warnings"));
}
