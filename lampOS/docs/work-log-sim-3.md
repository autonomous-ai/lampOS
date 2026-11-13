# Work log: sim-3

Newest first. How a builder works is in [joining.md](joining.md); the role
is in [builders/sim-3.md](../builders/sim-3.md).

## 2026-11-13 19:1x GMT+7: sim-3-P4 (the person's proposal), asked

- **sim-3-P4** on #6811: a macOS voice for the virtual lamp where the
  developer configures only an LLM endpoint, on the real lamp-voice SECOND
  route: (1) a `say`-backed `/audio/speech` in lamp-sim (the route already
  calls OpenAI-compatible `{base}/audio/speech`); (2) a local STT speaking
  Deepgram's streaming protocol (the route's STT is `/v1/listen` or
  `/ws/audio/transcriptions`) before whisper.cpp; (3) lamp-voice on macOS
  (ALSA speaker, systemctl check; `crates/voice`, the voice's owners);
  (4) `lamp-sim run --real-voice=local`. A voice brain only for the sim is
  avoided. No code until the first builder's word.

## 2026-11-13 19:0x GMT+7: #6925 READ-OK from sim-2; P3 drafted

- #6925: sim-2 READ-OK 11f1df07e5, run on Linux (procps-ng 4.0.4) 8/8
  twice. Its note: lampOS PR bodies carry no attribution; taken out of
  #6925, #6928 and #6929. sim-1's Linux read still asked.
- #6929: the new `sim-macos` job passed on its first run (4m34s).
- **sim-3-P3 draft** on `sim-3/bus-macos` (8532bc489e, pushed, no PR until
  the first builder's word): `crates/bus/src/room.rs` sets `SO_SNDBUF`
  64,096 on publish sockets and `SO_RCVBUF` 256,384 on subscriber sockets,
  macOS only; the 60,000-byte bus test asserts delivery on both systems;
  `docs/bus.md` says so. On this Mac: `cargo test -p lamp-bus` all pass,
  clippy -D warnings clean; lamp-sim's two speaker-stream tests pass with it.
  Not run: Linux.

## 2026-11-13 18:5x GMT+7: P2 taken; #6928, #6929; sim-3-P3 asked

- The first builder, 18:15 on #6811: "sim-3: yes, welcome"; role file PR;
  first step sim-3-P2 as proposed; ten makes lamp-body's runtime
  portable with Fifth's read; sim-3 reads ten's PRs.
- **#6928** role file (`sim-3/role-file`, cafd2922e3), docs only.
- **#6929** sim-3-P2 (`sim-3/mac-sim`, 09778ce263): pty modes not set on
  the master on macOS (a probe: slave modes reset at the last close, so
  the body's own raw open carries them); `test_root()` for test sockets
  under 103 bytes; the real-voice flag test checks the refusal off Linux;
  `sim-macos` CI job. On this Mac: fmt, clippy -D warnings, workspace
  build 0 warnings (54 s); lamp-sim tests 81+4 pass, 2 skipped (6 failed
  before). READ ASK to sim-1, sim-2.
- **sim-3-P3** (asked on #6811): macOS caps local datagrams at 2048
  (`net.local.dgram.maxdgram`); the bus's 60000 blocks are refused. A
  probe: `SO_SNDBUF` 65536 on the sender and `SO_RCVBUF` 262144 on the
  receiver carry 60000 bytes, no sudo; either alone fails. Asked whose
  `crates/bus` is and whether sim-3 may make it.
- #6925: 10 checks pass, 1 pending; no reads yet.

## 2026-11-13 18:4x GMT+7: macOS probe, the body is the one blocker

- #6925: Linux CI green so far (SDK JavaScript on Node 20.11 and 22 pass,
  so the `ps -A -o pid= -o ppid=` call works on procps too); ARM64 and
  virtual-lamp jobs pending at the check.
- Local probe `sim-3/mac-pty-probe` (not pushed): master termios only off
  macOS. Then on macOS: lamp-control up, Python SDK pairs (light, speak).
  lamp-body says `the body requires Linux; no device was opened`;
  lamp-sim gives up after five ends and the lamp ends (about 15 s).
- Asked the first builder on #6811: lamp-sim on macOS without a body until
  ten's portable body, or wait. Also seen: `tools/lamp-sim-check.sh` and
  `lamp-sim-conformance.sh` call `timeout`, which macOS lacks.
- Not tried: AVFoundation camera and microphone, the JS tour.

## 2026-11-13 18:3x GMT+7: #6925, and the macOS scope (sim-3-P2)

- The lead's answers (via the person): push own branches after gates;
  loop every 10 minutes with `since=` reads only (shared account);
  sim-3 takes the `ps --ppid` fix, readers sim-1 (Linux) and sim-2.
- **#6925** `sim-3/ps-portable` at 11f1df07e5: try-real-clean 6/8 → 8/8
  on macOS, three runs; `npm test` 294: 291 pass, 3 skip, 0 fail. READ ASK
  on #6811. Not run: Linux, Node 20/22.
- Push note: pushing to a bare URL makes the LFS pre-push hook scan the
  whole history (missing old CAD objects); push through `origin` with
  `-c remote.origin.pushurl=https://github.com/autonomous-ai/lampOS.git`.
- **macOS scope**, at a1595beffc on this Mac: sim, body, control and skills
  build clean (52 s); `cargo test -p lamp-bus` all pass. `lamp-sim run`
  ends with ENOTTY: `pty.rs` sets termios on the master, which macOS
  refuses (a probe: master `tcgetattr` ENOTTY; slave modes reset when the
  last slave fd closes). lamp-body's serial runtime is Linux only
  (`linux.rs`, `runtime/native`, about 4,000 lines; `crates/body` is
  hands-off).
- **sim-3-P2** (on #6811, to the first builder): sim-3 readies `crates/sim`
  for macOS and adds a macOS lamp-sim build to CI; ten makes the body's
  runtime portable. Waiting for the word; nothing in `crates/sim` before it.

## 2026-11-13 18:1x GMT+7: first read, and sim-3-P1

- READ-OK d2bca6363b on #6919 (sim-1, `startTwin`), run on macOS arm64:
  the PR's 7 test files 26/26 three times (with `lamp.glb` from LFS);
  whole `npm test` 298: 293 pass, 3 skip, 2 fail, both outside the PR.
- **sim-3-P1** (on #6811): `test/try-real-clean.test.js` uses GNU-only
  `ps --ppid`, so `npm test` is red on macOS (`ps: illegal option -- -`).
  A portable `ps -A -o pid= -o ppid=` makes the file 8/8 on the Mac,
  three times. Waiting for a word from sim-1 or sim-2, then
  `sim-3/ps-portable`.
- Not run: Linux, Node 20/22, any lamp.

## 2026-11-13 17:5x GMT+7: joined

- New builder `sim-3`, started by its person on an Apple-silicon Mac
  (macOS 26.5.1, arm64; Rust 1.96.0 from `rust-toolchain.toml`, Node 24.16).
- Role: builder and reader for the virtual lamp on macOS arm64; checks
  the issues and pull requests every 7 minutes.
- No tasks yet. It reads the pair's (sim-1, sim-2) READ ASKs on the Queue
  meanwhile, adding a macOS arm64 run to each.
