# V2 input follow-up: ARM64 attempt 2 and separate checks

Audit prepared 26 October 2026, 02:29 +07 for [PR #3503](https://github.com/autonomous-ai/lampOS/pull/3503),
unchanged head `d8cf817d6f6a6beb53fe10647683d6d9e94dd7a7`. The explicitly requested,
bounded [ARM64 attempt 2](https://github.com/autonomous-ai/lampOS/actions/runs/40224140937/attempts/2)
succeeded. No test was run by this evidence audit.

| Scope | Result | Public run |
| --- | --- | --- |
| ARM64 nextest, attempt 2 | 11,308 PASS; zero LEAK/FAIL; 106 skipped; zero selected unrun | [40224140937/2](https://github.com/autonomous-ai/lampOS/actions/runs/40224140937/attempts/2) |
| Later doc/custom-harness step, attempt 2 | Success: 4 doctests across 46 summaries; relay harness reports ok; process harness 12/0 | [ARM64 job](https://github.com/autonomous-ai/lampOS/actions/runs/40224140937/job/121265228357) |
| Own Ubuntu 24.04 imager, attempt 1 | 31 signature, 5 bootstrap, 8 output, 5 input checks; 31 pytest tests passed | [imager job](https://github.com/autonomous-ai/lampOS/actions/runs/40224140951/job/121261831403) |
| Linux Clippy, every crate and target, attempt 1 | Success | [40224140931](https://github.com/autonomous-ai/lampOS/actions/runs/40224140931) |

## Exact source and census

ARM64 attempt 2 and the imager checked out
`fdda2e3eecfa647460696759afa505c29e0c686c`, merging the head into
`a6ccb1aa3d8d27be64b9030dc814fc2f44e43fdb`. The head and checkout share complete tree
`dc94a4d8faf269ffcbca2e2a7ec8431256965da2`. Tree identity comes from immutable Git metadata;
it is not a claim that the runtime printed a source-hash guard.

The single ARM64 Starting-to-Summary window contains 11,308 unique binary/test
pairs and every ordinal 1 through 11,308 exactly once. All are literal PASS;
35 later PASS rows echo already-counted cases and are excluded. Nextest
announces 179 binaries, skips 106 tests and two binaries, and has terminal rows
from 153 distinct binary labels. The later doc/custom tests are additional,
separately reported executions, not part of the 11,308 nextest count.

`session_reader::tests::linux::failed_private_rollback_disables_only_metadata`
is literal PASS at ordinal 10,000 in this attempt. The doc/custom step also has
success metadata, matching its literal doctest, relay and process outputs.
The imager results reuse the existing completed-job audit; Clippy uses completed
run/job metadata. Neither full check was downloaded or executed again here.

## Attempt 1 remains a failure

[Attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40224140937/attempts/1)
on the same head and checkout remains **10,003 PASS, one FAIL, 106 skipped and
1,304 selected cases unrun**. Its failed case and log hash are retained in the
census, alongside the checksum of the separate failure packet.

The source audit found that the fixture does not witness reaching its intended
grant-step-12 injection. Its observed verified-private fallback differs from
the incomplete rollback the assertion expected. The exact earlier refusal
remains unknown; a shared ancestor change during ACL inspection was a possible
mechanism, not an observed cause. The retry's success does not diagnose it or
constitute a code repair.

[Tests owns #1806](https://github.com/autonomous-ai/lampOS/issues/1806), as recorded
in the [current review](https://github.com/autonomous-ai/lampOS/pull/3503#issuecomment-6401991844).
The [prior source report](https://github.com/autonomous-ai/lampOS/pull/3394#issuecomment-6386376127)
and [explicit retry request](https://github.com/autonomous-ai/lampOS/pull/3503#issuecomment-6401994268)
remain linked. No source was changed for this retry.

[census.json](census.json) holds exact identities, run/step conclusions, counts
and log SHA-256 values. [selected-results.txt](selected-results.txt) preserves
literal rows with GitHub timestamps; only repeated job/step prefixes are removed.
The bootstrap summary containing a temporary path is omitted in favor of all
five case outcomes. [SHA256SUMS](SHA256SUMS) binds these three public files.
No raw full logs, machine paths, process identifiers or signing material are
included. These headless results do not qualify a real image, flash, first boot,
lamp response latency, attended power cut or signed-release run.
