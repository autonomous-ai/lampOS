# Original #3427 native CI, no rerun

Authored `6620b73f31e9a8e31b5cd5bb6d3f6e767182f9fb` on `162216f82db66e642e0c8e3033a9866cfec1ce1f`; actual tested preview `6ef40512dbf21eacb50d5c4c2331a77e1dfa008f`. Whole authored/tested trees equal `29a05d23cb5cc8c03103a953137f44c0cbd0422a`.

Original ARM run 40147215375, job 121031016480, succeeds: 7,407 PASS plus one LEAK, all 7,408 selected cases terminal, 53 tests skipped, zero failed or unrun. All 21 supervisor cases are literal PASS, including changed ownership, cooperative stop and missing-inhibit controls. The LEAK remains explicitly listed in the receipt. The parser collapses only 34 identical echoed terminal lines; no status is renamed. This is the workflow's selected suite, not a full-workspace test-count claim.

Original Linux run 40147215382, job 121031016287, passes pinned Rust 1.96 whole-workspace formatting and strict all-target Clippy. Original final-job metadata and selected lines are retained. Full log hashes identify `gh run view --log` renderings, not raw API or Actions ZIP bytes; the raw complete renderings and all terminal rows remain privately retained, and the original public runs are linked by ID.

This adds no local ARM64 execution, CI rerun, device action, timing or old-failure waiver. The earlier unqualified local epoch stays unchanged; the separate qualified Mac proof is in `fresh-162`. Author readiness and independent reader dispositions remain separate from this technical result.
