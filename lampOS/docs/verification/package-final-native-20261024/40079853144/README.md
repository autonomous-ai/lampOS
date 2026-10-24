# Skills carry native evidence

Run 40079853144, head `6cba8153250ad07f71db36ca66f49099a1f4d7e8`: dedicated **8 passed / 0 failed / 0 ignored / 2 filtered**. All 7,008 source hashes match the tested CI merge Git blobs and stayed unchanged; its parent is the authored head. Eight case inventories and matching child exits are in audit.json. Raw text files are byte-identical; binary archives and full source manifests are excluded with hashes. This audit checks retained receipts and source binding, without independently replaying every domain assertion. Ordinary ARM64 and Linux Clippy are separate gates. No physical output, updater or power-loss claim.

Additional read-only semantic audit verifies both package directions: 60
commands (32 successes, 28 intended refusals), eight actual positive package
archives, 40 unchanged install inputs, 80 snapshot file hashes and 12 exact
inventory comparisons. No executable was run by this audit. The API artifact
ZIP digest/size verifies and its archive matches the audited download. Strict
Linux Clippy **40079852895** is green; ordinary current-head ARM64 **40079852889** is now green:
**7,355 PASS plus one LEAK**, zero failed/unrun, 51 skipped. All original prepared-head failures/results remain separate.

Reviewer-two READ-OK `6cba81532` carries from the byte-identical prepared
Skills tree; parent is landed, and current source/gates are cleared. Integration
onto later base advances remains the train’s responsibility.
