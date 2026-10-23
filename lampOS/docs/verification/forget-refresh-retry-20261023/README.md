# A refused follow-up forget after the next-turn note refresh

Read of public PR #3130 at `a1b8034bfed33e723b22e3779f98f81ea59c3511`,
23 October 2026. The correction's two direct retry tests pass **2 / 0**,
and its current-Memory shared-evidence test passes **1 / 0** on Darwin arm64,
Rust 1.96.0, with `RUSTC_WRAPPER` empty. The complete outputs are retained.

A remaining P2 is reproduced with the production DaysClient and real Memory
socket, protocol and SQLite store. The test fixture injects exactly one
pre-dispatch `forget` refusal (`ok:false,error:io`); every other request uses
the real `days_protocol::answer`.

1. Seed a follow-up observation and an unrelated kept row sharing evidence.
2. Open session 1 and mark the real follow-up asked. The first bare forget
   probes capability, sends its observation ID, and receives the injected
   definite refusal. The test asserts both the ID and refusal.
3. Finish that reply and adopt a successful same-person session-2 note via
   `DaysClient::told_for_session_within`. The refreshed snapshot must still
   contain the unrelated keep.
4. Immediately retry the bare forget. The client sends `forget_last`, the
   real store deletes unrelated kept ID 2, and the client reports
   `forgotten:true`. The unrelated-row assertion fails: **0 passed / 1 failed**.

This refresh is the second route's existing production path: an attempted
forget calls `context.changing()` (`remember.rs:5075`); `Sessions::tidy`
drops the now-stale note (`second_provider/worker.rs:6668–6676`), and
`ask_if_due` increments the session (`6845–6852`). `NoteRead::adopt`
(`remember.rs:3022`) calls `replace_session`, whose line 2760 clears
`just_asked`. Keeping the slot only inside the immediate call does not
preserve it across that refresh. The original direct retry tests omit it.

```
cargo +1.96.0 test -q --locked --offline -p lamp-voice --lib a_refused_forget -- --nocapture --test-threads=2
cargo +1.96.0 test -q --locked --offline -p lamp-memory --test it voice::a_follow_up_forget_preserves_an_unrelated_keep_with_shared_evidence -- --exact --nocapture
cargo +1.96.0 test -q --locked --offline -p lamp-memory --test it voice::a_refused_follow_up_forget_survives_the_next_turn_note_refresh -- --exact --nocapture
```

`refresh-counter.patch` adds only test fixture plumbing and the counter.
Each command binds all **6,520** regular source/workflow files, unchanged
throughout execution. The test patch was removed byte for byte and the peer
checkout restored clean; see `refresh-restoration.json`. Exact commands,
exit codes, raw-output hashes and input manifests accompany the raw logs.

The counter invokes the worker's next-session read through its public client
API; it does not run the complete worker or a model. No lamp, played audio,
transport EOF, physical cut or real release was exercised. The older-Memory
compatibility change is a separate path and is not a cause of this failure.
