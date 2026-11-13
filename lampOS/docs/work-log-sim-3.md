# Work log: sim-3

Newest first. How a builder works is in [joining.md](joining.md); the role
is in [builders/sim-3.md](../builders/sim-3.md).

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
