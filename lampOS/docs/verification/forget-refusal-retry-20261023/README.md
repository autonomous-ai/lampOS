# Follow-up forget refusal and retry, 23 October 2026

Preparatory second read of PR #3130 at
`8dd86272e9f9bfec63e1c62df2d6a06c714e75b3`, stacked on #3127's
`206665f1367ac5922e8c7925be5ff2f7311c3381`. This is a scoped finding,
not final qualification of the dependency or later train.

The real DaysClient is given the existing follow-up fixture, whose shown
entry is id 12. A bounded socket store explicitly refuses its first
`forget(id=12)` with `{ok:false,error:"io"}`. The same person's immediate
bare retry sends `forget_last(kind=kept)` instead. The fixture store then
removes unrelated id 30, and the real client reports `forgotten:true`.
The counter allows retry of id 12 or a safe refusal with no second request.
It fails only when unrelated memory is selected.

On the Mac, the one added test fails **0 passed / 1 failed**, exit 101,
with all earlier setup/first-refusal assertions passing. See `counter.txt`
for both real outbound requests and the original panic. `result.json`
retains exact argv, source hashes and output digest. All 6,520 tracked
inputs stayed unchanged during the command; `remember.rs` was restored
byte for byte afterward. No production change or full crate gate was made.

The source clears `context.just_asked` before the store acknowledges the
write (`remember.rs:4955-4963`). After the definite refusal returns through
5094-5101, the retry falls through at 4970 to the ordinary last-item request.
Retaining the explicit target until acknowledgment, or remembering a safe
refusal that cannot turn into last-item deletion, would preserve retry intent.
The author owns the correction and its concurrency/expiry behavior.

This uses actual DaysClient dispatch and a simulated store response. It does
not claim a real Memory daemon, Engine, lamp, personal data, or acoustic run.
