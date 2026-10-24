# Released Voice delivery-ledger continuity

These six ignored tests execute the published ARM64 `lamp-voice` from releases
6408 and 6498. They must run on isolated native Linux ARM64, with a writable
private `/run/lock` and no devices, network, cloud credentials, or other Voice
consumer under the same UID. The Rust fixture also typechecks on macOS; it
refuses execution there. The outer runner owns its timeout and cleanup.

```sh
LAMP_COMPAT_VOICE_6408=/artifacts/6408/lamp-voice \
LAMP_COMPAT_VOICE_6498=/artifacts/6498/lamp-voice \
LAMP_COMPAT_OUTPUT=/tmp/pair-output \
cargo test -p lamp-voice --test release_pair -- --ignored --test-threads=1
```

The output parent is optional and must already exist if supplied. Each case
creates a fresh retained `vr-*` directory. Both artifacts are checked against
fixed release SHA-256, byte length, and ELF architecture before private copies
are executed. The running process executable is hashed through `/proc` too.
The private executable copies are checked again after execution, with their
SHA-256, size, and mode retained in `artifacts-after.json`.
The artifact pins and release source commits are in `fixture.rs` and each
retained `case.json`. No artifact is downloaded or built by these tests.

The exact child command is `lamp-voice schedule-consumer`, with
`LAMP_SCHEDULE_SPEAK=on`, `LAMP_SCHEDULE_DELIVERY_DIR`, `LAMP_SCHEDULE_SOCKET`, and
`LAMP_BUS_ROOT` pointing into the case directory. A connected Unix datagram is
its stdin. The fixture retains the production SCM_RIGHTS consumer claim until
the process is killed and a real SIGKILL wait status is observed, then closes
the claim before starting the other release. Every live ledger write and
recovery belongs to a released executable.

A real typed bus publisher supplies fresh host-boot-bound `schedule/due`
messages as `schedule`. A strict local JSON-line Schedule fixture supplies one
pending occurrence and accepts only Get, List, Status, and the exact guarded
Ack. It can refuse Get or withhold Ack replies to select a durable boundary.
It commits an accepted Ack once and only replays that exact receipt.

Each direction covers three cuts:

- Prepared but unfinished: the next released process offers the same occurrence
  with a newer attempt and a new consumer correlation. It sends no Ack until
  the fixture supplies a completion for that new attempt. The Ack request ID
  retains the occurrence's first durable attempt, as both released writers'
  `make_ack` implementations require; the completion attempt still advances.
- Completed before Ack intent: later Get requests are refused before the
  authored completion is sent, so an intervening recovery tick cannot plan Ack.
  The opposite release acknowledges without another offer.
- Ack intent persisted before a reply: the opposite release retransmits the
  exact saved request, including its revision and request ID, without another
  offer. Fresh pending heartbeats continue during this check.

The original release then reopens the recovered state. The ledger identity and
attempt counter are preserved; the acknowledged occurrence neither offers nor
acknowledges again. Raw datagrams, API requests/replies, kill/reap records,
stderr, and stopped-store snapshots are retained. Today's `Store` is used only
to extract JSON from forensic copies; live file hashes are compared around
each copy inspection. It never opens or rewrites the database handed to the
opposite released reader.

The completion proof is authored at the consumer's existing private IPC
boundary. These tests do not establish actual speech, playback retirement,
coordinator cancellation/privacy behavior, Schedule-store persistence, a
physical power cut, release-signature trust, or device qualification. No
provider, microphone, speaker, or actuator is started.
