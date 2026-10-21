# second-P8: distinguish a requested-audio metadata refusal

Second, 2026-10-22 01:30 +07. Proposed and started on [Queue #2602](https://github.com/autonomous-ai/lampOS/issues/2602#issuecomment-6323472887)
under the owner's instruction to take the next slice while CI/review proceeds.
The first builder's named routing is requested. The separate branch
`second/audio-metadata-diagnostics` starts at #1757's qualified
`0a878cd4d9d44867b2971e70c118244f0c21ad38`; #1757's head is unchanged.

## Problem and scope

The retained packaging refusal `audio file changed while checking` cannot
identify whether the description, manifest or recording changed, or which
comparison failed. [The earlier evidence review](../verification/requested-audio-metadata-review-20261022/README.md)
establishes that the refusal precedes archive creation, but not its cause.
This slice improves the next observation; it does not claim to fix that cause.

Change only Skills' `requested_audio.rs`, its embedded tests, its existing
behavior documentation and a verification folder. Keep every refusal and all
content hashing, identity checks, byte bounds and read accounting. Add no
retry, timestamp tolerance, new logging, synchronization or path disclosure.

On file-stability failure, keep the existing prefix and add fixed labels for
role (description, manifest, recording), phase (after-read, final), changed
opened/listed stamp fields and the original/opened/listed numeric stamps.
Format only on failure from already obtained metadata. Preserve separate
parent-directory, initial-open and content-hash refusals.

## Latency and validation target

There must be no additional filesystem operation, hash, successful-path
formatting or authority-lock work. The failure message must fit in 1,536 bytes
including worst-case integer formatting, with no paths, filenames, hashes or
contents. This is a bound for diagnostic construction, not a new claim about
physical playback latency.

Use the existing descriptor/path replacement fixture, parent replacement
fixture and deterministic stamp injection. Cover comparison sides and fields,
role/phase discrimination, absence of path/content markers and the numeric
message bound. Keep equal-stamp content mutation and full-read accounting
assertions. The original full scan remains synchronous and outside the
authority mutex; Voice's playback and authorization paths need no edit.

Mac qualification will be fmt, strict Clippy and Skills tests only. Native
qualification belongs to the PR workflow. No device or sound action is part
of this slice. A transient scan-only failure still becomes a boolean and may
not be journaled; no new runtime journal output is proposed.
