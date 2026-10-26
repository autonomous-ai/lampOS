# Composed Mood f4419 original native checks

Both original [#3623](https://github.com/autonomous-ai/lampOS/pull/3623) checks for `f4419f83042513ae8aca0d3f565926ddc8737423` completed successfully on pull-request attempt 1:

- [Linux ARM64 build and test, 40414617660](https://github.com/autonomous-ai/lampOS/actions/runs/40414617660): **7,541 unique PASS; 0 FAIL, LEAK or TIMEOUT**, 53 aggregate skipped tests and 114 skipped binaries. Ordinary nextest selected 12 packages and 70 binaries; workspace compilation is not full-workspace runtime qualification.
- [Linux formatting and strict Clippy, 40414617657](https://github.com/autonomous-ai/lampOS/actions/runs/40414617657): workspace/all-target and separate promoter checks passed.

Both actual checkouts were preview `caf53af0a1ec1a064c0987cab12e634f8a66619b`, ordered parents `90ca725c514bbb1e4b36318d7b3ccbb14dfd05f1` then `f4419f83042513ae8aca0d3f565926ddc8737423`. Its tracked tree `bc45d5c19285435e56f0d955d5d9db45bdc3ba1f` equals the authored head's tree. Workflow, nextest configuration, composed runtime fixture and Reader source bytes match immutable authored Git blobs. Source equality does not establish binary equality.

The execution window begins at ARM64 log line 823 and ends with the summary at line 8366: 7,541 distinct `(binary, test)` identities and distinct ordinals 1–7,541, without duplicate executions or retry/flaky records. The nextest boundary is 155.820 s, without an idle-host or performance claim. The 34 post-summary PASS rows are configured final-output reprints, 22 realtime and 12 privacy-worker; each matches original identity/status/duration/ordinal and its actual configuration filter. No blind deduplication or additional executions were counted. The 53 skips are aggregate-only; individual skipped identities were not emitted.

All **340 ordinary Expression cases** passed across four binaries (27/4/281/28), including both composed controls exactly once:

- `sleep_name::the_service_breathes_the_head_with_moods_breath_from_the_bus`: ordinal 1,150, PASS 0.189 s.
- `sleep_name::an_expired_sleeping_name_is_journaled_once_by_the_service`: ordinal 1,147, PASS 0.055 s.

All **14 ordinary Reader controls** passed once. This is Main's Reader source, not another PR's Reader patch. The isolated Linux root/setpriv DAC case remained ignored; the non-Linux ACL fallback case was not compiled. The [separate composed Mac proof](../tests-mood-90ca-composed-20261027/) at [0ff49ea2e](https://github.com/autonomous-ai/lampOS/commit/0ff49ea2eb9530c3fc89ae8dac6cf646c45caf00) records full serial Expression **340/0/0 once**, both controls, source/owner and 254 sealed guards. Its proof blob matches that immutable commit. Host/native counts overlap and are not additive.

Successful auxiliary steps remain separate: promoter **25/0/0**, workspace doctests **4/0/0**, relay-signals **ok once**, and process **13/0**. The export reports 47 current workspace binaries, two selected examples and `SHA256SUMS` (50 uploaded files). Artifact ID `12695197040`, 116,059,812 bytes, has matching API/upload digest `sha256:b5efe1aa31613b1b3c85ab5cc7149ed4e4d8b984716ca86b33cae55e2a658c48`. Only metadata/logs were collected: no archive or ARM64 binary was downloaded or executed locally, and individual payload checksums were not independently verified.

The original [fa67 native audit](../tests-mood-fa67-native-20261027/) remains tied to fa67/a3b and its original runs. The historical Mac failures, original 2bf controller FAIL1 and distinct read-only alias disposition remain unchanged. No historical cause or waiver is inferred. Latest-base disposition and current-head independent reads remain separate from this original-run audit.

[audit.json](audit.json) exports per-binary counts, both affected records, all required Reader records, source/preview bindings, exclusions, configured-reprint accounting, auxiliary results, artifact metadata and retained-receipt hashes. A bounded read-only watcher closed normally for both original runs; there was no CI write or rerun.

The original parser-time collection manifest remains unchanged as a producer snapshot: its `audit.stdout` summary entry was recorded before that output closed. The separate post-close manifest `604be310c54527f55e072c2ee915ca44e045451d51b82d9182409ec1da1b0000` now binds the completed summary and every included file's rechecked hash/size. Both receipts are retained in `audit.json`. Only that summary entry changed after the initial snapshot; CI logs, source and identity/census receipts, all test outcomes and prior published history remain unchanged. No CI or test was replayed.
