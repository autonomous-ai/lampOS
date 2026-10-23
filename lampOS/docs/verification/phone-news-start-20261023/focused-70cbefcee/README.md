# Phone-news start notification: focused measurements and controls

Ready: **NO — full gates have not run.** This receipt covers the frozen base
`70cbefceee62c0bc4cad4ec0b7054335dd8f0a5d` and the two-file authored patch,
not the subsequent carry to `e5cd73aca` or its qualification. The completed
run is `run-20261023T031009.481569Z` (2026-10-23 03:10–03:24 UTC).
No Linux, ARM64, full-package, reader, strict Clippy or workspace-build result
is claimed here. Parent retains publication and qualification ownership.

The single case is
`messages::tests::a_text_never_waits_on_the_news_for_the_phone`.
The six normal observations and five controls each ran exactly once as planned:
**9 passing libtest observations and 2 intended failing observations**. The
runner accepted all eleven prescribed outcomes; this is not a green package gate.
Every invocation had exactly one named case, 3,588 filtered cases, no ignored
case and no external timeout. No failed observation was retried.

## Observations

Compilation is excluded. Wall time spans test-process launch through exit;
libtest time is its own reported duration. First use and repeats are retained.

| Observation | Exit | Wall seconds | Libtest seconds |
| --- | ---: | ---: | ---: |
| normal-before/sample-1 | 0 | 3.394540 | 3.33 |
| normal-before/sample-2 | 0 | 3.403274 | 3.33 |
| normal-before/sample-3 | 0 | 3.389714 | 3.32 |
| normal-after/sample-1 | 0 | 3.128815 | 3.07 |
| normal-after/sample-2 | 0 | 3.096773 | 3.07 |
| normal-after/sample-3 | 0 | 3.133567 | 3.07 |
| healthy-delayed-start-before/sample-1 | 0 | 3.836898 | 3.76 |
| healthy-delayed-start-after/sample-1 | 0 | 4.761799 | 4.68 |
| delayed-start-no-preemption-before/sample-1 | 0 | 3.825329 | 3.77 |
| delayed-start-no-preemption-after/sample-1 | 101 | 7.672022 | 7.62 |
| missing-ready-event-after/sample-1 | 101 | 5.057714 | 5.0 |

Normal medians were **3.394540 s before** and **3.128815 s after**
(a 0.265725 s difference). These are three sequential samples
per source, not a randomized performance study or a device-latency measurement.
All successful runs retain the complete two-second unwanted-news observation.
The earlier 3.320-second ARM case is not a baseline for this comparison, and
there is no under-three-second claim.

[Observations](observations.json) give all eleven results and marker offsets.
[Commands](commands.json) give exact argv, cwd, environment reference, times,
exit codes and raw-stream references for all 40 recorded commands. The exact
sample command is the corresponding compiled executable followed by:

```text
messages::tests::a_text_never_waits_on_the_news_for_the_phone --exact --nocapture --test-threads=1
```

## What the controls establish

The old fixture waited 1.3 seconds and assumed the phone round had begun.
The candidate's fake model stores a Notify permit when its phone-news future is
actually polled, immediately before the unchanged 30-second wait. The fixture
waits at most five seconds for that event before setting `sent_at`. This is an
entry observation, not round completion or general worker readiness. A permit
arriving before `notified()` is awaited is retained.

- Healthy 1,600 ms startup delay: both fixtures pass. Before has text-timer
  marker byte offset 0 and no phone-entry marker; after records phone entry at
  offset 86 before text timing at 125. Passing the delayed old fixture therefore
  does not demonstrate preemption of a running phone round.
- The same delay with production phone-round preemption deliberately removed:
  before passes with no phone-entry marker. After records entry before timing,
  then fails the unchanged original response watchdog at `messages.rs:967`,
  `the text waited: []`. The two controls differ from their healthy counterpart
  only by replacing the production select/pending loop with `round.await`.
  This is a transient broken-behavior control, never shipped product code.
- Removing only the new notification: after fails at `messages.rs:954`,
  `phone-news round did not begin: Elapsed(())`, with no text-timer marker.
  This proves the fixture cannot proceed to timing when its event is missing.
  The control intentionally has a dead-code warning for `began`; its complete
  compiler stderr is retained. No strict-Clippy result is claimed for mutants.

The controlled false negative is demonstrated for this delayed-start schedule.
It does not establish a frequency or explain every historical unscheduled
failure. The passing normal samples do not qualify a full parallel suite.
Marker offsets are zero-based offsets within each original stderr payload,
not within the wrapper files. See [raw payloads](raw-payloads.json).

## Preserved behavior and source identity

All five [original assertions](original-assertions.json), and every byte from
`let sent_at` through response checks, the complete two-second quiet window,
worker abort, cleanup and final assertions, are identical in **all seven source
variants**. The original five-second response watchdog, strict response elapsed
`<2 s`, 30-second fake pending wait, one-second product news look, 50 ms product
input polling and 10 ms fixture response polling remain. Clocks remain real.
The authored production prefix and all source outside this exact test are
byte-identical. Only `messages.rs` and its English `docs/messages.md` description
are changed by the authored patch.

[Variants](variants.json) binds all seven source hashes to the exact compiled
binary hashes and inventories. Reconstruct normal-before from the retained
[source snapshot](source/messages.before.rs.txt); apply
[authored.patch](patches/authored.patch) for normal-after, then the named control
patch to its declared before/after source. The document portion of authored.patch
applies to the document at the frozen base. The same candidate documentation
was present in both normal arms; production/source inputs otherwise match the
base. Six patch dry-run checks succeeded against external scratch files.

[Source proof](source-proof.json) independently reconstructs all 9,991 tracked
paths (6,648 under lampOS), including root robots/lamp fixtures and literal
symlink contents, from the frozen Git tree and authored patch. All seven full
source aggregates match the execution records. All five assertion macros and
the response/quiet tail were checked independently, not merely counted.
Mtime refresh preserved the full content aggregate. Exact candidate restoration
was recorded after each variant and finally; the final aggregate equals the
initial candidate aggregate. This audit did not mutate a repository or target.

## Artifact, environment and raw evidence

[Environment](environment.json) records jobs 2, debug 0, incremental 0,
offline/locked compilation, cleared target/test-thread overrides and the
explicit target directory. Rust 1.96.0 reports `x86_64-apple-darwin`; every
recorded executable is Mach-O x86_64. Physical hardware and translation state
were not established, and host load was not isolated or matched.

Each source compiled once. The exact selected compiler-artifact event was
matched to its recorded artifact metadata; each was freshly compiled, test
profile, debug assertions and overflow checks enabled, with no Cargo features.
The runner checked executable and full source hashes before each invocation.
These are captured hashes of the executed binaries, not a claim about the
current contents of a reused target directory.

All seven full raw inventories are byte-identical: **3,589 tests, 14 ignored**;
the selected test occurs once and is not ignored. One canonical raw inventory
is retained in [raw/inventory.txt](raw/inventory.txt), with every variant bound
by command and payload hash. Listing a case does not mean it ran.

All 22 exact-test stdout/stderr streams, seven compiler stderr streams, and
all identity/list streams are retained losslessly. Identical identity and
inventory streams share one payload with individual provenance references.
[raw-payloads.json](raw-payloads.json) gives exact byte offsets, lengths and
SHA256 values; slice those bytes to recover each original stream. Wrapper
closing markers are outside the payload; original bytes are never trimmed.
Both expected failures retain their full stdout and stderr. No stack trace
was requested (`RUST_BACKTRACE=0`).

Bulk Cargo machine streams, original preparation/runner metadata and the
9,991-entry source map remain external with exact paths, byte lengths and
SHA256 values in [external-files.json](external-files.json). Their portable
replacement is the verified selected artifact metadata, complete common
inventory, source reconstruction proof and normalized commands. The local
PATH value is omitted and hashed. These omissions are explicit availability
limits; no scripts, binaries or Cargo JSON streams are included.

[validation.json](validation.json) records package verification;
[SHA256SUMS](SHA256SUMS) covers every other packaged file. Parent may publish
this receipt, but the source's later full qualification remains separate.
