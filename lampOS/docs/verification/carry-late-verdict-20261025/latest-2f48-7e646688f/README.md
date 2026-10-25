# Full train 114 Voice/Eval coverage

{
  "head": "7e646688f38ed435a65cda02e022a7fca13ba144",
  "tree": "7222d924326848a871f88dad35d8e96312f15cda",
  "base": "2f48b0edd24c07c0fda0e67c8985addc8a40cdf2",
  "full_coverage": {
    "head": "7e646688f38ed435a65cda02e022a7fca13ba144",
    "base": "2f48b0edd24c07c0fda0e67c8985addc8a40cdf2",
    "selection": "All default Voice/Eval test targets: two disjoint Voice-library selections plus Voice bin/integration/doc/test-enabled-example targets and full Eval. Distinct invocations retained; no repeated library test.",
    "passed": 4657,
    "failed": 0,
    "ignored": 21,
    "summaries": 12,
    "source_unchanged": true,
    "disjoint_complete_library_coverage": true,
    "example_targets_included": [
      "render_line_second",
      "stop_words_stt_replay"
    ]
  },
  "source_count": 7041
}

The initial scoped gate passed fmt, strict all-target Voice/Eval Clippy, workspace build and its selected tests. A separate complementary library invocation skips every original library filter; both selected/filtered counts reconcile exactly, with no repeated library test. Target inventory then identified two test-enabled examples omitted from the original bins/integration/doc selection: each ran once and passed. Twelve actual summaries cover all default Voice/Eval test targets. Earlier nine-summary scoped metadata remains exact, not rewritten as the combined run. There was no changed-source test failure or hidden retry in this gate chain. The separate P3 diagnostic retains its setup failure, natural non-reproduction and explicit-continuation old-source failure.

All frozen source records were checked against committed bytes/modes, and every stage/completion source guard passed. Seven initial artifact records plus the library complement and two examples give nine unique test executables in the correct exclusive fixed-worktree target. The complement library digest matches the initial selection; actual Cargo fresh flags, profiles and executable/dep-info hashes are retained. These are producer post-compile records, not independent private-byte or pre-exec attestation. Same-worktree reuse is allowed; there is no all-fresh or cross-worktree target claim.

The full authored patch against public base2f reconstructs the exact executed Git tree without relying on an unreachable local commit. The next carry changes exactly the Cloud product unit and its switches row; all crate/test/manifest bytes stay equal. Its affected census/current fmt/Clippy/build qualification and native runs remain a separate source epoch. This completed full test execution is not renamed latest-base execution.

One compiler job per worktree, two lanes together under the recorded four-job cap; no isolated-host performance claim. No local ARM64 execution/linking, device, calibration, physical latency or activation. Earlier failed native and reported reader Pet/SIGTERM failures retain their scopes and limits.
