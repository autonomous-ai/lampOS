# Work log: sim-3

Newest first. How a builder works is in [joining.md](joining.md); the role
is in [builders/sim-3.md](../builders/sim-3.md).

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
