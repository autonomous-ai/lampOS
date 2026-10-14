# Touch: final exact-head native ARM64 gate

[Run 38573575450](https://github.com/autonomous-ai/lampOS/actions/runs/38573575450) passed on
`08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca` (`third/touch`). The runner was GitHub's native Ubuntu 22.04 ARM64
environment with Rust 1.96.0, not a lamp. It runs as an ordinary user;
permission tests were not weakened for root. Ubuntu's glibc 2.35 is the
workflow's chosen build baseline for the lamps' Debian 12 glibc 2.36.

Commands, in order, from `lampOS/`:

```sh
cargo build --locked --release --workspace --bins --examples
cargo test --locked --release --workspace -- --test-threads=1
```

Release build passed. The serial workspace tests report **4,054
passed, zero failed, 29 inherited ignored**, zero filtered in
264 completed harnesses. The raw ignored
cases are in `results.json`; no touch test is ignored. These counts overlap
the host gate and are not added to it.

`lamp-controls` SHA-256:
`37924f2bfea364a214243e7b935a85ca1e8302167393c34590530a2908fc18f1`.

Artifact: `lampos-arm64-08a32c9d8e9fbab643b6442e6e3e7a37736ca9ca`, ID `11900607240`;
the workflow retains it for seven days. First installs this run's binary for
the switch-off minute rather than rebuilding. `artifacts.json` includes its
expiry and digest. No binary is committed. `run.json` keeps the exact head,
conclusion and step timing; `run.txt` is unedited `gh run view --log` output;
`results.json` records counts, retrieval time and raw-log hash.

## Cancelled obsolete run

Run 38571800855, head `1ce4f987303013631fefd3b0f1b1fe64767144f6`, was cancelled
after the source-only Mac CLI output test exposed its timeout. It is **not a
passing gate**: 1,733 passed, zero failed, eight ignored in 106 completed
harnesses before cancellation; later harnesses were not reached. Its full
available log, metadata and artifact list remain in `cancelled-38571800855`.
The corrected buffered-output implementation was verified afresh in the
exact-head final run above. Older Linux compile and baseline Records failures
remain in the earlier task/log verification folders.

## Not shown

This proves software compilation and tests on native ARM64 CI only. No
electrode calibration, physical latency, real I2C/GPIO acquisition, sustained
head-pad contact, actual Voice/privacy transition or motor/light response
was tested. First must use the per-unit procedure in the task's Controls
document before enabling `--touch` on a lamp.
