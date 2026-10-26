# Shared operational logging

`lamp_bus::logging` gives a part an explicitly constructed logger and
`lamp-bus logs` follows all participating parts on one event topic. No existing
runtime starts using it automatically. `Logger::default()` and
`Logger::disabled()` perform no I/O, create no worker and admit no records.
No global log facade, environment switch, service, deployment file or existing
stdout/stderr behavior is changed.

It is separate from `lamp_ipc::log_relay`, which keeps each part's stderr
lines flowing to the journal without blocking. The relay carries everything a
part prints, as text, to one machine's journal; this stream carries chosen
operational records, typed, to anyone on the Bus who follows them.

The IPC relay's full-pipe/socket regression keeps each destination full until
`write_now` returns `AGAIN`; only then does it drain the destination and check
that the shared descriptor remains blocking. The case runs in an isolated
test process with a 15-second diagnostic watchdog, so a blocking regression
can be killed and reaped. Its successful exit must include the exact selected
test's passing result. This replaces a 100 ms host stopwatch, not a product
deadline or a measurement of journal latency. Linux CI also covers the private
pipe descriptor and `MSG_DONTWAIT` socket paths.

The stream is for bounded operational summaries: readiness checks, a dependency
failure, or a state transition. It is separate from periodic `Diagnostic`
health reports, voice evidence, conversation memory and recordings.

## Schema and identity

The canonical `Log` and `LogLevel` types live in
[`crates/bus/src/logging.rs`](../crates/bus/src/logging.rs), and `lamp-msgs`
re-exports them. This is a deliberate transport-metadata exception to the
normal message ownership rule: msgs already depends on bus, and the bus CLI
must use the exact schema without introducing a dependency cycle or a second
definition. `logging_topics()` and `declaration_of()` recognize the optional
topic; `first_topics()` retains its original seventeen entries.

| Property | Contract |
|---|---|
| Topic / type / version | `logs` / `lamp_msgs/Log` / 1 |
| Kind | Event, with no period or lifespan |
| `part` | Bus source name, `[a-z][a-z0-9_]{0,31}`, equal to header source |
| `level` | `debug < info < warn < error < fatal` |
| `code` | Stable identifier, `[a-z][a-z0-9_.-]{0,63}` |
| `message` | At most 512 decoded UTF-8 bytes |
| `attempt` | Positive count of open-logger admission attempts; invalid/full attempts consume a number |
| `created_unix_ms` | Admission wall clock in milliseconds since 1970 |
| Serialized payload | At most 4,096 bytes, encoded into a fixed buffer |

Unknown JSON fields are ignored for forward compatibility. Missing required
fields and unknown enum values fail deserialization. `check()` checks value
bounds; `from_message()` also validates exact topic/type/kind/encoding, payload
size, and source-to-part agreement. Generic undeclared JSON is not a log.

The bus header's `seq` counts publication attempts accepted by the bus; a
message rejected at the logger queue never receives a bus sequence. The log's
`attempt` makes that distinction visible. The header's `at_us` is elapsed time
since this logger started, sampled during admission; `at_unix_ms` is sampled
later during publication. Neither admission time nor publication wall time
establishes a total order across processes. Pair an attempt with the bus
source and publisher boot identity, never with source alone across restarts.

Use one logger per part/process identity. It is not cloneable and
`try_log` requires `&mut self`; multiple threads must coordinate through their
existing owner. Do not create a worker per call or replace a stalled worker
without confirming its exit. The existing subscriber remembers only 32
source names per topic. With more, older accounting is forgotten; simultaneous
publishers using one source name appear as repeated restarts. Those existing
bus limits are unchanged.

## Admission and worker ownership

```rust,no_run
use lamp_bus::{Bus, logging::{Logger, LogLevel, Admission, FINISH_WAIT}};

let bus = Bus::from_environment()?;
// Explicit startup I/O, outside interaction handling.
let mut logger = Logger::start(&bus, "body")?;
let admitted = logger.try_log(LogLevel::Info, "checks_complete", "output checks complete");
if admitted == Admission::Full {
    // Existing health reporting may include logger.report().full.
}
let finished = logger.finish(FINISH_WAIT);
// Inspect confirmed, publish_errors, subscriber_drops and unconfirmed separately.
# Ok::<(), std::io::Error>(())
```

`start` declares the topic synchronously and creates one worker. An invalid
source, incompatible declaration or unusable root is returned to the caller;
no worker is left by a failed start. A missing usable root is created according
to the existing Bus rules. The owner decides how to report an unavailable
optional logger without changing interaction readiness.

Admission validates lengths before cloning strings, serializes to a fixed
4 KiB packet, then uses `std::sync::mpsc::SyncSender::try_send` into a
64-entry channel. The queue therefore has a fixed packet capacity, plus one
packet being processed and the caller's temporary packet. It never waits for
queue capacity and never performs filesystem, socket, stdout or stderr I/O.
It returns `Disabled`, `Queued`, `Full`, `Invalid` or `Closed`. Full or invalid
records are rejected whole; no truncated prefix or overflow file is produced.
The logger adds no synchronous fallback and no automatic retry queue.

Only the worker publishes and refreshes liveness. It calls `alive()` at
approximately 100 ms opportunities, including while idle; the Bus's default
lease refreshes the publisher file at most once a second. The default three
second lease is intended here. A custom lease shorter than the worker cadence
must not be interpreted as a logger-readiness promise. Filesystem stalls can
delay this worker and eventually cause admission drops, but cannot add disk
waits to `try_log`.

Every subscriber still has its own nonblocking transport queue. Slow followers
drop their newest messages without holding up other followers. A new subscriber
can take up to the Bus's default 200 ms discovery interval to be found. There
is no replay for events sent before discovery or while nobody listened.
The 4 KiB application cap does not raise a host's datagram limit: stock macOS
may refuse an escaped record above its 2,048-byte datagram limit. Such a worker
publication failure is counted; the input record is never truncated.

`report()` gives accepted, full, invalid, rejected-after-close, processed,
successful-publish-call, publish-error, liveness-error and subscriber-drop
counts. Running snapshots may straddle a worker update. Subscriber-drop totals
are retained even when a publish call reports a root error after sending.
The Bus suppresses repeated root errors until recovery, so `publish_errors`
counts returned errors, not every event lost to an unavailable root.
`published` means the bus call returned success; it does not mean any
subscriber received the message. `unconfirmed` is accepted minus processed,
not an estimate of records safely stored.

`finish(wait)` closes admission and waits at most `min(wait, 500 ms)` for the
worker to drain accepted packets and release its publisher. The channel's
disconnect contract drains queued packets before signaling EOF; there is no
separate empty/closed race that can discard an accepted tail. `confirmed`
means the worker acknowledged completion, including failed publication
attempts. It does not acknowledge subscriber receipt or durable storage.
A timed-out report remains false even if a later finish sees completion.
Drop only closes the channel; it never joins a worker blocked in filesystem
I/O. The worker has no stderr fallback that could recursively log a failure.

## Following and capturing

```text
lamp-bus logs
lamp-bus logs --part voice --level warn --seconds 30
lamp-bus --root /private/tmp/lbus logs --level debug --count 5 --seconds 10
lamp-bus record /private/tmp/log-capture logs --seconds 60 --max-mb 8
```

`logs` defaults to `info` and above. `--part` selects one source; omitted, all
parts are followed. `--count` counts matching valid records only. The output
shows creation time, severity, part, code, escaped message, attempt, boot,
bus sequence, bus gap and restart flag. It never infers loss from a gap between
filtered attempts. Raw terminal controls, including ESC and C1 characters,
are escaped. Malformed/non-log messages are passed over and summarized on
stderr when the command ends. A typed subscriber also counts wrong-type
messages rejected by the bus. Existing malformed-traffic handling and timing
behavior of `Subscriber::recv` remain unchanged.

Wrong CLI input exits 2 with usage; operational failure exits 1. Reaching a
requested count exits 0; expiration with fewer matches exits 1. Without a count,
normal duration expiry or a closed stdout exits 0. The command inherits the
existing CLI's synchronous stdout handling: a stalled output can delay this
follower and lose its events, without holding publishers. SIGTERM has the same
default behavior as `tap`; a subsequent publish removes its abandoned socket.

`record` is an optional capture tool with its existing per-topic file cap
(64 MiB unless `--max-mb` is supplied). It stores declarations and original
headers plus base64 payloads, not receive timestamps. It can append and stops
that file when full; it does not rotate an ongoing shared log or enforce a
lifetime total across capture directories. It provides no crash-durable or
whole-final-line guarantee on a failed disk write. No log history is retained
by the logger or by `logs`, and no record is delivered when nobody listens;
`keep-logs`, below, is what keeps it.

### Kept for the next morning: `keep-logs`

`lamp-bus keep-logs DIR` is the opt-in, bounded writer that keeps the stream
on disk (`crates/bus/src/logging/keep.rs`, #699). Nothing starts it.

    lamp-bus keep-logs /var/lib/lampos/logs --level info --file-kb 2048 --files 10

- **What a line is.** One JSON object per validated log at `--level` or
  above (info): `at_unix_ms` (the log's `created_unix_ms`), `level`,
  `part`, `code`, `message`, `attempt`, and its bus identity `boot`,
  `bus_seq`, `bus_gap`, `restarted`. Text stays JSON-escaped, so a line is
  always one line.
- **Bounds.** Files are `DIR/logs.NNNNNNNN.jsonl`. Each start opens the next
  number, so a restart begins a new file. A file is closed before a line
  would take it past `--file-kb` (2048, from 64 to 65536), and only the
  newest `--files` (10, from 2 to 100) are kept: 20 MiB at most by default,
  V1's Go logger's 2 MB times ten. The oldest of its own files is removed
  as a new one opens. Nothing else in `DIR` is read or removed, and `DIR`
  must already exist: a missing one exits 1, and it is never created.
  Files are made `0640`.
- **Loss is visible.** `bus_gap` and `restarted` say what the bus lost or
  where a publisher began again. A line that cannot be written (a full or
  failing card) is counted, said once on stderr until writing works again,
  and the next line written carries `unwritten_before` with the count. The
  end says how many were kept, not written and passed over.
- **Whole lines only.** A write that fails partway (a full card) is cut back
  to the last whole line, so the next line is never glued to a partial one
  and a file's bytes stay counted. A file that cannot be cut back is not
  written again: the next line starts a new file, and the damaged one keeps
  only its partial tail at the end.
- **Durability.** Data is synced at most once a second while lines come,
  and at each new file and the end. A power cut loses about a second at
  most. SIGTERM ends it without the last sync; what it wrote is already
  with the kernel. Only the keeper waits on the disk: the bus keeps every
  publisher apart from a slow subscriber, as for `logs`.

What is still the image/service owner's, under #737: the unit that starts
it, the folder, its owner and group, and whether a lamp runs it at all. The
existing Records adapter recognizes only closed voice-service metadata
sessions; it must not be pointed at this folder as if it had an ownership
contract. Do not vacuum the shared journal or silently enable transcript
storage to fill a gap.

## Privacy, reuse and evidence limits

This bus uses the existing `0770` directories and `0660` sockets/files: the
lamp's group can subscribe, without separate authentication or redaction.
Never submit credentials, authorization headers, raw audio, transcripts,
personal conversation text or arbitrary configuration dumps. The small message
bound is not a content classifier; producers remain responsible for these
rules. Codes and concise operational summaries are the intended content.

The ROS design intent in [bus.md](bus.md#what-is-taken-from-ros) is reused:
one shared typed stream, known source/severity, direct publication, and slow
subscriber isolation. This does not introduce ROS 2 or claim ROS wire or
delivery-policy compatibility. Health's diagnostic levels remain separate.

The V1 source baseline inspected was `origin/main`
`84d6d818cf35b28b507c983a97430c64274fe504`. Its
[Go logger](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/84d6d818cf35b28b507c983a97430c64274fe504/system/lib/logger/logger.go)
uses level thresholds, 2 MB files and ten backups; the
[HAL setup](https://github.com/autonomous-ai/Physical-AI-Operating-System/blob/84d6d818cf35b28b507c983a97430c64274fe504/hal/server_support/log_setup.py)
uses 20 MiB server files and 5 MiB usage files, each with three backups.
Keep recognizable levels and writer-owned bounds. Change caller-side I/O:
Go calls console/file handlers synchronously and its GELF overload/backlog
path appends to disk; HAL directly attaches rotating handlers. Neither is
copied into this interaction-path admission API. No cloud relay or credential
access is introduced.

The existing [voice service trace writer](../crates/voice/src/service_trace.rs)
supplies the local model for fixed packets, counted loss, worker-only I/O,
bounded finish acknowledgment and truthful incomplete evidence. This helper
uses the standard bounded channel instead of exposing a new general worker
framework. Existing Bus publication, source/gap and permission semantics are
reused. Records' restricted layout is preserved.

No owner runtime or persistence integration has been made: a part starts
logging here only when its owner adds a `Logger`.

The latency target is **under 1 ms from entry to return of `try_log`**. On
Third's Intel Mac, release build, the ignored
`admission_cost_with_a_stalled_worker` test measured, with the worker stalled
and 10,000 attempts: first 22.9 µs, median 0.54 µs, p95 0.56 µs, worst 1.4 µs.
Those repeated samples include 64 queued attempts followed by full-queue
rejections; they do not separately establish repeated successful admission
with a running worker. Startup, publication, receipt and storage are outside
that boundary. No lamp timing is claimed.
