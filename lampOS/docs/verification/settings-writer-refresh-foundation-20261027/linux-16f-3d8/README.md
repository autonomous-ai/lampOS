# Original #3775 Linux qualification

Run 40474816661, job 122090318699, attempt 1: completed successfully. No rerun requested. Actual checkout 27a5cd9bcbc52abdd121a97f7ae33b1aa08a71f7 has ordered parents [3d8c75558e235c125b9cf953a66a6c193888fd9e, 16f0e1cb27d2796c953ba1e0d0de92f4e37aff53]. Its complete tree ad933fb9f533d3fee3642ad5074cec447bfe7b3f equals the authored head’s complete tree.

The full 623-line job log and original run/job/attempt APIs were inspected. Rust 1.96.0 was installed for x86_64 Linux; the workspace also pins 1.96.0. Actual executed stages all succeed:

- Stable-promotion tool formatting and locked strict all-target Clippy.
- `cargo +1.96.0 fmt --all --check`.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`; lamp-update is visibly checked, with successful Finished output.

Required step conclusions are success and the actual shell uses `bash -e`. Numeric per-command exit codes are not separately printed. Cache restoration/source-time reuse is recorded, not represented as a clean rebuild. Cache save is intentionally skipped; no required gate is skipped. The raw log’s final Node action deprecation warning is retained and is separate from Rust diagnostics.

`proof.json` carries source and stage bindings; `stages.txt` contains numbered sanitized excerpts. Raw complete logs and API receipts are retained privately, identified by hashes. This proves Linux formatting and strict Clippy on this exact authored tree. No ARM, runtime tests, device, installation, signature-trust or writer-refresh operation is claimed by this audit.
