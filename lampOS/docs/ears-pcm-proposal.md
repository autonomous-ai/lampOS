# Opt-in Ears PCM export: review checkpoint (#1669)

Status: local implementation under review, with an opt-in publisher, bounded
buffer primitive and guarded Bus API. No service has been enabled or deployed,
and no hardware qualification is claimed. Existing default `ears/level` and
`ears/sudden` behavior is unchanged. See [ears.md](ears.md) for the implementation.

First's lamp-4ace probe found that `plug:device_micro1` is exclusive while Ears
owns it: the second ALSA reader returned busy and zero bytes. The preferred
follow-up is one Ears owner exporting admitted blocks, not another capture
reader or an image-level `dsnoop` change. Recording helpers cannot solve the
missing publisher by selecting mic2.

## Proposed boundaries

- A separate explicit opt-in remains off by default. Export would require the
  privacy-controlled runtime. Requesting export without that runtime must fail
  closed, rather than quietly selecting the compatibility capture loop.
- The existing capture owner remains the only ALSA owner. An optional copy is
  taken only after the read/fence/lease checks, before the owner clears its
  320-sample block. No I/O or waiting is added to that boundary.
- Each exported block must retain its original permit, capture epoch, fence,
  source sequence and read-completion time. A later lease never extends it.
  A lost block must remain observable as a sequence gap.
- PCM does not enter the 16-item metadata channel. The proposed handoff holds
  at most one block, drops rather than waits on overflow, and clears owned
  buffers on invalidation. A returned in-flight block needs its own final
  boundary checks; taking it from the slot is not publication authorization.
- Muting, a lease gap, changed lineage, restart and shutdown must prevent old
  epochs from being emitted. A recorder must preserve gaps as missing coverage,
  not turn inserted silence into measured quiet.

Latency targets for the proposed runtime: no blocking operation on the capture
handoff; target under 1 ms per 20 ms source block; publication only while the
original permit is valid. These are design targets, not measured results. Test
first use, repeated blocks and stalled publication separately before enabling.

## Guarded Bus boundary

The existing `publish_bytes` API is insufficient as a final PCM boundary. It
copies bytes into a reusable datagram, performs filesystem discovery/presence
maintenance, and can remove dead subscribers and rescan between sends. A fresh
check before calling it does not establish freshness after a stalled operation.
Its reusable datagram also retains bytes after the call.

The candidate guarded PCM send path adds bounded serialization and checks after
preparation and immediately before each nonblocking send. Discovery, filesystem
maintenance and unlimited retries cannot sit in that path. Sender-owned PCM
must not remain in the reusable datagram after return. The existing bus expiry
grace is not a substitute for the original privacy deadline.

Third's review requested an Ears-only 50 ms transport lifespan, now supplied in
both its declaration and messages. The receiver's existing 50 ms grace still
applies. Expired queued blocks are dropped at recv; current privacy and consumer
retention checks remain necessary. Current `Audio` metadata lacks the original
lease lineage and clock identity for strict source-epoch revocation.
Old consumers must not silently interpret new privacy semantics as ordinary
unrestricted audio. A competing Voice publisher on `mic1/audio` must also be
excluded explicitly; the bus permits multiple sources.

## Retention is a separate decision

Capture permission does not by itself authorize a recorder to keep minutes of
PCM, or the classifier to retain its proposed two-second window. Recording is
an explicit operator-run data collection activity. Classifier retention remains
the separate proposal in [sound-events.md](sound-events.md).

Clearing an owned application buffer is not a promise to erase allocator,
stack, kernel socket or subscriber copies. Scheduling may pause a worker after
its final check. We can test bounded ownership and stale-block rejection at
specified boundaries; we cannot claim that every raw byte disappears at exactly
50 ms. The final review must state which boundary the permission governs and
what consumers are allowed to retain.

## Verification still required

The primitive and host wiring tests cover disabled export, overflow, original-deadline
expiry, changed epoch/fence and invalidation. Runtime qualification additionally
needs mute/restart during a partial read, queued and in-flight blocks, a stalled
bus, subscriber replacement, finite retry behavior and source sequence gaps.
No hardware recording or acoustic latency claim follows from unit tests.

## Native CPU comparison for the lamp operator

Use the reviewed candidate binary for both runs. Keep `LAMP_PRIVACY_CONTROLS=on`,
Voice privacy publication, capture device, scene and all other service settings
identical. Only change Ears' `LAMP_BUS_AUDIO` between exact `off` and `on` in a
temporary operator-owned drop-in; preserve and restore the original unit and
privacy state. Restart Ears for each run; never change Voice's microphone owner.
Do not run until the lamp's loan and privacy review are cleared.

For each setting, check Ears remains active, wait for warm-up and record three
30-second windows from `/proc/<MainPID>/stat`: `(utime+stime)` before/after,
`starttime` unchanged, elapsed monotonic time and `getconf CLK_TCK`.
CPU percent of one core is `100 * delta_ticks / CLK_TCK / elapsed_seconds`.
Report cold-start separately, plus RSS before/after and received block/drop
counts. Use the same subscriber load for both settings and say whether a bus
recorder was active; a sender with no subscriber is not representative of a take.
A changed PID/starttime, revoked capture lease or a failed recording invalidates
that comparison. No CPU or hardware timing result has been measured yet.
