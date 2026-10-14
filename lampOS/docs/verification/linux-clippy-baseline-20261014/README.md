# Linux Clippy dependency failure, 2026-10-14

Task 29's standalone Linux gate stopped at inherited Body code:
`crates/body/src/inspection.rs:615`, `pub(crate) fn run`, is unused in the
Linux library and `-D warnings` makes that an error. The same command was
repeated using unchanged Memory, Voice and Body source from Fourth base
`3a8d0ffbdce969d238d4152533e2b89897315706`; only unrelated Records source
was modified in that standalone copy. It failed identically. `clippy.txt`
is the unedited output of this baseline reproduction, exit 101.

Environment: existing local Podman VM, Linux x86_64, Debian 12 build image
`localhost/lampos-arm64-cross:latest`, Rust 1.96.0, offline cached packages,
four Cargo build jobs. This is a host dependency check, not ARM64 execution
or a lamp result.

```sh
cargo clippy -p lamp-memory -p lamp-voice -p lamp-voice-eval   --all-targets --offline --locked --target x86_64-unknown-linux-gnu -- -D warnings
```

No Body edit, lint suppression, test skip or relaxed gate was made.
Reported to First on task #698; Linux strict verification remains pending.
