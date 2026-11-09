# A late services receipt restores payload after admitted forget

This is an intentionally failing Engine regression, not a repair or a
merge-ready branch. It extends the send/forget integration HOLD on #5913.

## Source and boundary

The test is appended directly to context carry
`69c39c4f82f5148d060afffd515a9939288b4e26`, based on
`9d6ec47fb804b985811af0992acd5dbcf46af6d2`. It does not depend on the
provider/playback speech extension or the earlier two added late-receipt
controls. Production code is unchanged.

The spoken request explicitly asks to send and forget the face. The actual
Engine dispatches the send, is cut, then admits the face forget on the resumed
request. Only after the forget runner is entered does the fixture release the
original send. That old route is fenced: its watch returns `unavailable`, while
the real services receipt store records `Told(sent:true)` for its numeric attempt.
The fixture uses `services::keep_send_for_test` to model completion; it does not
exercise a service transport or account.

A second cut interrupts deletion. The next resumed request explicitly tries the
send again. Before checking privacy, the test requires exactly one send dispatch
and the definite already-sent refusal. Those assertions pass. The original watch
cannot supply success, so the successful services fallback is exercised.

The privacy assertion then fails on the synthetic private call-ID canary. The
resumed requests also visibly contain the synthetic argument and result canaries,
and the old note promising that the call and result are attached. The later
canary assertions are not reached. No real private content is used.

A genuine late receipt may update the fixed accounting status. The test permits
an already-redacted `Told(sent:true)` value; it does not require status to remain
unknown. Numeric attempt identity and duplicate-send protection must survive
redaction. Backdating the retained wait's timestamp models expiry; this is not a
measurement of the real service deadline.

## Native execution

Mac, Rust 1.96.0, one retained target directory, two build jobs, incremental off,
dev/test debug info off, short owned `TMPDIR` with member group. Commands run from
`lampOS/`:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 test --offline -q -p lamp-voice --lib second_provider::tests::sent_write::a_services_fallback_after_admitted_forget_keeps_status_without_private_payload -- --exact --nocapture --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --lib second_provider::tests::sent_write:: -- --test-threads=1
cargo +1.96.0 clippy --offline -q -p lamp-voice --all-targets -- -D warnings
```

- Formatting passes.
- Exact regression: zero passed, one failed, exit 101; test body 0.03 s.
- Full sent-write module: eight passed, one failed, exit 101; 2.73 s. The new
  regression is the sole failure.
- Strict all-target Voice Clippy passes, exit 0.
- Documentation checker: 192 documents, 1,899 links, zero broken. Verification
  README files are outside that checker's counted set; this receipt adds no links.

Raw synthetic diagnostics and command receipts are retained locally under
`/tmp/fourth-fallback-current-*`. No whole-workspace, new ARM64, physical deletion,
acoustic, or service-network acceptance is claimed for this fixture. The separate
required CI run on the unmodified carry does not include this new test.
