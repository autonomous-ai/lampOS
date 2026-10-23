# Saved rollback order, 23 October 2026

Follow-up to #3173: save the validated old startup order before a switch,
including inactive changed units and paths through unchanged dependencies.
Restoration uses that saved order even when later installed unit text is
unreadable or has changed. It validates all journal members before excluding
an already-restored prefix. Cleanup keeps live build folders and their
records if it cannot read the installed inventory.

The test-first patch on unchanged production reports **8 passed / 6 failed**;
the identical patch on the fix passes **14 / 0**. Ignoring the saved order
alone restores the unreadable-inventory failure, **0 / 1**, and the source
was restored exactly before the full gate. Cases also cover interrupted
put-back, inactive units, 11 malformed-order forms, a field-absent legacy
journal and an empty unit order with live build folders to preserve.

Mac gate, Rust 1.96.0, on integration base `b3a1ece0b` plus this change:

```
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -q --locked --offline -p lamp-update --all-targets --message-format=short -- -D warnings
cargo +1.96.0 test -q --locked --offline -p lamp-update
```

All commands pass: **332 tests passed / 0 failed / 4 ignored**. The manual
fixture writer was also run separately, **1 / 0**, to keep a new dated
23 October state/journal pair beside the unchanged 21 October pair.
[Full commands, raw output, negative controls and source bindings](https://github.com/autonomous-ai/lampOS/blob/2a289f992/lampOS/docs/verification/updater-saved-order-20261023/README.md).
All 6,654 bound regular files stayed unchanged during the full gate.

The optional field is ignored by the previous reader. Legacy journals
retain graph reconstruction without rewriting their unknown fields.
Malformed present orders retain the journal and recovery prefix; malformed
JSON syntax keeps the existing set-aside behavior. Old journals can still
be blocked by unreadable or cyclic installed graphs. Saved-file reads,
restoration writes, systemd calls and health checks can still fail.

Source self-review found no P1/P2. Cross-crate searches find no outside
consumer of the changed journal or private ordering helpers. No new switch,
message, topic or unit line is added; person, privacy, forget and Voice
quiet handling are unchanged. The mixed-build documentation states both
directions, the default 3600-second quiet bound, and unresolved external,
socket, target, alias and drop-in dependency paths.

These are file/fake-systemd fixtures and existing process tests on a Mac.
They do not establish real systemd behavior, physical power-cut durability,
acoustic latency or signed-release acceptance. Official native ARM64 and
Linux Clippy are required separately on the PR head.
