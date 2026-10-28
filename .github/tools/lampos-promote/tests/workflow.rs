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

/// The stable notes step (#3976) reads the receipt this tool prints, and it
/// can't fail the job (`continue-on-error`): a field renamed here would leave
/// the notes unwritten with only a log line. Every key the script takes from
/// `receipt` is one the receipt has.
#[test]
fn the_stable_notes_read_only_fields_the_receipt_has() {
    let receipt = serde_json::to_value(lampos_promote::Receipt {
        outcome: "promoted",
        sequence: 8389,
        commit: "c".repeat(40),
        source_run: 1,
        source_attempt: 1,
        promotion_run: 2,
        promotion_attempt: 1,
        previous_asset: Some(3),
        stable_asset: 4,
        envelope_sha256: "0".repeat(64),
    })
    .unwrap();
    let script = include_str!("../../../scripts/lampos-stable-notes.py");
    let mut read = std::collections::BTreeSet::new();
    for opening in ["receipt[\"", "receipt['", "receipt.get(\"", "receipt.get('"] {
        for (at, _) in script.match_indices(opening) {
            let rest = &script[at + opening.len()..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '_')
                .collect();
            read.insert(name);
        }
    }
    assert!(
        read.contains("previous_asset") && read.contains("commit"),
        "{read:?}"
    );
    for name in &read {
        assert!(
            receipt.get(name).is_some(),
            "the notes read {name}, which the receipt lacks"
        );
    }
}
