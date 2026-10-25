# Room-only forget must not cross adoption into private memory

A real local Memory protocol scenario reproduced an existing scope-boundary bug: `forget_said` removed two entries where only the one room entry was authorized, deleting the replacement already adopted into a private scope. The fix is written and independently source-reviewed, but its Rust gates are **unrun**. This receipt is not a new-fix, current-Main, ARM64 or hardware qualification.

## Valid trigger and contract

1. Remember room entry A with speaker X.
2. Remember replacement B, with A in `shown`, attributed to speaker Y.
3. Adopt active B into Y's private scope.
4. Confirm Y has one active kept entry.
5. Ask `forget_said` for X's room lines.
6. Read Y's kept entries again: the old program returns none and reports two removed.

The explicit `forget_said` contract in [memory-days.md](https://github.com/autonomous-ai/lampOS/blob/056a76132354f40f97eb5fbc519778c04f1ccb19/lampOS/docs/memory-days.md) says no person's own scope is reached. Adoption intentionally keeps the same global row and provenance links, changes its person/id and deletes its room-only speaker row. The old recursive walk restricts its initial seeds to the room, but not replacement destinations. With the speaker barrier removed by adoption, that walk can admit and erase the private row.

The narrow correction restricts both replacement destinations to entries still owned by `everyone`, preserving the other-speaker barrier. It does not sever adoption links or change ordinary `forget`, which is intentionally allowed to follow those links. Three protocol regressions cover the adopted correction, a longer private chain and unchanged ordinary-forget provenance. No schema, request field, switch or default changes.

## What actually ran

One isolated server scenario used a private relocated copy of the source-bound old Mac Memory executable, SHA recorded in summary.json, with a fresh temporary store and no model or bus. Its Memory runtime crates, local runtime dependencies and workspace manifest are byte-identical to public Main119's relevant source. The complete workspace differs in Voice's dependency edge, so whole-workspace feature/binary identity is not claimed. The original shared cache was read only; the retained copy's bytes match its actual prior compiler-output record. No compiler, VM, device or installed/user memory was used or changed.

Six protocol calls completed. The expected one private kept entry became zero, while `removed` was two instead of the room-only one. The old forget RPC took about 56.34 ms from client socket creation/connect through the complete response and JSON decode. This is one old-executable local observation, not a before/after latency comparison. The new code keeps the caller's remaining tool budget and adds no RPC, network work, wait or asynchronous task; new fixed-binary and hardware latency remain unverified.

The exact original and proposed recursive SQL strings also ran on five minimal SQLite metadata cases. The proposed query satisfies all five room/private-boundary and existing other-speaker/room-history expectations; the original violates three. These are direct SQL checks, not compiled Rust or service gates. Standalone pinned Rustfmt and diff checks pass. The independent four-file source review found no concrete P1/P2 within its scope.

The old process group is gone and its copied executable is unchanged. Raw protocol text, temporary databases, controller code and executable remain private. Canonical Mac/Linux checks, full Memory and dependent tests, current native CI and readers are still required before the fix is pushed as code or called ready. Existing failures and all separate memory-activation/physical holds remain.
