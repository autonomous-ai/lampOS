# Historical #725 candidate: paired synthetic screen

2026-10-19. Reject the historical f376 candidate as the replacement: it hears
fewer positive lines and more controls than the pinned primary in the warm arm.
This is the old [#725 candidate](https://github.com/autonomous-ai/lampOS/pull/725),
not the unavailable rescue model or a new trailing-name model. Thresholds remain
unchanged. No office pilot is needed for this losing candidate.

| Warm form | Primary | Candidate f376 |
|---|---:|---:|
| Bare “Lamp” | 1/3 | 0/3 |
| Initial “Lamp, set a timer for five minutes” | 2/3 | 1/3 |
| Trailing “Thank you, Lamp” | 0/3 | 1/3 |
| “Hey Lamp” | 3/3 | 2/3 |
| All positive lines | 6/12 | 4/12 |
| Controls with a hearing | 0/24 | 2/24 |

Warm rows are exactly `lead_ms == 2500`. Positives have `positive == true`,
grouped by `phrase_id` over voices A/B/C; controls have `positive == false`.
The two candidate control hearings were synthetic “The lamp is on.” (A,
0.9815) and “Lamb is my favourite dish.” (B, 0.9327), one hearing each.
All quoted sentences in this bundle are generated fixture text, not transcripts
of people or real office speech.

The frozen native checker processed all 144 inputs once per model, interleaving
primary then candidate. All **288 checker calls succeeded**, with 288 exported
rows and none omitted. Both classifiers used identical pinned frontend bytes,
raw 16 kHz mono PCM16, default detector threshold 0.9, no rescue, no quiet gate
and no stop model. Each input reset the checker. Bounds were 30 s per call and
30 min overall. The initial low-priority setup failed with “Operation not
permitted”; the verified helper was subsequently lowered to nice 10 without
restarting. That setup failure is retained in provenance.

All 144 fresh primary results match the earlier baseline's maximum/hearing
scores and input timestamps at printed precision. Scores have four decimal
places. This is **not a host-timing match**: the checker binary changed, both
models were rerun with the same frozen binary, and processing times were not
compared or combined.

There are 36 unique synthetic utterances under four padding arms, not 144
independent utterances. The 24 unique controls contain **33.2419375 s** of
speech (531,871 samples). Repetition and padding do not provide an office-rate
denominator; no false-hearings/hour or false-assistant-reply rate is qualified.
These reused inputs are not a blind holdout for choosing a future model.
Played-line recordings remain unavailable. No acoustic, STT, live-service
pre-roll or device-latency result is claimed. There was no audible playback,
live capture, device access, training or threshold change.

[paired-inputs.csv](paired-inputs.csv) contains all 288 rows with stable pair
IDs, anonymous voices, synthetic text and input digests. Maximum/hearing times
are positions in the input, not latency; hearing arrays preserve every event,
and empty arrays mean none. Each pair has exactly one row per model.
[summary.json](summary.json) contains all four padding arms. Model, frontend,
checker, input-manifest and retained raw-evidence digests, exact settings and
warm selection are in [provenance.json](provenance.json). Raw evidence is
identified by text labels and hashes only. [SHA256SUMS](SHA256SUMS) covers the
four payload files; no audio, model binaries, real transcripts or machine paths
are included.
