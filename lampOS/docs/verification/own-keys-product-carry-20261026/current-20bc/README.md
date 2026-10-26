# Own-keys ba950 carry and Update qualification

Current head 20bc7d626e7b53d4ea7e7e3a2b5018c93973c7f9 on ba950f289b4e0d4fd072d080ea3a1b54e7bce03f. Seven patches remain equal. Thirty-seven authored blobs/modes are identical; shared install.rs inherits exactly the upstream test-helper patch. The complete repository delta from d378/6663 is install.rs and updates.md only. The executed d378 source was preserved before rebase.

Current gate is fmt, strict Update all-target Clippy and matching compile/runtime selections `cargo +1.96.0 test --locked --offline -p lamp-update --lib --test install`. Runtime adds `--no-fail-fast -- --test-threads=16`; compile adds `--no-run --message-format=json`. Runtime bound is 870 seconds plus at most 15-second polling and 15-second cleanup, within 900 seconds. All nine inherited preflight controls, seven own-keys installer/product controls and the existing setup-gating census must pass once. Counts and literal target summaries are in census.json.

The explicitly owned compilation target was reused only after previous exclusive Voice closure and a new guard of every preserved executable, local library/build-script output and depfile. Exhaustive source inventory comparison confirmed exactly the two inherited files changed. The new gate retains its own actual/preserved local outputs and depfiles. Previous actual artifact paths may be rebuilt; previous results retain their original execution identity.

Exclusive Voice 4093/0/21 stays d378/6663. Historical six-crate 6175/0/48 stays ff17/690. Reused-target observation and both metadata preparation failures remain separately preserved. No earlier result is relabeled as current Update execution. Current native ARM64/Clippy and activation remain separate; no device or provider actions.
