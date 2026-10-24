# Keep-alive mock framing

Head `5053a00c3bdd517093da5363a7a647ec5ba99d90`, base `22b753df5`.
Only the two test mock readers and fixture documentation change. They now
accumulate through CRLF-CRLF and answer complete header-only GET requests.
The existing TTS mock framing pattern is reused, with a 4,096-byte header cap.
All seven original assertion expressions are byte-exact. Existing 200 ms,
600 ms, 10 s, 100 ms and 50 ms values are preserved; runtime code is unchanged.

Mac fmt, strict Voice all-targets Clippy and full serial Voice pass:
**4,005 passed / 0 failed / 21 ignored**. Both existing keep-alive cases and
the new split-header case pass once. All tracked source hashes are unchanged.
Exactly 26 package names were invalidated; 25 path-package identities were
observed, with fresh workspace artifacts and explicit unchanged vendor reuse.
Exact-head source-only offline Linux fmt passes: 7,023 regular file hashes
unchanged, owned container removed. No full workspace or local ARM gate.

The new control observes a server read of the incomplete header before
sending the final separator, then reads through EOF after a write half-close.
It requires one complete response. One isolated modeled old-per-read mock
fails that assertion, expected exit 101: two complete responses observed,
one expected. The private source is restored and its worktree clean. This
is a deterministic fixture mechanism control, not a replay or proven cause
of #3380's historical three-versus-two connection count. No normal case was
retried and no duration or count assertion was relaxed.

Executed positive test binaries were hashed before invalidation. Compiler,
source, command and negative-control metadata are retained; private home
paths are redacted with both digests. No executable/archive or configuration
state tree is exported. Official PR checks and a reader remain required.
No production policy, physical lamp, provider or acoustic claim.
