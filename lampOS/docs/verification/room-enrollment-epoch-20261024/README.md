# Late room publication after enrollment invalidation

Test-only follow-up to #3344's direct epoch-interleaving coverage P3. The
approved parent head stays unchanged; no runtime, product setting, protocol
or behavior changes here.

Both cases obtain a real Engine microphone proof and the original live
request lease, then retain that original line as a delayed room task would:

- `remember_speaker_room_publish_after_forget_cannot_revive_its_original_proof`
  calls the real enrollment forget path between publications.
- `remember_speaker_room_publish_after_privacy_reopen_cannot_revive_its_original_proof`
  closes and reopens the Engine's camera privacy authority between them.

The lease must still be current after invalidation, distinguishing these
cases from the existing stopped-lease control. The first publication's
proof disappears; passing the task's retained proof through
`publish_room_line` cannot expose it again through the line cache. The
retained pre-invalidation line also
returns NOT_SEEN at the guarded remember entry before transport, with no
socket service. Each fixture uses a distinct request number in the shared
line cache.

These are ordered interleavings, without sleeps to create a race. They do
not execute the asynchronous known-file read, physical privacy switch,
camera, or a concurrent filesystem stall. There is no latency measurement
or product activation. They test a safety boundary already implemented by
the parent rather than repair a newly demonstrated runtime defect.

Validation: focused enrollment **26/0**, formatting and strict all-target
Voice Clippy pass on unchanged test source. Full serial Voice **3,995 passed,
0 failed, 21 ignored** (library 3,714/0/15; integration 270/0), and workspace
build pass. No actual released-pair execution is inferred from ignored tests. An earlier diagnostic sandbox run had
Unix-socket permission failures followed by poisoned-lock failures; it is
retained separately and is not the qualified source-bound result.
