# Controls and Expression product contract, 24 October 2026

Static item 6 audit for Monday release readiness. Third holds no lamp. No SSH, install, service restart, binary execution, signature-trust verification, physical calibration or latency measurement was performed.

## Source and published text binding

Release `lampos-lamp-voice-6610` names source `83da28dc7`. Nine sampled product/settings/installer/test files are byte-identical to fetched lamp-voice `9bd23ebf0`; exact hashes are in audit.json. Seven downloaded text assets (four lamp-specific units, two manifests and SHA256SUMS) match public asset digest and size. All four unit files equal their release-source files. Public hashes establish byte identity, not trusted signatures or a live lamp state. No payload binary was downloaded or executed in this audit.

## Product contract

- Controls: `current/lamp-controls serve --touch`, shared bus, explicit `/etc/lampOS/controls/controls.json` and `controls-pins.json`, privacy switch on, RR priority 30. CPUSchedulingResetOnFork is absent; the unit explicitly intends the strip child to retain RR30. No scheduling policy change is proposed.
- Expression: `current/lamp-expression serve --enable --head-touch`, shared bus, `/etc/lampOS/expression/expression.json`, system zone, kernel clock, rest requests on, Nice 5.
- Installer PRODUCT embeds both product units. SETTINGS embeds the three matching settings paths. Existing locally modified settings and unrelated drop-ins are preserved; a new binary alone does not establish that installed units or settings were refreshed.
- Product pins name primary/reset and four head candidate lines, active high with pull down. No per-lamp head baseline is shipped; the base strip waits and legacy head publication is off. The existing product-pins/unit integration test is included in #3369's fresh-target Controls 509/0/3 gate. Source equality is not a new hardware test.
- Surround is an optional carried binary with no product unit, explicitly documented as not run by lamps. Its published-pair test does not establish product activation; this is not a missing-unit fix claim.

## Published lamp-specific templates are not the product units

The release's four unit assets are named for lamp-4ace and lamp-52e6. Both use pinned binary directories rather than product current links. The 4ace Controls template declares RR30 and touch; its Expression template declares head touch. The 52e6 Controls template has no touch flag or RR stanza, and its Expression template has no head-touch flag and uses a fixed UTC offset. These are verified repository/template differences, not observations of either lamp's effective service. They do not justify changing another builder's unit or asserting that a live lamp lacks the feature.

## Holder readback and physical acceptance still required

The holder should record only the relevant effective unit/drop-in fields: Controls policy/priority/reset-on-fork, argv and privacy switch; Expression argv/rest switch; setup-marker status; current executable target/build; setting paths, modes and digests. Compare the actual process argv and scheduler with those effective definitions. Keep credentials, names and personal catalogue content out of the record. Record whether these are product units or lamp-specific definitions and whether install refreshed them; do not infer either from a successful binary update.

Do not overwrite per-lamp calibration to force an equality check. First and the holder retain #3276's touch hand sequence and action-to-feedback measurements (tap, hold to sleep, wake, quiet tap response, killed-listener one-tap reopening), plus privacy timing and load behavior. Enrollment/first-name checks belong to their separate sheets. Second retains #2809's attended signed-update/put-back run, with Migrate's trust input and the holder window. This static audit closes none of those physical gates.
