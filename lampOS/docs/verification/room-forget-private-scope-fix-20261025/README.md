# Keep room-only forget out of private memory

A room entry can be corrected and its active correction adopted into a
person's private memory. Adoption retains the replacement link and removes
the room speaker marker. Previously, room-only `forget_said` could follow
that link and delete the private correction.

Each replacement destination must now still belong to `everyone`, in both
directions. The other-speaker barrier remains. Ordinary `forget` and adoption
retain their existing provenance behavior. This corrects the existing
operator without a schema, wire-protocol, switch or activation change.

Three public-protocol regressions cover an adopted correction surviving,
its later private versions surviving, and ordinary forget still removing
both linked versions. Source review found no new P1/P2 in the narrow fix.

## Gates and source identity

Rust 1.96.0 used ordinary profiles, one Cargo job and serial tests. Full gates
on `045236cb1`, based on `0ddd07377`, passed formatting, strict Memory/VoiceEval
all-target Clippy, workspace build, Memory **327/0/8** and VoiceEval
**623/0/0** (passed/failed/ignored). The preceding fixed source on
`86416a0d3` passed the same full counts separately.

The carry `50f90df1b` onto `bf39e4188` has an equal one-commit range-diff.
That upstream advance changes only Actions schedule replies and their tests;
1,127 Memory/VoiceEval runtime, test-dependency and configuration paths remain
identical to the full-suite source. Actions is a dependency of Voice's own
tests, but not of the Voice library used by Memory/VoiceEval.

On the new carry, the following fresh checks passed:

- `cargo fmt --all --check`.
- Strict all-target Clippy for Memory, VoiceEval and Actions.
- `cargo build --workspace`.
- Each of the three new Memory integration regressions: **1/0/0**.
- Actions' schedule integration group, including the clock reply: **19/0/0**.

The full-suite counts retain their 045/0ddd source; they are not another full
Mac execution on bf39. The final commit adds only this verification update
to the tested 50f runtime/test source. Published-head native ARM64 and Linux
Clippy remain required. Every owned group closed; source and index stayed
fixed during each gate. Only identified local incremental caches were
retired between closed runs; executable witnesses remain separate.

## Real socket regression

The [earlier reproduction](https://github.com/autonomous-ai/lampOS/blob/a9735799a8/lampOS/docs/verification/room-forget-private-boundary-20261025/README.md)
retains the actual old-program failure: six requests removed two entries
instead of one and changed the private count from one to zero. Five direct
SQL checks supported the proposed boundary before compiled Rust execution.

The current normal Memory executable was exercised once with twelve socket
calls across two sequential synthetic scenarios. Each forget removed one
room entry; the first private count stayed one and the repeated scenario's
private count stayed two. The calls took **57.72 ms and 61.70 ms**, below the
preselected 100 ms local target. The measured boundary is client socket
creation/connect through complete response and JSON decoding. First means
the first forget after fixture setup, not server startup.

The current executable SHA-256 is
`614b39dc044b93e363820ddf039e2acabcea5c1b4806f5a56b8f1e8d7627aa11`.
It was bound to the successful workspace build and stayed unchanged during
the protocol check; an independent private copy is retained. Earlier fixed
executables separately passed the same twelve-call check: 0c10bfe0f at
58.73/58.55 ms and 457e665d8 at 62.16/58.95 ms. Their different bytes were
measured independently; source equality did not substitute for a binary
check. The original failing call took 56.34 ms at the same boundary.

These small local observations demonstrate the deletion boundary, not a
latency improvement, acoustic responsiveness or lamp acceptance. No provider,
device or personal content was used. Only counts, times and build identity
are published; raw protocol text, stores and executables remain private.
