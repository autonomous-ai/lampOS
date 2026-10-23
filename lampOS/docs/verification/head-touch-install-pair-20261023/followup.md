# Cue default and local-system override follow-up

Source `92088b7eaf1127cba12037399e7a8f7aef451846`, tree
`fd526b051d729a6976b012aef342ab8a1db1d004`, on current `be16c3327`.
[Source and output hashes](final-source.json). The [range-diff](followup-range-diff.txt)
keeps the original three commits identical and adds two focused commits.

## Changes and read

- `4d00bc114` adds `/usr/local/lib/systemd/system` to the conservative
  override scan. The existing refusal matrix now includes a head-off Voice
  drop-in there, preserving state, files and zero host calls. The original
  controls are unchanged. This addresses the privacy reader's P3.
- `92088b7ea` implements the assigned −20 dBFS default. The c0a4 comparison
  found −30 ear-only, −25 faint and −20 clear at product speaker level.
  The default is shared by tap/wake, sleep breath, unclear/misheard hums
  and `say`'s built-in hum; the tap evidence does not qualify all of them.
  The test pins −20 and checks both waveforms' RMS, fades, duration and
  peaks. The former 15 dB peak-to-RMS allowance remains; its new ceiling
  is −5 dBFS, below clipping. Fallback notice and file-sound limits stay
  independent. Timing and admission gates are unchanged.

The docs distinguish the 21 October attended c0a4 recovery (~370 ms to
readiness) from the later 23 October injected-input recovery (~300 ms).
Neither is first-speaker latency. The historical-trial clarification in
`lamp-runs.md` was the only file changed after the final test command began;
all tested code and test files match the recorded hashes. No lamp was used.

## Final Mac gate

Intel Mac, pinned Rust 1.96.0. Commands from the source checkout's `lampOS/`:

```sh
cargo fmt --all --check
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/third-head-touch-install-target cargo test -q --locked --offline -p lamp-voice -p lamp-update -- --test-threads=2
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/third-head-touch-install-target cargo clippy -q --locked --offline -p lamp-voice -p lamp-update -p lamp-controls -p lamp-setup --all-targets -- -D warnings
```

All exit zero. [Full tests](final-host-tests.txt): **4,310 passed, 0 failed,
19 ignored**, all 15 targets complete. Update includes unit 120/0/2 ignored,
install 87/0 and rounds 151/0; its two Linux-only custom harnesses are
ignored on Mac. Voice library is 3,653/0/15 ignored and every integration
target passes. No new test is ignored. [Strict Clippy](final-host-clippy.txt)
is empty. Formatting passes. Earlier [cue tests](cue-focused.txt) were 8/0
and [focused install controls](local-system-controls.txt) were 5/0.

The first full invocation stopped in Update's unit target: **115/5/2**.
Four fixtures failed with `Operation not permitted` when binding loopback
sockets, and a bus fixture failed its assertion. That [output](sandbox-denials.txt)
is retained. The unchanged suite was run with approved local socket access;
all targets then passed. There was no assertion, timing or product change
between these attempts. The failed attempt never reached Voice or Clippy.

## Native and review status

The earlier `413378623` head passed ARM64
[39975707789](https://github.com/autonomous-ai/lampOS/actions/runs/39975707789):
11,072/0/69 skipped, one reported Voice LEAK; doc tests and process harnesses
passed. Linux Clippy
[39975707764](https://github.com/autonomous-ai/lampOS/actions/runs/39975707764)
also passed. Its checked merge `3b72fc4b5` has the same tree as that authored
head. [Retained summary](first-head-native.txt). Those results do not qualify
the follow-up head.

Fresh official native gates and source-only Linux formatting are requested
for `92088b7ea`. The privacy reader re-reads the two-commit range; the units
reader checks the combined carry and update pairing. The first builder's
explicit HOLD remains until those reads and its exact-head READ-OK. No
ARM64 binary was linked on the Mac, and no exact-new-build physical acceptance
or acoustic latency result is claimed.
