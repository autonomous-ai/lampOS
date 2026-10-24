# Guarded enrollment: the speaker’s own name

Third’s follow-up to #3318’s name-ownership P2 and guarded replacement-prompt
P3. The branch is `third/remember-own-name`, based on landed `9ed44f88c`.
These are Mac software receipts. No ARM64 link on the Mac, lamp action,
classifier-accuracy result or physical-latency claim.

## Reproductions and focused correction

- `reported-findings-before.txt`: before implementation on `8b42bb15f`,
  **2 passed / 3 failed**. A real Unix socket receives the third-person and
  mixed-name requests. The guarded known_as response also exposes the stored
  name and requests an unsupported replacement confirmation.
- `before-final-audit-23.txt`: first final focused run **23/0**, including
  the original 16 enrollment controls plus seven new controls.
- Independent final source review found greetings/polite wording could still
  become a bare name after stripping. `vocative-before.txt` reproduces all
  seven cases against a real socket (**0/1**). The fix requires an explicit
  introduction after any preface, enrollment request or polite suffix.
- `focused-final.txt`: corrected **23/0**, command from `lampOS/`:
  `CARGO_TARGET_DIR=/private/tmp/third-remember-voice-target CARGO_BUILD_JOBS=2 cargo test -q --locked --offline -p lamp-voice --lib remember_speaker -- --test-threads=1`.
  Source-only independent reread closed the vocative finding; its two source
  hashes match `source-before-full-gate.json`.
- The first whole-gate attempt was deliberately interrupted during Clippy
  after the vocative finding, before changing source. Formatting had passed.
  `interrupted-gate.json` retains that partial result; it is not a Clippy pass.
  The full gate was restarted on the corrected source.

## Timing boundary and limitation

`latency-debug.txt` measures raw name byte cap → trim → Said allocation →
name_heard → speaker_named_self, without sockets or proof construction, on
x86_64 macOS, Rust 1.96.0, debug profile. Ordinary input: p50 **92.7 µs**,
mean **93.6 µs**, max **469.6 µs** over 1,000 iterations. Maximum-size inputs
exceed the 1 ms design target: 4 KiB prose averages **3.69 ms**, and 4,079-byte
unique candidate/heard tokens average **9.55 ms**, max **9.83 ms** over 100.
Oversized raw name refusal averages **34 ns**. See all cases in the raw log.

This probe predates the final vocative/preface refusal, so it is not an
exact-final-build performance receipt. `latency-probe.patch` applied to
`8b42bb15f` reconstructs the measured known_faces.rs, including its temporary
probe; its hash is in the source receipt. No probe or timing assertion remains
in product tests. These are no release-profile or lamp latency guarantees.

## Physical and release state

The switch stays off. The [prepared physical sheet](../remember-speaking-face-20261024/physical-trial.md)
is unrun and belongs to First/the holder. Classifier false positives, missed
anchors at the two-second recognition cadence and actual acoustic latency
remain qualification limits. No score or product-unit increase is part of
this follow-up. The separate docs-only #3325 read retained in
`build-rows-review.json` verifies its arithmetic, not physical readiness;
Third held its unconditional bystander-safety wording on the PR.

Full local and native gate receipts will be recorded when each completes.

## Pushed head and completed local gate

PR [#3328](https://github.com/autonomous-ai/lampOS/pull/3328), head
`390532a53`, base `9ed44f88c`; exact five files, one-line commit. Formatting,
strict all-target Voice Clippy, full Voice **3,984/0/15 ignored**, and the
workspace build pass. The whole run’s source hashes remained unchanged and
were verified against the authored commit’s blobs (`local-gates.json`).
Raw full-suite output is `voice-full.txt`; the quiet successful build emits
no output (`workspace-build.txt`). All commands ran from `lampOS/`, with
`CARGO_BUILD_JOBS=2` and an isolated target directory.

The PR is held for two public privacy reads. Native ARM64/Clippy and exact
Linux formatting are requested. Those results are distinct from this local
pass. The source-only internal reread closed the final vocative finding;
no old test was rewritten to make the new guard pass.

## First public read and context-dependent bare names

Reviewer-three posted READ-OK `390532a53`: both original findings close,
with **63/0** focused tests including its 17-negative/8-positive probe and
strict Clippy reported. The independent whole-library run remains pending.
Its nonholding P3 is preserved: a bare name after a question about a friend
can be interpreted as the speaker’s name. The line-only guard cannot know
that preceding conversational context. The physical sheet now contrasts
self-name and friend-name prompts; no activation or general semantic
ownership guarantee is inferred from these host checks.

## Public reads, native pass and requested refusal fixtures

Both public readers approved implementation `390532a53` and closed the
original findings. ARM64 run 40057250479 passed **11,171 tests (one reported
Voice journal LEAK), 71 skipped**, process **12/0**; Linux Clippy 40057250486
passed. Exact Linux fmt passed per Second, with 6,967 files unchanged;
Third verified its repository/subtree bindings. See the implementation receipts.

Reviewer-three’s independent Mac library run with its probe reported
**3,700/4**, with each failing test passing twice alone. Those outcomes are
retained on #3328; isolated success does not prove the cause of failure.
Third’s full two-thread gate and native gate are separate passing evidence.

The lead requested explicit refusal fixtures. `7fb4feb20` adds five string
cases and documents the joined-name/addressee, bare-context and lost-question
limits: 17 added lines in two files, no runtime change. Current focused
**23/0**, formatting and strict Voice Clippy pass, with unchanged hashes.
The full local Voice/workspace passes are carried from `390532a53`, not
claimed rerun for this test/docs addition. Renewed public reads and native
checks are requested for the new head. The physical sheet now includes all
of the readers’ P3 contexts; it remains unrun.

## Current head fully read and native green

Both privacy readers carried their READ-OKs to `7fb4feb20`; Third lifted
the author hold after the second read. Exact Linux formatting passed on
6,967 unchanged files; Third verified both authored git trees and the raw
log hash against Second’s published receipt. `current-linux-fmt-reported.json`
retains that reported execution and cleanup evidence.

ARM64 run 40058695333 succeeded on `7fb4feb20`: **11,171 passed (one
reported Voice journal LEAK), 71 skipped**, process **12/0**. The preview
`931987d6a` tree equals the authored tree, based on current `9ed44f88c`.
Linux Clippy 40058695335 passed. `current-native.json` binds the checkout,
parents, raw log hash and result. The train owns integration; no lamp action
or activation follows from these software gates.
