# Holding-only answer continuation (#1579)

Host verification for `second/holding-answer`, based on `lamp-voice`
`6275fcc8cb2c229f8b18a6db3b4485cb46a11676`. The final Rust source hashes,
commands and test output summaries are in `gates.json`. This is an Apple ARM64
host with Rust 1.96.0, not a lamp result. No cloud service or physical device
was used.

`LAMP_HOLDING_ANSWER=on` is off unless explicitly set. The worker keeps at most
256 transcript bytes and reports only a complete holding phrase with no
function call. The coordinator waits for successful playback retirement and
one second of fresh quiet, then queues one written note on the original
worker and provider session. A second holding answer, replacement route,
new turn, mute, hand stop, capture gap, failure, or a three-second expiry
cannot create another continuation. Reminder and other lamp-originated turns
are excluded.

## Regression evidence

- The real provider worker emits one observation for a chunked holding
  phrase, before its idle completion. Disabled, substantive, overflow,
  function-call, attention-held and reminder cases emit none.
- The owner fixture emits exactly one written note at the one-second quiet
  boundary, with no added logical clock time. Fresh capture, privacy,
  playback, admission and session guards apply. Unfiltered VAD prevents
  busy-room admission from hiding a person's recent speech.
- A different worker with the same numeric session cannot inherit the
  promise. Real route channels show the note delivered once to the original
  worker, never replayed to fallback after failure. Ordinary written turns
  still use fallback, and idle fallback resumes after the pinned turn ends.
- A fallback stand-in cannot send a pinned note even with an equal session
  number. Rebinding refuses before changing the queued note or request state.

The focused holding run passed 26 tests; all 14 route tests passed. A new
route fixture initially supplied an invalid recovered-link description;
correcting its metadata made that test exercise rebinding. Earlier owner
fixture setup failures concerned stepped-clock ownership and muted capture;
those fixtures were corrected before the gate. The first broad run passed
3,061 tests before the final route pin was added. The final run below includes
the pin and its regressions. After rebasing, the combined worker-flag test
also checks cut-in, misheard-hum and holding-answer together; the full gate
below was rerun on the rebased code.

The first post-rebase gate stopped in the hello-policy integration fixture:
its second subscriber received no reply before the two-second deadline.
An isolated run passed all five tests. Inspection found that the fixture
registered a second decision subscriber after the first response had cached
the subscribers, then expected an immediate rule response inside the bus's
20 ms discovery interval. Both subscribers now exist before the first
response. The assertions and timeouts are unchanged; the final gate below
includes this fixture correction and no production hello change.

## Final local gate

Commands ran from `lampOS/`, with incremental compilation and dev/test debug
information disabled, four build jobs, and one external build cache. Local
socket/process fixtures were permitted. Existing ignored tests remain ignored.

- `cargo fmt --all --check`: passed.
- `cargo clippy -p lamp-voice -p lamp-cloud -p lamp-memory -p lamp-skills -p lamp-voice-eval --all-targets -- -D warnings`: passed.
- `cargo test -p lamp-voice -p lamp-cloud -p lamp-memory -p lamp-skills -p lamp-voice-eval -- --test-threads=1`: passed. Counts summed from the command output:

| Crate | Passed | Failed | Ignored |
|---|---:|---:|---:|
| lamp-voice | 2198 | 0 | 11 |
| lamp-cloud | 36 | 0 | 0 |
| lamp-memory | 209 | 0 | 5 |
| lamp-skills | 88 | 0 | 0 |
| lamp-voice-eval | 539 | 0 | 0 |
| **Total** | **3070** | **0** | **16** |

- `cargo build --workspace`: passed.

`ProviderOutput`, `CloudRequest`, `HoldingAnswer` and the new holding modules
were searched across `crates/`. Their changed uses are confined to Voice.
Cargo manifests identify Cloud, Memory, Skills and Voice-eval as its reverse
consumers; all four are included above. The workspace build checks all other
crates. The switch census includes the named off/on lamp check.

## Limits and lamp check

The worker, owner and route checks use separate host fixtures. No complete
real-provider conversation round trip, recognition accuracy, acoustic
latency, or promised story delivery is established. Zero added logical clock
time at admission is not a wall-clock or acoustic measurement.

Native Linux ARM64 CI must pass on the published PR head before it is ready;
its run number belongs in the PR body. The first builder's lamp check is five
hand-over rounds with the switch off and on, counting holding-only turns and
measuring the last holding word to the first promised-answer word. The switch
remains off pending that check.
