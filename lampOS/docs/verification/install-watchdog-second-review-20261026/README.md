# PR #3574 independent host review

Exact executed head: `32cc833686748249256d76e8529298a100ac7a82`; parent: `aa7cf9029ef051ee5770837ccaed4ffc646cd74f`.

Independent isolated host gate: formatting, strict lamp-update all-target Clippy, then matching compile/runtime `cargo +1.96.0 test --locked --offline -p lamp-update --test install`. Runtime adds `--no-fail-fast -- --test-threads=16`; compilation adds `--no-run --message-format=json`. This is one complete install integration invocation, not the full Update or workspace suite. Exact outcomes and cases are in census.json.

Reviewed the changed helper, all nine affected test entry points and preserved assertions. No P1/P2 found. Five ordinary install positives now run directly; they still verify installed sequence, override/target preservation, head-pins migration and host enable calls. Four refusal cases retain their owned children, ready witness, 15-second setup and 5-second post-ready limits, kill/reap and scratch cleanup. Product code is unchanged.

The runtime controller has an 870-second deadline, at most 15-second polling overshoot and at most 15 seconds for termination/reaping, within the requested 900-second outer bound. No retries. Parent-reported historical baseline failures and author/native results are independent evidence, not rerun or erased by this host gate. No device or ARM64 execution occurred here.

Raw log hashes, normalized commands, source inventory hash and actual/preserved executable bindings are retained. No binaries or private paths are published in this packet.
