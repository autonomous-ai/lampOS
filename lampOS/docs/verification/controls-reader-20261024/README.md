# Independent Controls pair review

Exact authored head `27b3695f0235bc7a721dfb216cd21203c249491d` passes Mac
formatting, strict all-target Controls Clippy and full serial Controls tests:
**509 passed, 0 failed, 3 ignored**, all 6,994 source hashes unchanged.
Exact source-only Linux formatting passes in an owned offline read-only
container; 6,994 hashes unchanged and removal verified. No local ARM build/test.

Independent audit of official dedicated run **40075828326** verifies
**2/0/0 ignored/1 filtered**, 36 matching start/reap receipts, 24 successful
commands and 12 intentional refusals. Each direction has nine byte-identical
47-frame replays with exactly two expected slide events; all three written
pins files agree with the expected complete JSON. Stream hashes, original
deadlines, readonly payload pins, clean terminal/source/cleanup results and
all eight authored files matching tested merge `76ad068a7` are checked.
Source maps remain unchanged across 10,347 tracked paths; this review does
not independently compare every captured base blob to Git. Transient
per-command data preservation additionally relies on executed fixture assertions.
Ordinary ARM64 remains a separate pending gate at this publication.

No new P1/P2 found. The fixture owns a private process group per command,
requires reap and absent group before success, and retains bounded failure
cleanup. No authored hanging-child cleanup regression or live output disk
quota is present; these remain P3 limits. Payloads are identical, unsigned
released binaries. No hardware calibration, acquisition/feedback latency,
signature trust, updater transaction or whole-lamp acceptance is claimed.
