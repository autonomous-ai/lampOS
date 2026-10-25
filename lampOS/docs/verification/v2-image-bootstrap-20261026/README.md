# V2 image bootstrap source qualification

26 October 2026. Carry of [#3246](https://github.com/autonomous-ai/lampOS/pull/3246)
for [#2538](https://github.com/autonomous-ai/lampOS/issues/2538), based on
`abc79d609126e7c1b83dedb220b7be00b7e7ab6a`. The original two commits remain
patch-identical (`git range-diff` reports `=` for both). The added repair
verifies the exact signed manifest before any bootstrap download/execution,
uses the same staged envelope for both install attempts, checks the recorded
build, and selects output by this invocation's compression mode.

## Commands and observations

| Command, from repository root unless stated | Mac result |
| --- | --- |
| `bash scripts/imager/tests/test_lampos_envelope.sh` | 31 passed, 0 failed |
| `bash scripts/imager/tests/test_lampos_image_bootstrap.sh` | 5 current behavioral observations passed |
| `MAKE=<Command Line Tools make> sh scripts/imager/tests/test_v2_image_output.sh` | 8 passed, 0 failed |
| `python -m pytest scripts/imager/tests -q -p no:cacheprovider --basetemp <owned temporary directory>` | 30 passed, 1 skipped |
| `cargo +1.96.0 fmt --all --check`, from `lampOS` | passed |
| `bash -n` separately for the builder, verifier and both Bash suites; `sh -n` for the output suite | passed |
| Ruby `YAML.load_file` and presence of the `imager` job; `git diff --check` | passed |

The skipped test requires Linux and e2fsprogs and resizes a disposable file.
The new Ubuntu 24.04 CI job runs it, all three shell suites and the existing
image tests. No Rust source changed, so there is no changed-crate local
Clippy/test command; the PR's original Linux and ARM64 workflows remain
separate required evidence.

The full source gate ran on the `14c64a8db` carry. All 21 image inputs were
unchanged during it and through the carry to `abc79d609`; fresh formatting
also passed on that base. An independent source read then found that the
bootstrap fixtures used `bytes` instead of required `size` and omitted
`units`. The fixture was corrected to the current installer schema, and its
suite alone reran with
`LAMPOS_BOOTSTRAP_BASELINE_REF=c6bb89c611f58d806f3e0103753691f0aff66b96`:
**10 behavioral observations passed**, comprising five current cases and
five historical controls. The unchanged helper, output and existing-test
results are not counted as additional executions.

The historical controls reach the original unsigned-bootstrap execution
boundary, install a later remote manifest on both attempts, and accept a
mismatched final record. The fixed cases refuse the attacker before fetching
its payload, keep both attempts on the staged build, and reject the wrong
record. Both versions also reject a changed payload and accept the local
channel fixture. A separate original Makefile control reproduced the stale
compressed-output rename. These are command-mocked source observations;
downloaded fixture bytes are never executed.

## Retained evidence and limits

[inputs.sha256](inputs.sha256) binds the final 21 image inputs, including the
corrected bootstrap suite. [test-results.txt](test-results.txt) contains
selected literal result rows from the full gate and the corrected-suite run;
only temporary paths are removed. The raw logs and ephemeral-key fixture
roots remain private. Earlier development/harness failures and the original
schema-inaccurate fixture run are retained separately and are not treated as
negative security results. [SHA256SUMS](SHA256SUMS) binds this public packet.

The verifier authenticates exact bytes and rejects ambiguous envelopes/JSON;
it does not replace the installer's schema, inventory or release policy.
Keys are generated for each test. No production signing key, Docker image
build, disk mount, flash, lamp, first boot, latency or release acceptance was
used or qualified. The original PR's test-key image build belongs to its
original head and is not evidence for this carry. Native CI is pending at
publication.
