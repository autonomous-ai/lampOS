# Retry a reminder after an admitted interruption

Kind: changes every lamp's default. No new switch or wire format.

The build 6919 lamp-4ace replay reported a reminder left held after the person
cut it off. An admitted user interruption now retains its exact receipt until
the old owner is revoked, queued output is cleared and the successor or revoked
authority publishes successfully. The existing OwnerFenced retirement then
permits another attempt only before any qualified own generation retirement.
An existing own-retirement cursor stays held, including a later unfinished
generation after an earlier one retired. It does not complete or acknowledge
the reminder; unretired partial words may repeat. Known provider failure, invalidated playback clocks,
privacy closure, unexplained drops and failed publication keep their existing
held outcome. Source selection, five-second pacing and whole-output Ack remain.

## Review correction: own retired audio stays held

Review of the original 61a4e1774 offer found that a reminder with a retired
final generation but no provider terminal join could retry after a talk-over.
Its green original gates do not waive that product fault. A new live control
fails 0/1 on that offered production at the intended retry/Abandon boundary.

The correction excludes any qualified own-retirement cursor from retry and
leaves it held without Ack. This conservatively includes a later unfinished
generation after an earlier one retired; the cursor alone does not prove
whole-provider completion. No-audio and audio-before-any-own-retirement
interruptions still retry after the same fences. A receipt control pins both
retired-only and retired-then-new-audio holds. Prior receipt files below retain
their original source identities; the review correction is qualified separately.

## Corrected qualification on current product inputs

`review-negative.json` and `review-negative.patch` retain the intended **0/1**
NoTerminal failure on original offered 61a4e1774. Its test executable is kept.
The corrected overlay on 41233 plus the separate fixture dependency passes
formatting, strict Voice/Voice-eval all-target Clippy, workspace build, full
Voice **4,080/0/21** and Schedule **214/0/7**. All nine own controls pass once
(`review-fixed.json`). Those full results retain their actual source identity.

The carry onto train153 0c1f6437e drops the now-landed fixture dependency.
Fresh formatting, strict Voice/Voice-eval Clippy, the complete reminder-focused
library suite **110/0/2**, and workspace build pass (`current-0c1.json`). All
nine own controls pass once again, with actual executable and source hashes
preserved. `carry-0c1.json` lists changed inherited inputs; this focused carry
is not relabelled a full Voice/Schedule run. The PR requires its fresh original
native ARM64 and Linux Clippy gate and a reread of the corrected policy.

## Initial qualification before the own-retirement correction

The original unchanged-production control on 3d9722f38 fails 0/1 at the intended
missing-retirement assertion. `negative.patch` is that exact test overlay;
`negative.json` binds its executable, raw log and unchanged source manifest.
The original executable and raw logs remain in the builder's private evidence.

An earlier full run on d21 failed: 3,787 passed, two existing body-push/search
host-watchdog failures, 15 ignored. Schedule did not run. Its result is kept in
`earlier-failed-full.json`; it is not reclassified as passing. Separate test-only
PR #3520 corrects those fixtures' excessive publication cadence and records its
own negative and fixed observations. No product timeout is widened here.

On the frozen 2a353bfdb fixture dependency plus this exact runtime patch:

- `cargo +1.96.0 fmt --all --check`: pass.
- `cargo +1.96.0 clippy -q --message-format=short --offline --locked -p lamp-voice -p lamp-voice-eval --all-targets -- -D warnings`: pass.
- `cargo +1.96.0 build -q --offline --locked --workspace`: pass.
- `cargo +1.96.0 test --offline --locked -p lamp-voice -- --test-threads=1`:
  **4,078 passed, zero failed, 21 ignored** across seven harnesses.
- `cargo +1.96.0 test --offline --locked -p lamp-schedule -- --test-threads=1`:
  **214 passed, zero failed, seven ignored** across eight harnesses.

`fixed.json` binds all 7,164 source hashes, the patch, actual preserved test
executables, commands, terminal results and seven new controls each passing
once. Tests ran serially on native x86_64 macOS with one compiler job.
The historical c5 carry used fixture head 1fa9ae1b9; only docs/build-rows.md differs
from that qualified overlay. Every other input and the entire runtime patch
is identical (`current-c5-carry.json`). Historical counts keep their actual
source identity. Those historical gates do not waive the duplicate-speech review finding.

## Scope and latency

The paths add no I/O, network call or timer to input admission. Settlement uses
the existing consumer queue after authority publication. Fresh eligibility and
five-second attempt pacing remain prerequisites for later delivery. Synthetic
controls establish ownership and durable token behavior, not acoustic latency,
physical interruption or played-word completion. First owns the lamp replay.

No face, person, Memory or routing fields change. Stop/privacy and stale-source
revocation remain independent. No new topic, switch, unit or dependency needs
a census entry. The separate positive-pending Skills trigger fault is not
included in this runtime repair.
