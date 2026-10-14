# Touch native ARM64 gate, before the working-pad update

Run [38564855931](https://github.com/autonomous-ai/lampOS/actions/runs/38564855931)
passed on task head `721d7c0722320281ff02d0b7d97cd91c3c23e730`.
This is a GitHub-hosted Ubuntu 22.04.5 ARM64 runner, Rust 1.96.0
(`aarch64-unknown-linux-gnu`), not a lamp.

Commands, in order, from `lampOS/`:

```sh
cargo build --locked --release --workspace --bins --examples
cargo test --locked --release --workspace -- --test-threads=1
```

The build passed. All 263 completed test harnesses total **3,987 passed,
zero failed, 29 inherited ignored**, zero filtered. The exact ignored cases
are in `results.json`; no touch test is ignored. Do not add these counts to
overlapping development-Mac runs. `run.txt` is the unedited `gh run view
--log` output; its hash and the retrieval time are in the result. `run.json`
keeps source, step timing and conclusion. `artifacts.json` identifies the
retained native binaries; no binary is committed here.

`lamp-controls` SHA-256:
`30a54d871363d46d3a75b6c3b4b6488b49e64942b4fb79e461f83b7904c493d3`.

## Scope changed after the passing run

First's [20:05 hardware answer](https://github.com/autonomous-ai/lampOS/issues/711#issuecomment-6169047710)
confirms that E4/E8 are faulty on lamp-4ace and asks for a per-unit working
set and per-pad touch/release thresholds. Head 721's explicit threshold
arrays already support independent pad values, but it requires all twelve
pads in the geometry. It is therefore **not offered ready**. The pending
working-set change needs new tests and a new green native run on its final
head; this passing run is retained as the prior result.

## Not shown

No hardware access, physical latency, electrode calibration, sustained
head-pad sensing, applied Voice/privacy transition or motor/light response.
This result does not cover the subsequent working-set implementation.
