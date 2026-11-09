# Additional provider cancellation controls

These tests address the remaining regression coverage requested on #5856.
They add no production behavior. The provider-only candidate starts at
`eebeb8b23cc1a9b1a27ff10c0bfe4d3742c1eb9c`, directly on `a2260fd1b85`, with the
278-line appended test patch. Its existing test prefix and production file
remain byte-identical to that parent.

## Provider-only qualification

Mac, Rust 1.96.0, short owned TMPDIR `/tmp/A`, one retained target, two build
jobs, incremental and dev/test debug info off. These stages all passed in
order, with production/test source hashes checked before and after each:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --offline -q --workspace --all-targets -- -D warnings
cargo +1.96.0 test --offline -q -p lamp-voice --lib second_provider::tests::forget_speech:: -- --nocapture --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --lib second_provider::tests::held_forget:: -- --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice -- --test-threads=2
cargo +1.96.0 build --offline -q --workspace
```

The focused filters passed eleven and twenty-six cases. The full Voice package
passed 5,762 tests with twenty-two ignored and zero failures: library 5,472
passed/sixteen ignored, binary one passed, integrations 279 passed/six ignored,
examples ten passed, package docs zero cases. Focused results are not added
again to that package total. The full command took 774.512 s; workspace build
passed in 118.337 s. The documentation checker passed 192 documents/1,898 local
links/zero broken; verification READMEs are outside its counted set.

Source SHA-256 values:

- Production `second_provider.rs`: `3f327ca93a3b061b370f46653249f72bd68c3cff632b0f98b06b51f5ae86b8f4`.
- Tests `forget_speech.rs`: `7ff8f342f416a06a610267ade4d55ec00f9f3375a185ae123acdc582b4fe01cc`.

Receipts and logs remain under `/tmp/fourth-provider-only-controls-*`.
Current-head Linux/ARM validation and review remain separate. The following
counterfactuals and diagnostic failures were executed on the earlier combined
provider/playback candidate `7a27d3b4fb9235e9a09f32d10f4ebe1b53c94fc0`, also
based on `a2260fd1b85`; they are not relabeled provider-only or acoustic tests.

## What the tests check

- Two synthetic private sentences precede one ordinary forget. TTS for the first
  remains blocked while the second is queued. Both TTS calls must have started;
  the second sentence must not reach saying. Forget must release the blocked
  waiter, and only fresh audio may complete on the same request.
- An unasked ordinary forget, or an explicitly requested forget-everything, first
  produces its actual confirmation question. The same known person's plain yes
  admits the held call. The runner verifies the current person scope, and the
  everything marker where applicable. It checks that the yes-request generation
  is invalid synchronously at runner entry, before its future waits for I/O.
  Synthetic queued provider text/audio for that request are discarded, and fresh
  same-request speech remains possible after the runner is released.

The second test accumulates transcript chunks and compares the whole question.
Its initial version incorrectly required the long everything question in one
chunk; that run had ten passes and one fixture failure before the yes. The
corrected unchanged-production run has eleven passes, zero failures, 0.05 s.
A source review also corrected the known-person setup order: enabling the memory
camera clears FaceCache, so the observation is populated afterward.

## Combined-source controls and diagnostics

Mac, Rust 1.96.0, short owned TMPDIR, one retained target, two build jobs,
incremental and dev/test debug info off. From `lampOS/`:

```sh
cargo +1.96.0 test --offline -q -p lamp-voice --lib second_provider::tests::forget_speech:: -- --nocapture --test-threads=1
```

The corrected run passes all eleven tests. Two counterfactuals independently
omit one admission `queue.forget()` call:

- Without the ordinary cut, the queued-second-sentence case fails waiting on the
  old stalled speech service: zero passed, one failed, exit 101, 0.01 s body time.
- Without the held cut, the confirmation case fails the synchronous runner-entry
  generation assertion, followed by the admission-wait timeout: zero passed,
  one failed, exit 101, 3.04 s. This stops in its ordinary-forget iteration; the
  everything iteration is covered by the positive run, not a separate negative
  execution.

Production was restored byte-for-byte between and after these checks; its diff
against HEAD is empty. The whole documentation checker passes: 192 documents,
1,900 local links, zero broken. Verification READMEs are outside its counted set.
The final restored-source gate ran these commands in order:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --offline -q --workspace --all-targets -- -D warnings
cargo +1.96.0 test --offline -q -p lamp-voice --lib second_provider::tests::forget_speech:: -- --nocapture --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --lib second_provider::tests::held_forget:: -- --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice -- --test-threads=2
```

Formatting and strict workspace Clippy passed. The focused filters passed eleven
forget-speech cases and twenty-six held-forget cases. The full Voice unit group
finished with 5,485 passed, two failed, sixteen ignored (exit 101; 693.06 s test
body, 753.102 s command). Both failures were request-count assertions in
`chat_second_try`: `off_no_second_try_goes` and
`the_first_answers_in_time_and_no_second_try_goes` each observed two requests
instead of one. Their cause remains unconfirmed. The controller stopped at this
failure; the later integration/doc tests and workspace build were not run.
The production and test source hashes remained unchanged through every stage.
This is not a passing full-package gate.

Both failed cases then passed once individually on unchanged source with
`cargo +1.96.0 test --offline -q -p lamp-voice --lib <exact-case> -- --exact --nocapture --test-threads=1`:
`second_provider::tests::chat_second_try::off_no_second_try_goes` (one passed,
3.01 s body) and
`second_provider::tests::chat_second_try::the_first_answers_in_time_and_no_second_try_goes`
(one passed, 1.26 s body). These results do not establish why the full run failed.
The separate serial full-package follow-up also stopped in the unit group:
5,486 passed, one failed, sixteen ignored (972.94 s test body; 1,031.416 s command).
Its sole failure was
`make_skill_local::tests::a_skill_the_check_refuses_goes_back_to_the_model_once`:
the local mock thread asserted at line 671 on an empty request body, then the
model correction failed to connect and line 950 received null instead of the
expected skill name. The earlier two retry cases passed in this serial run.
The failed request's header was not captured, so its origin remains unknown.

That skill case then passed once individually on unchanged source (one passed,
0.85 s body, 1.383 s command). Separate previously unrun stages then passed on unchanged source:

```sh
cargo +1.96.0 test --offline -q -p lamp-voice --test '*' -- --test-threads=1
cargo +1.96.0 test --offline -q -p lamp-voice --doc
cargo +1.96.0 build --offline -q --workspace
cargo +1.96.0 test --offline -q -p lamp-voice --bins --example render_line_second --example stop_words_stt_replay -- --test-threads=1
```

Integration tests: 279 passed, six ignored, zero failed; the package doc target
contains zero tests. Workspace build passed. Binary/examples: eleven passed,
zero failed or ignored. These separate successes do not replace either failed
full-unit run. Both full-unit failures remain recorded; the isolated
passes do not establish an uninterrupted green full-package gate.

An independent 30-second loopback diagnostic bound a fresh ephemeral listener
and started no client. It received one unsolicited `GET` / `HTTP/1.1` request
(179 bytes) from loopback after 2.278 s, then closed normally after 30.003 s.
This establishes ambient localhost HTTP traffic in this environment. The failing
Voice fixture counts parsed requests without checking their method/path, but its
failed assertions did not print request contents; the diagnostic therefore does
not prove the origin of those earlier extra requests or waive their failures.

Logs and source receipts remain under `/tmp/fourth-provider-controls-*`. No device,
service account, real private content, physical deletion or acoustic acceptance
is involved. The independent retained-send privacy HOLD is unaffected.
