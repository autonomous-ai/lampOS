# Skills event-drain source handoff — not qualified

Claim: Queue6390256740, logged/pushed before code. Base `42f08cf038660ff60b3415f7c3a64740ce782fcc`; own branch `tests/1806-addon-event-drain`. Apply only `candidate.patch` to that exact base in a fresh source copy. Patch SHA-256 `c7495fa8ae6ecec00958acb26a0201277017e87e8f7e5775ea2a6b3327aea611`. The six payloads are source/producer records, not executed validation. Exact-file pinned Rust formatting and diff whitespace checks passed; compilation and all four observations remain unrun.

Only `crates/skills/tests/it/addon.rs::a_fifth_say` and `docs/skills.md` change. The observer drains concurrently into a bounded 32-Done queue, fails on overflow/errors and stops/joins on success or unwind. The original absolute eight-second completion deadline, every assertion, original proposal waits/full three-second negative observation and product ten-second/hour/drop rules remain unchanged. Queue loss is a candidate mechanism; historical cause is unproved.

## Fixed Linux x86 schedule

Use the same native Rust 1.96.0 Linux amd64 toolchain, user, captured environment and profile/features for both variants. Each variant starts with its own fresh compiler target. Existing owned registry and Git dependency-source caches may be shared; compiled local outputs are not shared between variants. No ARM64 cross-build. Clear inherited LAMP/cross-compilation/runner/linker/PKG_CONFIG/BINDGEN settings. Use CARGO_BUILD_JOBS=2, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0, CARGO_INCREMENTAL=0, RUST_BACKTRACE=1, offline locked Cargo. No RUST_TEST_THREADS override; retain default parallel full tests.

1. Baseline exact42: fmt, no-run compile, actual compiler-artifact/normal `lamp-skills` CLI hash/profile/features and full inventory binding; then one full Skills run and one exact standalone case.
2. Candidate same42 plus this patch: fmt/strict Clippy, no-run compile and matching actual context/inventory; then one full Skills run and one exact standalone case; finish with ordinary workspace build. Preserve every 0/101. No unchanged-test retries or best-sample selection. Abort further allocation on setup, provenance, census, cleanup or outer-timeout failure.

From lampOS, all Cargo compiler/test commands additionally use `--offline --locked --target x86_64-unknown-linux-gnu --jobs 2`:

- `cargo +1.96.0 fmt --all --check` (300 s outer bound).
- Candidate: `cargo +1.96.0 clippy -p lamp-skills --all-targets --message-format=json -- -D warnings` (2,400 s).
- `cargo +1.96.0 test -p lamp-skills --no-fail-fast --message-format=json --no-run` (2,400 s).
- Once per variant: `cargo +1.96.0 test -p lamp-skills --no-fail-fast --message-format=json` (3,600 s).
- Once per variant: actual bound integration-test executable `--exact addon::says_the_judge_dropped_do_not_spend_the_hour --nocapture` (120 s).
- Candidate: `cargo +1.96.0 build --workspace --message-format=json` (3,600 s).

Retain complete raw streams, actual command exits/target/case census, source bytes/kinds/modes/mtimes, actual ELF/test/normal-child bindings, configured environment and bounded owned child/container cleanup/absence. Capture before/after full-run counts and exact standalone outcomes independently. No binaries, private credentials or scratch script bodies in public evidence. Earlier Common9df Linux518/1/9 and its three failed direct observations remain distinct failed epochs. This schedule makes no isolation, general timing, causal or readiness claim.

Coordinate compute allocation before launch: Tests currently has the fixed NoPause147 observations active and allocates no additional local compilation during that window. A same-host offered slot must wait for its release; independently hosted Linux compute can be agreed separately. No observation is authorized by this source publication alone.
