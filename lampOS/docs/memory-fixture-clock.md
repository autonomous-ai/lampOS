# Memory's explicit child fixture clock

`lamp-memory-fixture` is a development-host child adapter. Its executable includes
the same `application.rs` entrypoint as `lamp-memory`; the ordinary binary and
all original health scenarios retain their existing behavior. The fixture has
`test = false` so extracting that entrypoint does not duplicate the original
binary's unit-test identities.

Before entering the shared application, the fixture reads exactly one JSON line
followed by EOF from explicitly piped stdin. `Bootstrap` contains `version: 1`,
an absolute `control` Unix-socket path, and an ASCII alphanumeric `nonce` of 1–32
bytes. Its immediate directory must be an actual directory with mode 0700; the
endpoint must be an actual socket with mode 0600. Symlinks at those two entries
are rejected. The parent owns these temporary resources. There is no production
environment switch or additional application CLI option.

Bootstrap read, EOF, connection and `Connected` transmission share a 30-second
host deadline, a watchdog: a `--memory-days` child's SQLite open makes full
flushes, which a busy Mac disk held for 8.8–13.6 s (#1569). The reader runs
only inside this dedicated child. A blocked stdin
reader or connection cannot be safely cancelled through `std`; bootstrap failure
returns from this executable, ending that reader with the process. The parent
must still observe the real child status and reap it. `Connected` proves only
the control connection, not application readiness or Reporter construction.

After bootstrap, the unchanged entrypoint installs its real stderr relay, parses
the original arguments, checks the private store, binds the actual server socket,
and selects the optional Reporter factory in exactly the existing order. Only
that factory supplies `FixtureSchedule`. With no selected Reporter there is no
reporter boundary to acknowledge. Default-off absence tests remain host timed.

`FixtureSchedule` implements the accepted `ReportSchedule` interface. Reads are
pure in one checked `Instant` domain, initially zero microseconds; only a valid
parent step advances it. This controls Reporter and publisher cadence. State
handler durations, count freshness, request timestamps, UTC headers, subscriber
leases, real store and socket operations, and the 100 ms Reporter shutdown bound
keep their existing clocks and behavior.

The parent sends `Step { nonce, round, at_us }`. Round 1 must settle at zero.
Later rounds are consecutive and nondecreasing, and may reach but never pass the
last acknowledged `next_due_us`. Each frame is at most 4096 bytes including its
LF. Every read, observed-work wait and response uses one cumulative 30-second
host deadline. The shared socket channel uses nonblocking I/O followed by bounded
`poll`; HUP is followed by a real read so queued bytes precede EOF. It does not
change timeout socket options on closed peers.

The Reporter sends nonblocking observations to a fixed 128-entry channel. Overflow,
a second observing worker, malformed progress, stale rounds and unsupported time
steps latch failure and close control; omitted observations cannot become valid
progress. The worker still uses its real park token. An unpark between a positive
wait check and parking is retained. Closing control freezes logical time and
wakes once; subsequent parks remain parks, so a closed fixture cannot spin or
fall back to host cadence. The unchanged `Reporter::finish` can still unpark it.

`Checked` contains the nonce, round, logical time, cumulative completed `cycles`,
cumulative publication `attempts`, `last_outcome` (`not_attempted`, `returned_ok`
or `returned_err`), the current `cycle_started_us`, `next_due_us`, and the exact
`marker` line including LF. The first positive boundary follows the actual initial
cycle: one completed cycle, start zero, next due 1,000,000; an initial Opening
cycle may have zero attempts. A step before that due time retains its cycle and
publication counts. A step exactly at due must observe the next actual completed
cycle and its positive wait, preserving the full one-second period. A zero-left
wait is never acknowledged. Equal-time settling can reuse a completed positive
boundary without waking the Reporter or fabricating another cycle.

After that boundary, the control worker writes the actual line
`memory-fixture barrier <nonce> <round>` to stderr, then sends `Checked`. Each
marker is at most 128 bytes. It shares the real application's stderr relay and
follows the Reporter's real publish call and failure/recovery logging decision.
The parent must observe the exact complete marker in its actual log destination
before advancing again. `Checked` alone is neither a downstream log receipt nor
proof of a Diagnostic datagram. Publication success is only the actual call's
return value; tests must retain their independent subscriber receives. EOF and
control receipts are never child-exit evidence.

The planned fixed health scenarios have a narrow log-capacity argument: four
bounded request lines at 256 bytes, 512 startup bytes, two transition lines at
40 bytes and one 128-byte marker total 1744 bytes. Observing each marker before
the next step keeps that interval below 4 KiB and the existing 64 KiB relay cap.
The adapter does not promise this bound for arbitrary error strings, request
floods, slow log destinations or unrelated scenarios. A blocked actual stderr
write can end only with the owned child; the parent retains its host deadline,
kill, real `try_wait` and reap obligations. Control teardown shuts down its real
socket and freezes/wakes the schedule, and only joins an already finished control
thread. No logical receipt substitutes for process completion.

The new `fixture_child` tests cover clock and protocol boundaries, real Reporter
publication with an independently received datagram, real park-token retention,
close/finish, bounded framing and closed-peer bytes, and marker ordering through
the actual IPC relay implementation. Their held-work adapter test models the
observation boundary; the unchanged Reporter scheduler tests separately hold the
real publication closure. These checks do not migrate or shorten any original
health scenario, and are not measurements of lamp performance.

## Original health fixture migration

The seven original `health` test identities retain their scenario checks. Four
explicit constructor sites select the shared-body fixture executable for five
live reporting children, including both backends in the retained-count helper.
Scratch, Reports and Process share one local `Rc<RefCell<Option<Controller>>>`;
there is no global lookup or implicit activation. The four other children use
the ordinary executable: both environment-only 2200 ms/100 ms observation loops
and both failed-open 250 ms post-reap drains keep their original host timing.

Controlled `report_when` drains the real subscriber in its original order. Only
an actual empty receive permits advancement to the next original reporter due
time. Receipts do not become Diagnostics. Readiness still requires the actual
ready file and live child; exit still requires the actual child status. Each
matching/logging loop retains one logical and host Window. The sustained-failure
loop shares one Window across all 200 ms advances and real log barriers, including
its final zero-left iteration, while keeping the obstruction through all three
full report periods. Request timestamps, three-second request watchdogs, State
handler durations/count ages, UTC headers and subscriber leases remain real.

Assertion logs settle current time and observe the actual stderr marker before
reading the file. Error diagnostics use a separate raw reader to avoid recursive
control calls. A failed controller remains failed; it never falls back to host
cadence. Original kill/wait cleanup remains authoritative. The cumulative
30-second control/marker waits exclude potentially blocking kernel cleanup
and filesystem operations. Local source preservation and execution evidence must
be reported separately; this fixture change is not a lamp latency measurement.
