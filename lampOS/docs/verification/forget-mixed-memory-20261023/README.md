# Follow-up forget against older Memory, 23 October 2026

Second's additional #3130 compatibility counter on exact stacked head
`8dd86272e9f9bfec63e1c62df2d6a06c714e75b3`. It uses the real `DaysClient`,
Unix socket protocol and SQLite `Days` store. All content is synthetic.

## Result

| Store runtime source | Result |
| --- | --- |
| Current `8dd86272e` | **1 passed / 0 failed**, exit 0. The unrelated kept item remains. |
| Legacy `ae4ab0022` | **0 passed / 1 failed**, exit 101. The store acknowledges deleting the Asked ID, but the unrelated kept item is also gone. |

Both use exactly the same new test (`counter.patch`). The store creates an
exchange, an explicit kept item and a follow-up observation; both derived
entries genuinely cite that exchange. The real Voice client offers and
marks the follow-up as asked, then receives a bare forget. Before the final
preservation assertion, both runs establish successful acknowledgement and
the exact request `forget` of the observation ID. Current Memory preserves
the unrelated keep; legacy Memory leaves no entries at all. No algorithm is
mocked or reconstructed in this counter.

`legacy-runtime.patch` substitutes the complete three changed Memory source
files from `ae4ab0022`. The other 17 runtime files and Memory's Cargo.toml
are byte-identical between the two heads; `legacy-binding.json` records
them and each substituted file's SHA-256. Voice and workspace dependencies
remain current. This is a mixed-source compatibility counter, not an old
release binary or deployed-update qualification.

## Reproduce

On `8dd86272e`, apply `counter.patch`, then run on the Mac with Rust 1.96.0:

```sh
cargo +1.96.0 test -q --locked --offline -p lamp-memory --test it voice::a_follow_up_forget_preserves_an_unrelated_keep_with_shared_evidence -- --exact --nocapture
```

Apply `legacy-runtime.patch` as well and repeat exactly the same command.
The full outputs are `current.txt` and `legacy.txt`. Their matching JSON
records retain source identities, command, counts, status, elapsed time and
raw output hashes. The current and legacy manifests bind all 6,520 tracked
inputs; each remained unchanged throughout its command. `restoration.json`
records all four restored files and a clean detached peer worktree.

## Review boundary

The new Asked path in `remember.rs` sends ordinary `forget` directly. Its
named-forget counterpart has #3127's older-Memory refusal probe; this path
does not use it. The dynamic result establishes the damage if these service
versions meet. Whether the product updater permits that combination is a
separate rollout audit assigned to Friday. No updater, device, release-key,
acoustic or full-workspace qualification is claimed here.

The present regression expects success on current Memory. If a correction
safely refuses older Memory, its legacy test must explicitly expect that
refusal and preserved data. The author owns that correction; Second has not
changed or pushed the peer branch.
