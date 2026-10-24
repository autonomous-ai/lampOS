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
