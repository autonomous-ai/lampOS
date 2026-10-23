# Corrected-candidate Linux x86 plan

Preparation only: no Cargo command or test has run from this plan. Execution needs the root agent's explicit review and CPU grant.

The immutable candidate is `da595851f239cd6456cc1fb4565d3fc272675702` on baseline `f477d6c49732dbd5cba84cbe18ea568692c056e6`. Relative to the initially tested candidate `b5f2e39d2a1d561364c754c1ff904ca588c63c29`, only `hand_controls.rs` changes: the fixture enters its stepped scope after child readiness and before queuing the provider fault and constructing OptionalCamera. The corrected hand-controls SHA256 is `c9c7647a8ac55de5c580e9de3d04b82ff041d61945c912c7bbfeb9242693cad1`. `source-correction.patch` records that exact commit comparison.

The plan contains only corrected-candidate execution. It reuses the original baseline's already-completed3,583-name compiled inventory through an explicit path and SHA256; it does not rebuild or repeat baseline tests. The original held-lines overflow control, initial candidate9pass/1fail/76unrun observation, and separately authorized baseline hand-touch pass remain unchanged under `/private/tmp/tests-boot-clock-linux-20261023/`. The corrected source is a new observation, not a retry of unchanged candidate bytes.

All86 exact case names and their ordering/expectations are unchanged. Each must pass once, in its own fresh60.x-second Linux time namespace, with the same120-second real host watchdog and clock/namespace checks. The full compiled inventory must still match the pinned original baseline. The runner stops on any unexpected case result and does not continue or retry automatically.

Sequential commands, from the immutable archived lampOS root:

```text
cargo test --offline --locked --target x86_64-unknown-linux-gnu -p lamp-voice --lib --no-run --message-format=json-render-diagnostics
<Cargo-identified retained x86-64 ELF> <exact case> --exact --nocapture --test-threads=1
cargo fmt --all --check
cargo clippy --offline --locked --target x86_64-unknown-linux-gnu -p lamp-voice --all-targets -- -D warnings
```

The last two commands are separate required Linux gates, reached only after all86 exact cases pass. Formatting has a600-second real watchdog; compile and Clippy each have2,700seconds. All commands inherit offline networking/Cargo settings; formatting performs no build. Jobs2, dev/test debug0, incremental0 and the explicit native x86 target are retained. The image's ARM/cross variables remain cleared. No ARM build, workflow, install or device action is included.

The same exclusively Tests-owned Cargo/target volumes and cached image are used. Source is the full tracked lampOS tree, archived directly from immutable Git objects; all source hashes are checked before/after freshening mtimes, then the snapshot is unwritable. Actual Cargo JSON identifies the executable, and retained ELF/hash/census proof precedes testing. Containers keep default UID1000/default capabilities, networknone and a read-only image. Only the owned volumes and private/tmp are writable during compilation. The owned-storage guard remains6GiB, leaving2GiB headroom within the8GiB budget; it is monitored, not a filesystem quota.

`runner-change.patch` and `plan-change.diff` show every preparation change from the initial runner/plan. Runner changes only pin the existing baseline inventory reference, add the two post-case gates, and restrict its CLI to the candidate role. Original runner files and receipts are untouched. `preparation.json` records plan/script/archive hashes and syntax/dry-validation results. Full raw outcomes will be written under a new `runs/` directory only after execution is authorized.

The60.x-second checks still do not demonstrate the1–21-second boundaries, the blocked Policy integration cases, the ignored real-latency probe, device behavior or production activation. No full-library test pass is implied by this exact-case run; the complete compiled inventory is a census only.
