# Sentinel008 Mac qualification

PR #3543 head `7cafe341a62cda2db4095821db40436784d8ffd7` on
`008e7a9746e7ddd58e951f8ea919a7d8beebb5ef` passed the complete actual Mac
`session_reader::tests::` module **4/0/0**, once serial. The supervised command
took **1.583 s**; libtest reported 1.00 s. The actual Voice library inventory
contained 3,822 cases; 3,818 cases outside this module were filtered out.
This is a module runtime result, not a full Voice suite result.

Formatting, strict all-targets Clippy over the actual twelve-package Voice
reverse closure, workspace build, and Voice library compilation passed. Only
`lamp-voice` was cleaned; only its strict artifacts were required `fresh:false`.
Other local artifacts retain their observed freshness. Workspace programs were
built before the test executable was compiled and frozen.

All 10,674 tracked source files, modes and index remained unchanged. Before
qualification, the canonical qualification alias matched the preserved original
Sentinel source exactly except for its branch label, at the official offered
SHA. All 207 prior sealed witnesses plus the new independent Voice library copy
remained intact; the current executable hash also matched before and after
runtime. All owned process groups closed; no retry occurred. The official
`7cafe` branch was pushed, without pushing the qualification alias.

The changed Linux-only case
`session_reader::tests::linux::replaced_record_never_changes_an_outside_sentinel`
was absent from the Mac inventory and unrun here. Original
[ARM64 40371123630](https://github.com/autonomous-ai/lampOS/actions/runs/40371123630)
was running at the reported checkpoint;
[Linux Clippy 40371123472](https://github.com/autonomous-ai/lampOS/actions/runs/40371123472)
and [general CI 40371114033](https://github.com/autonomous-ai/lampOS/actions/runs/40371114033)
reported success, pending preview-source and log audit. This packet claims no
new Linux runtime result.

The later equal-patch `8edc8aa02` carry on Main `978cb0e98` is source-only and
is not qualified by this run. Earlier 6588 native/690 source evidence retains
its original scope; [the prior native proof](../tests-sentinel-native-overlay-20261026/)
is unchanged. Sanitized counts, source/executable bindings and exported private
receipt hashes are in [audit.json](audit.json); no executable is published.
