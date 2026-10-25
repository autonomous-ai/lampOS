# Final published Skills journal fixture: frozen validation

The final producer fixture at `cf762f38eb1d53b56456764368608e70c8b02536`, applied to frozen Main `34c8b517c41c8d2a4099b87fe550d4eb25611ea2`, passes the declared six-stage validation. Its semantic source tree is `bcbf05c3d05d315914a06ea56af18376a658a2d0`. This validates that exact two-file fixture proposal; it does not qualify current Main, Common #3064, another PR, ARM64, privacy activation, a lamp or latency.

| Stage | Exit | Actual test counts (passed / failed / ignored) |
|---|---:|---|
| Workspace formatting | 0 | — |
| Strict Skills all-target Clippy | 0 | — |
| Skills no-run compilation | 0 | — |
| Full Skills, once | 0 | 519 / 0 / 9 |
| Exact case, once | 0 | 1 / 0 / 0 |
| Workspace build | 0 | — |

The exact case is `addon::says_the_judge_dropped_do_not_spend_the_hour`. The actual five-target census contains 528 cases including ignored cases. Both observations use default capture and default parallelism. There is no new baseline, retry or extra sample. All old failed epochs remain failed, without a cause waiver.

## Commands and binding

The Linux AMD64 profile uses the pinned cached image in summary.json, native direct same-directory Rust/Cargo 1.96.0 without a rustup selector, debug 0, no incremental compilation, two compiler jobs, no inherited LAMP controls and no RUST_TEST_THREADS override. The target starts fresh; the source caches are read-only. Literal Cargo vectors are:

```sh
cargo fmt --all --check
cargo clippy --offline --locked --target x86_64-unknown-linux-gnu --jobs 2 -p lamp-skills --all-targets --message-format=json -- -D warnings
cargo test --offline --locked --target x86_64-unknown-linux-gnu --jobs 2 -p lamp-skills --no-fail-fast --message-format=json --no-run
cargo test --offline --locked --target x86_64-unknown-linux-gnu --jobs 2 -p lamp-skills --no-fail-fast --message-format=json
cargo build --offline --locked --target x86_64-unknown-linux-gnu --jobs 2 --workspace --message-format=json
```

Between full and build, the freshly attested integration libtest is called directly with `--exact addon::says_the_judge_dropped_do_not_spend_the_hour`, replaying this new full run's observed environment and working directory. It is not an exact Cargo invocation. The Cargo-generated normal CLI binding, current executable/profile/features, all 46 metadata package identities, source/index/mode/mtime guards and full target census are checked. Replay does not reproduce file-descriptor tables, jobserver descriptors, ASLR, scheduling or host isolation. Historical R3 exact used `--nocapture`; no identical old/new invocation is claimed.

All 116 saved pre-observation executable witnesses match the before/after inventories for both tests. The later ordinary workspace build has separately verified source-bound artifacts and replaces the normal CLI: the final cache CLI is not claimed identical to the test-time CLI. The initial independent checker assumption that those phases kept one CLI was too strong; its failed assertion is retained privately with the corrected phase-specific audit. All six actual gate exits remain 0.

## Closure and limits

The terminal, frozen source and owned command-group/container absence are verified; both compiler jobs were returned to Tests after actual closure. Target ownership release does not authorize cache deletion. The 196-check final independent audit is separate from the preserved interim audit and its 359 checks; these are artifact checks, not additional tests.

Stage caps are fmt 300 s, Clippy/no-run 2,400 s each, full/build 3,600 s each, exact 120 s; existing VM startup has a 180 s cap. Root's launcher lacks a separate overall runner wall-clock watchdog. These stage caps do not bound arbitrary filesystem I/O or total elapsed time. This limitation is retained with the result.

Only selected counts, source hashes, declared commands/profile and closure booleans are published. Raw streams, actual environment values, private helper implementations, path inventories and executable witnesses remain private. No runtime source commit or PR readiness is supplied by this result. The producer owns integration and current-source gates for its fixture.
