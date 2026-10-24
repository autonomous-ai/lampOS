# Cloud acknowledgment test: published-head native runs

These original completed runs tested #3410 head `72a5a0f67c1a9d12d257e38d85cff7bfaf268999` on parent `bcb540a085408cdf764dfdb211d415a0e8239727`. They do not qualify the later local fc31 carry.

- [ARM64 run 40130844249](https://github.com/autonomous-ai/lampOS/actions/runs/40130844249): 1,216 selected passed, zero failed/timeouts/leaks, eight skipped. Cloud 196 + Update 397 + VoiceEval 623. The changed Wi-Fi test took 5.035 s.
- [Linux Clippy 40130844271](https://github.com/autonomous-ai/lampOS/actions/runs/40130844271): success.

`primary.txt` files are complete byte-for-byte original logs. `run.json` is original run metadata; the remaining JSON files are derived case, comparison and tree projections. The actual preview `9fa61c699afe5cd6f0a07219442bc175ad7f3e0f` has the authored head and bcb parent; its complete tree equals the authored tree. Compared with the earlier 5b run, the ten additional selected cases are landed updater controls, with no removed or changed-status cases. Both populations remain separately identified.

No CI rerun, local ARM64 build, device action, isolated-load comparison or performance-cause claim. File hashes and sizes are in `manifest.json` and `SHA256SUMS`.

The two primary logs retain original trailing whitespace. Diff whitespace validation excludes only these byte-preserved logs; hashes validate them instead.
