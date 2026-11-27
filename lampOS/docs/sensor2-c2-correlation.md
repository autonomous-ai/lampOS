# Sensor 2 C2 request correlation

This instrumentation is separate from the candidate classifier. Its base is
`0dbbfc95bbea6ddf397308e1a8ef1933c5d19dcb`; it adds no remote model request,
new transcript event, audio capture, report, or product default.

## Opt-in contract

An authorized test run sets `LAMP_TEST_INPUT_REQUEST_IDS=on` on the coordinator.
The coordinator's cleared child environment forwards exact `on` to the second
route only. An older coordinator cannot forward this capability. An older
worker ignores it and emits the old shape, accepted by the new coordinator.
The optional provider field is absent off. Directly forcing the capability
into a new worker beside an old coordinator is unsupported: the old strict
wire decoder rejects the extra field. Use a matching test kit; do not infer
capability from the model or enable it in product units.

The normal second-route final-ASR event carries its own request ID before
carried text or a synthetic name is added. Its existing `request: null`
semantics remain input, not output lineage. The coordinator records the same
`kind: transcript`, `turn: null`, `text`, `finished` and `at_us`, with only:

- `input_request`: the producer's exact request, not the currently active turn.
- `input_correlation`: `second-route final ASR request`.

Absent metadata stays `unreliable; session scoped`. Live/Gemini events are not
assigned guessed IDs. Output transcripts keep output lineage. Count-only
onboarding remains uncorrelated and follows the existing no-trace path;
existing care redaction remains in front of trace creation. No new transcript
is emitted when a stop/privacy boundary would otherwise discard the work.
Existing late input events retain their original identity, not a newer turn.

## Joins and limits

Join within one trace/session boot by `input_request` to existing turn and
capture/playback evidence. Never join solely by nearest timestamp, words,
journal order, or the current reply. Preserve raw trace provenance and handle
missing, repeated, stale or conflicting joins as unknown.

`at_us` is when the coordinator observes the event, not speech onset, STT's
socket-final timestamp, or acoustic playback end. Request correlation alone
does not establish a previous user/lamp pair, a measured answer end, correct
ASR, a trial label, consent, or sensor qualification. The C2 extractor must
still validate every required context and clock join independently. No device
run, backend request, or on-device latency measurement is authorized by this
source change.
