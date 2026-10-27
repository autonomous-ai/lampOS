# Child #3577: current-source host verification, 28 October 2026

Commit `4cd9d4995c482e0bc15f2ee537066205efd3d234` (tree `61b8507430ae3b4791cc7a441e18e58e4a0618e8`) follows Parent `1b1da9ae3fc6fbf1101cd1b88f10bcaa37fb55ad`.
The four normal compiler stages passed. The **whole Voice-it integration
binary passed 275 unique cases once**, including all **18 required
diagnostics** and the **one sequence identity within those 18**. Its three
sequence rows are one test, not three added cases. There were no failures,
selected unrun cases, ignored cases, retries, timeouts or LEAKs.

Workspace docs are separate: **4 PASS / 0 FAIL / 0 ignored**. Neither the
Parent's 4,177 cases nor an earlier source's 275 cases supplies credit here.
Both child blobs are exact the original `a47fe22e56d181f6d7946bc5b98d90dd9560293e`.

[source-ledger.json](source-ledger.json) records source hashes, the 18 outer
terminal references, and the saved normal Voice/test-driver bindings.
Executable hashes were copied from actual 50364's saved input manifest;
the current target was neither read nor rehashed. Inputs were guarded
before and after Voice-it; later Cargo docs/builds have a separate boundary.

Native PR checks and review remain separate gates. These host fixtures do
not measure lamp hardware latency or activate behavior. See
[receipt.json](receipt.json) for the source-scoped counts and
[manifest.json](manifest.json) for this text packet's hashes.
