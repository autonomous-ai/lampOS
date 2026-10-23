# Photo #2988 — exact-head native CI receipt

Head `c71274d32b76e8d7587ec6bd0ff5072f78a9c615`; tree `c419e6c2647191176e0f4cd7cb84a71316f11d2c`; author parent Common `00c79072298c583c88ac92a0cad42d2f9155520c`.

| Native gate | Result |
| --- | --- |
| [ARM64 39967057269](https://github.com/autonomous-ai/lampOS/actions/runs/39967057269) | **11,220 passed / 0 failed / 69 skipped**, including **1 leaky pass**; **0 selected tests unrun** |
| [Clippy 39967057642](https://github.com/autonomous-ai/lampOS/actions/runs/39967057642) | Strict workspace/all-target Clippy passed |

Both jobs ran attempt 1 and checked out merge `7daa68a461e3b92a1c9a3ff5ee315e20320aca52`, with merge base parent `810e05109bced3beca88a5b602930cfed5fba4e5`. The executed tree **exactly equals the author tree**, with zero mode/OID/path differences. No CI retry or dispatch was issued.

All **96 Photo controls** and **80 Common dependency controls** explicitly passed. One scratch census belongs to both groups, giving **175 distinct test identities**. This includes the three new Pending tests (seven source cases), three retained held-HTTP controls, wire/scoped/transport/Engine cases, Cloud's neutral broker receipt and cleanup, and scratch/product censuses. The identities and current source hashes are in the accompanying JSON; older results are not inherited.

Separate from nextest: **4 doctests passed**, **12 turn-end-wire process cases passed**, and `relay_signals` succeeded. Nextest selected the whole workspace with profile `pr`: 166 binaries, with two custom-harness binaries excluded from nextest and run separately. The 69 skipped tests remain skipped. The leaky pass is `lamp-voice same_voice::tests::each_line_the_check_says_is_said_to_its_journal`. The recount has 11,219 PASS and one LEAK, with 35 repeated terminal lines deduplicated and no conflicting results.

Observed CI commands (relative to the lampOS workspace):

```sh
cargo build --locked --release --workspace --bins --examples --message-format=json-render-diagnostics > arm64-build.jsonl
cargo build --locked --release -p lamp-schedule --example schedule_check --target-dir target/lampos-artifact-examples --message-format=json-render-diagnostics > arm64-schedule-example.jsonl
cargo build --locked --release -p lamp-expression --example replay_cost --target-dir target/lampos-artifact-examples --message-format=json-render-diagnostics > arm64-expression-example.jsonl
filter='not (binary_id(lamp-ipc::relay_signals) | binary_id(lamp-turn-end-wire::process))'
cargo nextest run --locked --release --workspace --profile pr --no-tests=pass -j num-cpus --partition hash:1/1 -E "$filter"
cargo test --locked --release --workspace --doc -- --test-threads=1
cargo test --locked --release -p lamp-ipc --test relay_signals
cargo test --locked --release -p lamp-turn-end-wire --test process
cargo clippy --locked --workspace --all-targets -- -D warnings
```

Artifact [12507514335](https://github.com/autonomous-ai/lampOS/actions/runs/39967057269/artifacts/12507514335), `lampos-arm64-c71274d32b76e8d7587ec6bd0ff5072f78a9c615`, belongs to the exact ARM64 run/head. Its **116,019,716-byte ZIP** matches GitHub SHA-256 **`0d70f4ad44187ac75db3fda2ecfeb65c53de1149f6254cd9f1a553c09d535cd9`**. All **49 members** (47 current binaries and two selected examples) match `SHA256SUMS` and are **ELF64 little-endian AArch64**. Nothing was extracted or executed. Download/check bounds were 300 seconds, 512 MiB compressed, 3 GiB expanded and 256 members.

**Remaining limits:** original-capture classification and stricter Photo whole-interval proof are not integrated; actual Eyes transaction and First/Mobile physical/privacy/lamp acceptance remain open. `LAMP_PHOTO_SPEAKER` is not enabled. Generic scoped speaker fixtures and green CI do not establish original/replayed input exclusion, liveness, device behavior or activation approval. This receipt makes no readiness or deployment claim.

The accompanying [JSON](receipt.json) contains sanitized structured counts, exact control/source identities and artifact member hashes. It contains no absolute personal paths or raw logs.

Train 50 `09f8e7412` landed after these jobs began. It is not included in this receipt; the clean carry is being qualified separately.
