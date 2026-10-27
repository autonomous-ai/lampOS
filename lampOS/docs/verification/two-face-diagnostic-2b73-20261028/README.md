# Two-face fixture diagnostic preparation

The diagnostic code was executed on Main
`2b73dd03073947c62b3d10593f293aa9ab954410` plus the two-file diagnostic patch
`6de9cd03d3b2b3acd588f2ae8e18ddd12b3bf938e639fb35d200292b7bde8a62`.
The candidate is carried onto Main `89823095e69dcbafaa5b3b79733c17ecf9d8e8f7`:
that delta changes only four played-tool files. All 297 captured local driver
dep-info inputs, every actually emitted local package directory, and the
workspace manifest, lock and Cargo configuration are unchanged. No test was
rerun or relabelled as execution on the later base. The earlier unexecuted fc9
preparation is also retained. This is diagnostic evidence, **not a repair**.

The original #3544 ARM run 40588908587 failed on checkout
`22cf6a6939ecd1968c756422a7ac029cf8759741`, whose parents are
`c1fa2bc9f327f5d553bb3dcc355053d8e7e94bfa` and
`1b1da9ae3fc6fbf1101cd1b88f10bcaa37fb55ad`. Its 6,466 passes, three
failures and 1,336 selected unrun cases remain recorded. The two-face case
failed its first positive scenario after 7.721 seconds: eight two-face looks,
held mode on and no privacy change. The actual tuple was Unknown and no sign
that anyone called the lamp; the expected tuple was Busy and the existing
face-alone refusal. No cause follows from the total duration or from source
equality. The trace and privacy failures belong to separate work.

The original fixture, clock, bus, room, attention, thread, manifest and lock
inputs match this preparation base. The current diagnostic patch changes only
test code and hooks removed by `cfg(test)` in a normal build. The original
five scenarios, assertions, sleeps, 2-second publication lifetime, 50 ms bus
grace, 1-second gap/freshness rule, 1.5-second persistence, 2.5-second retained
window, 16-look cap, privacy epoch/cutoff and default-off behavior remain.

The receipts separate three boundaries:

- Publication records its actual before/after monotonic and wall-clock times,
  the monotonic argument, and `Published` delivered/dropped/removed counts.
  A delivered count means a socket queue took a message. It does not prove
  that this reader admitted it. The publisher API does not return its actual
  header; the timestamp argument joins to the header at ingestion and verdict.
- Reader receipts run **after parsing inside the existing held-state lock**.
  They record the actual header/sequence, sampled epoch, admission time,
  accepted/rejected branch and bounded kept state. No second reader lock was
  added. Malformed or otherwise unparsed messages are outside this receipt;
  a missing receipt is not classified as a specific transport or parse cause.
- Verdict hooks observe the two existing `Bus::latest` reads without another
  read. They retain actual headers and `expired`, plus actual turned/count
  results and the held-policy operands. A missing latest message means
  absent-or-error; the hooks do not invent an expiry diagnosis.

Each calling-thread observer owns its log. TLS contains only a weak lookup to
that non-Send observer, restored when its scope ends, including assertion
unwind. Reader data belongs to this Heeding instance under its existing lock.
Each log has at most 64 records and an explicit dropped-record counter; each
kept-look snapshot has at most 16 scalar entries. There is no frame, audio,
key, face name, extra filesystem read, network operation, clock setter or
production observer. A nonzero dropped counter means incomplete diagnostic
evidence. Observation adds work in test builds and can affect scheduling.

Fresh Darwin/x86_64 validation completed: workspace formatting, metadata
with `--no-deps`, strict Voice all-target Clippy and current Voice-library
compilation all passed. The actual emitted driver, opaque package IDs,
feature vectors, dep-info bindings and independent saved copy were admitted
before exact discovery and runtime. All three cases below passed once in
separate processes with one test thread and zero retries:

- `with_two_faces_held_looks_between_lines_make_the_first_line_busy`
- `with_two_faces_held_two_faces_must_stay`
- `with_two_faces_held_privacy_late_looks_and_clock_steps_leave_no_busy_room`

Each process had an outer 30-second / TERM 2-second / KILL 2-second watchdog.
No timeout, cleanup, unknown panic or selected unrun case was observed. Four
compiler commands took 229.655 seconds; the three test processes took
8.277 / 0.026 / 0.026 seconds. These are host process boundaries, not lamp
latency measurements. All source/tool/helper/driver guards and 193 historical
saved copies matched; one new driver was saved independently, making 194 at
this gate's release. All groups were reaped and the owner lock was released.

All five diagnostic reports were present with zero log drops:

| Scenario | Published | Parsed receipts | Admitted | Observed outcome |
| --- | ---: | ---: | ---: | --- |
| Held, eight two-face looks | 8 | 8 | 8 | Busy; face-alone refusal |
| Held, one stray two-face look | 4 | 4 | 4 | Unknown; allowed |
| Held, newest look sees one | 7 | 7 | 7 | Unknown; allowed |
| Privacy close/reopen | 9 | 9 | 8 | Unknown; allowed; post-reopen look rejected by epoch |
| Held switch off, one two-face look | 1 | 0 | 0 | Busy; face-alone refusal |

The positive run's eight admitted looks spanned **2.148085 seconds**, with a
largest gap of **309.821 ms** and newest age **109.311 ms** at the policy query.
Every existing latest read reported `expired=false`. Queue-delivery totals
were 8 / 5 / 8 / 10 / 1; those are socket sends, not ingestion counts or proof
of one current subscriber. Default-off has no current held-reader receipt.

The privacy report is specifically **not** proof of a fresh single look being
admitted and then failing the persistence threshold. Its post-reopen header
was 11 microseconds after the cutoff and not expired, but the reader had
sampled receive epoch 0 before waiting; current epoch was 2. The safety check
rejected it and the policy saw an empty deque. A future deterministic repair
must separately prove a fresh current-epoch single-look case and preserve the
queued/cross-epoch rejection control.

The local setup helper first exited before any Cargo command because it
incorrectly retained an unrelated OwnKeys activation assertion. Its exact
helper and zero-command failure are preserved. The corrected controller was
the first compiler/runtime execution for this source; no test was replayed.

The complete native failure, current host receipts, first setup error and
source-carry proof remain separately bound. A raw dep-info copy was not
retained; its hash and parsed source/hash rows were retained before bank
handoff. No subsequent busy-bank read or old actual-byte admission is used.
This result does not establish the original failure's cause, clear #3544's
failed gate, qualify a full suite or claim a device/provider result.

## Repair assessment, not implemented

Receipt-only diagnostics can identify a new run's delivery, admission and
freshness inputs. They cannot make real sleeps deterministic or establish the
historical failure's cause. The sleeps provide minimum delays, so the fixture
does not guarantee the production maximum gap and retained-window conditions.

Entering `scenario_clock::Scope` only on the publisher/test thread is invalid:
it is TLS and non-Send, and the spawned reader retains the real monotonic
clock. Sharing a monotonic value alone also leaves `Bus::latest`'s actual wall
expiry independent. The bus's existing cadence clock controls rescans and
liveness, explicitly not packet expiry or real delivery; it is not an expiry
clock seam.

A fully shared deterministic asynchronous clock would therefore require a
larger explicit seam covering publisher timestamps, reader admission,
verdicts and actual header expiry. It must not become a production option or
silently bypass `message.expired`. That injection is not part of this patch.

The narrower alternative is to drive deterministic policy/route scenarios on
the existing single-thread scenario pump, retaining every original outcome
and privacy/default-off assertion, and separately keep bounded real-bus
delivery/reader and actual-expiry controls. That makes the division explicit:
the deterministic route cases would not claim real asynchronous cadence, and
the real-bus controls would still require genuine receipts and fail on bad
inputs instead of being skipped. Any such split needs review of the exact
seam and the retained five-scenario mapping before implementation. No
additional sleep, deadline increase, threshold relaxation or retry-to-pass is
proposed. A runtime policy change, if later shown necessary, first requires
the actual V1 baseline read.
