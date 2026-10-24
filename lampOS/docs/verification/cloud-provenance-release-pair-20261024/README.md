# Published Cloud readers of installed Skills provenance

This is the narrow #2809 continuation for the consumer excluded by #3330:
actual published `lamp-cloud` reads provenance written by actual published
`lamp-skills` package/install commands. Official native run
[40070585559](https://github.com/autonomous-ai/lampOS/actions/runs/40070585559)
executed **2 passed / 0 failed / 0 ignored** on original head `2ad786f91`.
Its isolated container was removed and source/input hashes stayed unchanged.
The [retained audit](https://github.com/autonomous-ai/lampOS/tree/e1da6c247/lampOS/docs/verification/cloud-provenance-native-20261024)
checks both directions, 14 owned exits, 18 MQTT replies, 144 snapshot file
hashes and four actual archive hashes. This documentation correction changes
no fixture or workflow; new-head CI results remain separate.

Two directions use releases 6408 and 6498. An actual Skills installer writes
an installable package, the opposite released Cloud answers `skills.list`,
the replacement installer updates the package, and the original Cloud reads
that update. Both old and replacement Cloud readers must expose the exact
installed package version and archive SHA-256. Generic file snapshots are
additional evidence, not a substitute for the released consumer.

The Cloud payloads differ: release 6408 has 10,802,168 bytes and SHA-256
`9f3abad362555ad11177bdf41706e91d740bca8689928a8555c4ce3f257fc664`;
release 6498 has 10,804,480 bytes and SHA-256
`6d4d566d837dad9df137b475557f330deb62a84405128b879686368a9de30bf4`.
The relevant Cloud provenance parser and Skills package implementation are
identical at the two release source references. This qualifies the exact
shipped artifacts, not changed parser implementations.

Cloud has no offline `skills.list` CLI or install-directory override.
Therefore its read is exercised through a loopback-only MQTT-v5 peer in a
fresh network-disabled native ARM64 container. Its fixed `/var/lib` paths
are a private tmpfs, never host lamp directories. Dummy credentials use a
loopback broker and an empty backend URL; the released ping implementation
makes no HTTP request for that URL. No real account, broker, provider,
actuator, service manager or device participates. Every process uses a
cleared environment and is owned and reaped before replacement or cleanup.

Synthetic malformed and unknown-field metadata controls are distinct from
positive released writes. They must leave a valid loaded skill visible but
with no reported package hash. The installed provenance reader does not
validate the manifest format number, so no format-number refusal is claimed.
Source, downloaded payload, installed-file and input hashes accompany the
actual MQTT replies and process exits.

No Store signature acceptance, publisher trust, updater installation, cold
boot, power-loss durability, physical output or live account behavior is
qualified. Runtime defaults and interfaces are unchanged. The explicitly
ignored native cases require the dedicated official workflow; ordinary
ignored results are not evidence that these cases executed.

Host validation with Rust 1.96.0 passed: `cargo fmt --all --check`,
`cargo clippy --locked --offline -p lamp-cloud --all-targets -- -D warnings`,
and `cargo test --locked --offline -p lamp-cloud`: **196 passed / 0 failed /
4 ignored**. Both new release cases remain ignored on the Mac. YAML parsing,
all nine extracted shell blocks (`bash -n`) and actionlint passed; shellcheck
was unavailable. These static checks do not qualify native execution.
