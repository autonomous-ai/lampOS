# Common current carry — Voice/Update source audit

No concrete P1/P2 found in this bounded composition review. This is internal source review, not a build result or external reviewer signoff.

Audited the staged Voice/Update/product sources in `fourth/common-current-20261024`, HEAD `1d915d44c97fcdd980a8a5d9c9e4fecc9570d746`, with old Common `4d634474e740bd093430ac5ab62801fa9629f014` and its base `be16c3327524ccef8a1908a3b8ebf40c16efba8a`. Root owns Eyes main/Cargo conflict resolution; no full candidate-tree identity is claimed here.

## Composition evidence

- All upstream added/removed line sequences across 29 changed Voice/Update/product/4ace paths are exactly retained. All Common signed-line multisets across its 22 paths in this scope are retained. The only own-diff ordering variation is Git's alignment of unchanged braces/deletion placement in worker.rs; the actual sensitive functions are checked separately below.
- All 46 files in the union were byte-equal to their stage-0 index blobs when audited. `source-receipt.json` records each blob, mode and SHA-256.
- Nine authority/test files remain byte-exact to old Common: memory_scope.rs, remember.rs, services.rs, private_context and its two child modules, lookaway_authority_tests, pending_context, and kept_name_privacy_tests.
- Seven additional regions remain byte-exact to old Common: worker `private_speaker_permission`, `memory_context`, `memory_disclosure`, `memory_whose`, `memory_read_permission`; SpeakingFaces `positive_speaker`; and the typed KnownSpeakerSource definition/parser region.

## Behavior checked

1. **Same-speaker carry is admission, not private authority.** `worker.rs:6195–6241` records a 20-second conversation carry; `6426–6463` checks its voice/face evidence. The accepted eight-second uncertain-print follow-up exception does not create or change a ScopeLease. The private source matcher at `3926–4008` still requires this request's sealed original face, matching person and epoch, and validates the original proof expiry through `memory_scope.rs:805–817`. No speaker proof is manufactured from the carry flag or voice similarity.
2. **Stop/privacy boundaries survive.** The new actual Engine `Did::Stopped` notification is at `second_provider.rs:2982–2987`, before the new line is judged; worker consumes it at `5843`. `worker.rs:5657–5669` clears carry and cached speaking evidence on privacy. Existing exhaustive `Did` matches include the new variant; no unresolved match arm was found. The same-speaker tests include stop, privacy and thread forget, other-face takeover, turned-away caller, uncertain/late prints, and the 20-second start boundary.
3. **Turned-face policy does not weaken the positive-proof reader.** New per-face `turned_unread` storage at `speaking_faces.rs:228–234` and `turned_not_seen_speaking` at `450–466` are used by the public attention rule (`worker.rs:6063–6089`). They do not enter `positive_speaker` (`speaking_faces.rs:292–335`) or the typed source parser (`636–790`). An initial Unknown/no source still cannot grant private context. Existing Absent continuation and matching Checking pause remain bounded by the original grant; changed epoch/identity, stale/error, revocation and expiry still end it. Root must separately preserve the Eyes producer side when resolving main.rs.
4. **Product head-touch checks remain whole.** Current product Voice has `LAMP_HEAD_TOUCH`, `LAMP_RESTORED_MUTE` and `--controls-mute`; Body has `LAMP_HEAD_TOUCH`, Expression retains `--head-touch`, and product pins have no legacy_head. Update's new preflight (`install.rs:1199–1202`, `head_touch.rs:329–362`) and Body reader/bus checks (`head_touch.rs:219–276`, `310–318`) are current-main exact. Common's 4ace source/authority census (`unit.rs:506–565`) is retained beside main's head-touch exception (`unit.rs:320–330`). The two new attention switches are not silently enabled in the checked product/4ace units by this carry.

## Required named coverage inside Root's full gates

`gate-selectors.json` gives exact runnable argv and source identities. These are inventory counts, not passing-test claims; do not repeat them after full package tests merely to increase counts.

| Filter / target | Authored controls |
| --- | ---: |
| Voice `speaking_faces::tests::` | 15 |
| Voice `memory_scope::tests::` | 34 |
| Voice `second_provider::tests::private_context::` (includes short_line and carried_speaker) | 30 |
| Voice `second_provider::worker::lookaway_authority_tests::` | 19 |
| Voice `second_provider::worker::pending_context::` | 2 |
| Voice `second_provider::worker::kept_name_callback::privacy::` | 9 |
| New upstream worker same-speaker carry + turned-face controls | 16 |
| Engine `second_provider::tests::the_first_line_after_a_stop_tells_the_rule_of_it` | 1 |
| New same_voice controls / room_told controls | 2 / 2 |
| Update `--lib unit::tests::` | 10 |
| Update `--test install` (already contains installed-reader and head-touch census) | 90 |
| Update `--test round head_touch::` | 34 (28 retained + 6 Body controls) |
| Update `--test it scratch_paths::` | 2 |

The entire Update library has 123 authored tests, including two explicit ignores. New `tests/release_pair.rs` has two explicitly ignored old-binary compatibility tests requiring pinned ARM64 artifacts and an owned offline Docker context; ordinary package gates do not execute those, and this audit does not authorize `--ignored` or Docker work.

Root chose full Voice, Eyes and Update on both hosts with formatting, strict workspace/all-target Clippy and workspace build. That covers these selections without a separate repeated focused run; actual executed/ignored counts must come from the terminal logs.

No source, index, ref or mtime edits; no Cargo/target use, external posts, device access or physical/acoustic/privacy qualification. Existing tests separately exercise public admission and private Engine/final-output guards; this read does not claim a new combined physical carry/recognition test.
