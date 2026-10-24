# First Settings native attempt and workflow repair

Original official run **40077517657**, authored `990919c8d`, failed
**0 passed / 2 failed / 3 filtered**. The workflow mistakenly supplied
`LAMP_COMPAT_CONTROLS_*`; the fixture required `LAMP_COMPAT_SETTINGS_*` and
refused before either server or catalog CLI started. Independent retained
transcript audit counts **zero child spawns**. No actual-artifact state/RPC
qualification ran. Source comparison succeeds; cleanup validation refuses
because the two required terminal results do not exist. Neither is relabelled.
The full workflow log and original artifact archive remain private; selected
raw test output and archive hash are retained here.

Correction `8f5c600c1` changes only those two workflow environment names.
All 6,994 captured lampOS hashes still match the earlier passing Mac/source-only
Linux gates, explicitly bound rather than claimed reruns. YAML and eight shell
blocks are rechecked, and the fixture input names are checked against the
workflow. Fresh official pair/ordinary ARM64/Linux Clippy gates remain required.
