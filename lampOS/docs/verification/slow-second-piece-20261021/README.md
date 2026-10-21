# A held second speech piece

Part of #2602. This adds a regression to the real second-provider engine's
existing `Script` services. Production code and defaults do not change.

`a_slow_second_speech_piece_keeps_order_until_release_or_retirement` feeds
three sentences whose audio packets contain their text lengths. An explicit
release signal holds the second sentence's first audio. The first sentence
is delivered while the second is held, and a notification proves the third
sentence's packet is already ready. For 2,970 ms on Tokio's paused clock,
neither the third sentence nor generation/turn completion may pass the hold.
Releasing it delivers the second and third sentences, each exactly once,
with ordered transcripts, packet sequence numbers and all 1,500 samples per
sentence. A second case retires the turn during the hold: retirement drops
the waiting stream, rejects its release signal and emits no stale output.

The earlier order test delays the first sentence by 30 ms; this case covers
a delayed later piece and a ready successor. The 2,970 ms hold matches the
scale of the reported slow second-piece request. It is synthetic time, not
a measurement of a provider, speaker or acoustic improvement. No network
provider, lamp or playback device is used. The remaining observation does
not justify a new runtime change; the paired lamp comparison remains with
the first builder.

Checked on macOS x86_64 with Rust 1.96.0, from base
`07ce0a6c4f255f22bb39cdf40b83fb27def0c684`:

- `cargo fmt --all --check`: passed.
- `cargo clippy -q -p lamp-voice --all-targets --message-format=short -- -D warnings`:
  passed.
- `cargo test -q --locked --offline -p lamp-voice --lib second_provider::tests::a_slow_second_speech_piece_keeps_order_until_release_or_retirement -- --exact --nocapture`:
  1 passed, 0 failed, 0 ignored, both before and after the mutation below.
- `cargo test -q --locked --offline -p lamp-voice --no-fail-fast -- --test-threads=1`:
  3,280 passed, 0 failed, 14 ignored across six test targets.
- `cargo build -q --locked --offline --workspace`: running when this draft
  was published; the terminal result will be recorded in the pull request.

Mutation check: temporarily skip an empty later speech queue once the answer
has begun. The test failed at `a ready third piece and completion must wait
for the second` (0 passed, 1 failed). The production file was restored
byte-for-byte, and the regression then passed again. The mutation patch and
failed output remain in `/private/tmp/fourth-slow-second-piece-checks`, beside
the full local gate logs and source fingerprints. An attempted nextest gate
could not start because `cargo-nextest` is not installed; the full voice gate
uses `cargo test` instead.

Native Linux ARM64 and workspace Clippy must pass on the pull request before
it is ready. None of these checks shows acoustic continuity, a shorter
provider delay, or a paired lamp before/after improvement.
