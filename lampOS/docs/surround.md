# Starting, setting up and maintaining a lamp

**Not the product's path.** `lamp-surround` runs on no lamp
([deploy/product/README.md](../deploy/product/README.md)): a product lamp is
set up by `lamp-setup` ([setup.md](setup.md)) and updated by `lamp-update`
([updates.md](updates.md)), with the reset plan in `lamp-setup`. This page is
the design and the host-tested crate of task 12, kept as a record.

## What exists and what we take

V1 has working setup and recovery paths with a substantial failure history.
Keep its attempt identities, separation of network and runtime readiness,
network-loss grace and hotspot-client guard, single updater lock, journal
before publication, and the previous installation until health is confirmed.
Its setup redirect can lose the new address when the single-radio hotspot
disappears before DHCP finishes. Its original update race lost the live tree,
staging tree and backup after two requests thirty seconds apart. These are
V1's documented observations, not new measurements by Third.

Rebuild the ownership. `lamp-surround` owns its configuration generations,
setup/maintenance progress and verified releases. Voice, Body, Eyes and each
store keep their own hardware and records. Nothing uses legacy HAL or
os-server. A process being alive does not establish a capability, and a stopped
Body process does not establish that its servos are at rest.

Use [systemd's service model](https://github.com/systemd/systemd/blob/main/man/systemd.service.xml)
for independent startup and restart; capability evidence remains a separate
owner contract. `network-online.target` is a boot ordering point, not an
ongoing link-health monitor ([systemd's explanation](https://systemd.io/NETWORK_ONLINE/)).
For application releases, use [TUF](https://theupdateframework.github.io/specification/)
through [`tough`](https://docs.rs/tough/0.24.0/tough/struct.RepositoryLoader.html):
provision the root independently, enforce signatures, expiry and rollback
floors, and verify every target. Its ordinary datastore is not an atomic
power-loss journal, so Surround wraps the trusted metadata in its own durable
record. An unsigned download never becomes a release.

[RAUC](https://rauc.readthedocs.io/en/latest/integration.html) is a strong
candidate for a future whole-image A/B update, once the actual bootloader and
partition layout are qualified. [SWUpdate](https://sbabic.github.io/swupdate/swupdate.html)
supports wider installer policies; neither supplies this lamp's quiet/rest
handover or capability acceptance. [systemd-sysupdate](https://github.com/systemd/systemd/blob/main/man/systemd-sysupdate.xml)
also has useful file-tree transactions, but adopting it requires a pinned
image version and the same owner-specific acceptance. The immediate work is
signed, immutable application binaries and a recoverable release-set switch.

NetworkManager is an optional explicitly selected adapter, using its
[D-Bus checkpoints and device contract](https://networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.html).
V1 image scripts disagree about which manager owns Wi-Fi. Nothing here
installs a manager, guesses a wireless interface or takes Ethernet away from
its owner. Credentials do not belong in command arguments, URLs, logs or
progress; V1 did expose them there, and that path is not carried forward.

## V1 decisions, piece by piece

These rows supplement the source review in [V1 verdicts](v1-verdicts.md);
they do not change that document's historical verdicts. V1 source paths are
review evidence only, never build inputs for this crate.

| Piece and verdict | Actual V1 behavior inspected | V2 implementation and smallest remaining lamp step |
|---|---|---|
| Power-up, GO-2.19 `ignore` | `system/server/config_watch.go::handleSetUpCompleteChange` sequences network, agent, HAL, voice, volume and a greeting in one function. | `startup::Monitor` plans independent starts and checks fresh capabilities. First installs independent units and supplies verified binary, dependency and calibration facts. No automatic greeting. |
| Setup, GO-1.8 `later` | `system/device/{setup,setup_runtime,setup_failure}.go` use attempt identities, cancel stale attempts and separate network/runtime failures; persisted setup completion can precede runtime readiness. | Durable attempt generations, freshness and bounded deadlines; a LAN stays useful when Voice fails. The local page never infers readiness from saving settings. First supplies the actual reachability path when the single-radio AP disappears. |
| Settings, GO-1.10 `later` | A shared settings file and import-time HAL configuration make a small setting change restart hardware. | Immutable private provider generations, whole-file selection and a separate qualified Voice handover. First implements the supervisor's quiet/exit/fenced-selection contract. |
| Wi-Fi, GO-3.1–3.3 `later` | `system/network/{service,setup_failure,recovery}.go` distinguish failure causes; recovery tests use 90 s grace, 120 s retry, 45 s joins, and no retry with hotspot clients. Image scripts disagree about manager ownership. | An optional NetworkManager D-Bus adapter and injected-clock recovery policy. First selects the real manager/interface, permissions, country and secured recovery profile. Unknown client count suppresses automatic AP retry. |
| Update, GO-3.4–3.7 `later` | `system/bootstrap/state/state.go` writes via temporary file, fsync and rename. The old feed may be unsigned before provisioning and downloads an executable updater script. The shell installer has 49 documented fault cases and a recorded double-request data-loss race. | One maintenance owner, signed TUF metadata/targets, bounded staging, journal-before-action and immutable release sets. First provisions the signing root/feed and implements physical quiet/rest/exit receipts. No downloaded shell, archive extraction or unsigned bootstrap. |
| Reset, GO-2.20 `later` | `system/server/system/factoryreset.go` removes configuration, agent/enrollment/snapshot/Wi-Fi/GELF/persona data, preserves bootstrap state, uses a five-minute cooldown, tolerates wipe failures and reboots. | Each configured store durably freezes, then erases its own records; a missing receipt leaves reset incomplete. First wires all actual personal stores and their reprovisioning release. Calibration, physical privacy and update trust remain separate. No reboot on unproved erasure. |
| Pages, GO-3.24 `later`; image, EXT.31 | `useSetupStatusPolling.ts` polls every 600 ms, tracks a run counter, declares loss after five seconds and can call an unreachable join failed after 80 seconds; `SetupProgressScreen.tsx` retains the LAN address and runtime failure separately. Image scripts record manager conflicts. | Retain address/progress, but a lost page connection means unknown. Small self-contained page, private transport and exact origin. First provisions TLS, a unique setup code and qualified provider presets. Full turn monitoring and whole-image A/B installation are deferred. |

## Process and switches

`crates/surround` builds `lamp-surround`; it does not require legacy HAL,
os-server, a sibling source tree, a cloud account or a provider connection.
Its static HTML/CSS/JavaScript is embedded at build time. All runtime policy,
network integration, persistence and owner transport are Rust.

Maintenance requires **both** `enabled: true` in the private configuration
and `serve --enable`. The optional NetworkManager adapter additionally needs
the `network-manager` Cargo feature, Linux and `network.enabled: true`.
Default configuration has none of these enabled. Disabled service mode may
read existing status, serves an authenticated page and owns its private
listener; it does not create maintenance/configuration records, call owners,
change the network or start a component. An explicitly configured diagnostics
bus is independent of maintenance and publishes service health only.

```sh
cargo build --locked -p lamp-surround
cargo run --locked -p lamp-surround -- --help
lamp-surround doctor --config /etc/lampos/surround.json
lamp-surround plan --config /etc/lampos/surround.json
lamp-surround serve --config /etc/lampos/surround.json
```

Use `fixtures/surround.json` under the crate as a schema example, not a claim
that its paths, board dependencies or network owner are installed. `doctor`
only validates local files/configuration. `plan` prints proposed starts and
executes none, even with `--enable`. Linux clock identity defaults to the
kernel boot ID; host fixtures require an explicit `--clock-id`. All owner
times are `lamp_ipc::monotonic_us()` on that same host and boot, never browser
time or a publisher's unrelated origin.

The default HTTP transport is a private Unix socket. A lamp installation
needs an authenticated local TLS endpoint/proxy that preserves the exact
configured Host and Origin. There is no wildcard host, arbitrary CORS origin
or public TCP listener. `--listen-loopback PORT` explicitly enables a local
preview at `127.0.0.1`; its configuration must name that exact HTTP origin.
`--seconds N` bounds a fixture run (1–86400 seconds). No deployment files or
hardware owners are changed here.

Use absolute, canonical paths: no symlink ancestors, `..`, shared writable
store or overlapping operation/configuration/release/selection/bus roots.
Owned directories are mode 0700 with the service's effective UID; private
files are single-link mode 0600 regular files. Missing final directories can
be created, but insecure existing permissions are not silently repaired.
The operator code is a separately provisioned random 32-byte value encoded
as 64 hex characters in a private file (optional final newline). It is never
a hardware-ID suffix, provider key, URL, command-line value or log field.
There is no production default code in source.

The listener is acquired before adapters or maintenance recovery can run.
A second instance refuses a live socket; stale-socket removal checks UID,
mode, refusal to connect and inode identity. The maintenance worker runs on
a separate thread/runtime, so local storage work and an update download do
not occupy the HTTP executor. One queued command and one active command are
allowed; a durable redacted admission record precedes HTTP acknowledgement.
Losing the request or timing out does not cancel a command already admitted.
The operator must read status before repeating it.

## Written interfaces

| Boundary | Contract |
|---|---|
| Browser → Surround | `GET /`, `/app.js`, `/app.css`; `POST /api/login` with `{"secret":"…"}`; authenticated `GET /api/status`, `POST /api/operation`, `POST /api/logout`. Writes require exact Origin. JSON replies use `status`, `data`, `message`. |
| Operations | Tagged `operator::Command`: `save_provider`, `apply_configuration`, `join_wifi`, `probe_network`, `stage_release`, `apply_release`, `retry_update`, `reset`, `resume_reset`. Unknown fields are refused. Rust command/secret objects do not implement Debug or Serialize; persisted progress is redacted. A successful admission means durable acceptance, not completion. |
| Surround → supervisor | `owner::RequestEnvelope` / `ReplyEnvelope`, one JSON line followed by EOF over a private same-UID Unix socket. Schema, request ID, coordinator session, boot clock, deadline and observation time must match. At most 65536 bytes, one in-flight call. |
| Supervisor commands | `Probe`, `Update(update::Action)`, `Reset(reset::Action)`, `Configure { operation_id, expected, target }`. The actual types in `src/owner.rs`, `update.rs`, `reset.rs` are the wire specification; unknown fields/variants are refused. |
| Startup | `startup::Monitor::{plan,bind,observe,snapshot}` consumes explicitly supplied installed facts, requested launch generations and owner receipts. It returns data; it never spawns, signals or kills a process. |
| Network | `network::NetworkAdapter::{submit,poll}` with attempt/generation/clock correlation; `Unsupported` is the default. The optional manager adapter changes only the explicitly selected device/profile. |
| Diagnostics | Existing `lamp_msgs::Diagnostic` on `diagnostics`, component `surround`, once a second when `bus_root` is set. This reports Surround and known maintenance state, never substitutes for Voice or physical privacy. |

Sessions expire after 15 minutes, at most four are retained, and login is
limited to ten attempts a minute. Cookies are HttpOnly/SameSite=Strict and
Secure except the explicit HTTP loopback preview. Responses prohibit caching,
framing and external resources. Request bodies are capped at 32768 bytes;
eight read and two write handlers have independent capacity, a five-second
handling deadline, and the listener caps 32 connections with an absolute
ten-second lifetime (including incomplete headers and trickled bodies).
These are cooperative async deadlines; they cannot preempt synchronous work
blocking an executor. The maintenance worker's filesystem work is separate.
Ordinary owner actions have 30-second limits; probes have two seconds.
Quiet observation, configuration handover and Voice reset preparation allow
120 seconds to include the required 45-second silence. An earlier action
deadline still wins. Timeout is uncertainty, never proof that an effect did
not happen. The supervisor must deduplicate stable operation/action IDs,
enforce deadlines and ownership at the final effect boundary, and retain
unresolved recovery state across its own restart.

Status uses cached bounded data. Owner proof expires after two seconds;
setup's LAN/runtime evidence expires after five seconds. The worker refreshes
idle observations on a 500 ms schedule, with new commands preempting that
read-only refresh. This is a scheduling interval, not a completion guarantee:
one refresh may await the network deadline (ten seconds) and owner probe.
During a long operation, expired proof becomes Unknown rather than
being refreshed from the cache. The page withdraws readiness on a failed
fetch or expired display lease, preserves the last address as historical
information, disables changes and rejects late responses from old sessions.
`Inhibited` is healthy but explicitly not listening.

The pending-network worker fixture checks status-lock availability inside the
adapter's first poll and between polls. It waits for that observation with a
ten-second fixture watchdog instead of measuring a status read against a
20 ms host scheduling limit. The adapter stays pending; advancing the existing
injected clock by 46 seconds must still fail the join within the original
one-second fixture watchdog. The 45-second product join deadline is unchanged.

## Startup and first setup

The planner considers Body, Eyes, Voice and configured optional parts without
waiting for an optional part to become ready. Missing binaries, dependencies
or calibration are reported explicitly. Requested generations bind exactly
one owner session. A PID, successful spawn, socket connection or stale receipt
never establishes readiness. A component gets at most three launch attempts,
a ten-second initial readiness grace and a one-second retry backoff.
Replacement also needs verified old-worker exit; Body needs rest and Voice
needs admission closure plus 45 seconds of neither hearing nor speaking.
First's service supervisor must implement those proofs and keep one writer
per actuator. Absence of an observation is not proof an owner is absent.

Setup records separate link, cloud and runtime results. Each attempt has a
durable increasing generation, so reuse of a display ID cannot admit an old
completion. Join is bounded at 45 seconds and runtime preparation at 300
seconds. Two qualified authentication-rejection observations can fail early;
an ambiguous supplicant reason is not relabeled as a wrong password. A new
attempt cancels stale results. Restart marks unfinished and previously Ready
attempts interrupted, requiring new qualification, and clears current proof
while retaining the last address for diagnosis.

The NetworkManager feature uses D-Bus checkpoints and a volatile candidate
profile. A join's LAN proof must come from the selected device/candidate;
working Ethernet cannot falsely commit a failed Wi-Fi join. Commit rechecks
LAN, saves the candidate and then destroys the checkpoint. Those last two
manager operations are **not atomic**: a crash or failed checkpoint destroy
after saving can retain the candidate Wi-Fi profile. `CommitUncertain` asks
the operator to check status, and never claims credentials were rolled back.
First must qualify restart reconciliation on the installed manager. No broad
"delete new connections" checkpoint flag is used because it could remove
another owner's connections. NetworkManager does not supply a qualified AP
client count through this adapter, so its value stays Unknown and automatic
retry cannot evict a person from the setup AP. Recovery policy is supplied
and tested separately; `serve` does not guess AP ownership or automatically
activate an unprovisioned recovery network.

Provider settings stage a complete immutable generation: Surround's
versioned `document.json`, Voice's **unversioned** `provider.json`, a separate
`provider.key`, optional instructions, then a hash manifest. All generated
paths point inside that generation. Validation reuses `lamp-gemini` and does
not connect to the provider. Empty/whitespace-only instructions are refused.
`candidate.json` selects staged data; saving never restarts Voice.
`Configurations::select_quiescent` performs whole-generation CAS only when
First's supervisor holds its external quiet/exit fence and the configuration
directory lock. The HTTP worker cannot manufacture that fence.
An applied receipt must prove the same fence, durable selection and a fresh
new Voice owner using that generation; privacy inhibition remains authoritative.
Current, previous and candidate generations survive collection; unselected
generations and abandoned partial writes are reclaimed only after checking
their owned filenames, permissions and links. A damaged current generation
does not prevent choosing a fully verified healthy previous generation.

## Signed releases and rollback

Provision the TUF root through a trusted installation path, independently of
the download feed. Both metadata and target base URLs must be HTTPS, have no
credentials/query/fragment and end in `/`; redirects are refused. Root,
timestamp, snapshot, targets, expiry and rollback checks use Tough. The time
owner must explicitly attest synchronized UTC, agree with the actual host
clock within 30 seconds, and never move below the durable time floor. An
unsynchronized date blocks a new update; it does not remove the installed
accepted release.

The signed manifest (`release::ReleaseManifest`, schema 1) identifies an
increasing `release_sequence`, exact board/architecture/protocol and a data
schema range. Its target name is a single plain filename, such as `stable.json`
(up to 128 ASCII letters, digits, dots, underscores or hyphens; no path).
It contains exactly `lamp-voice`, `lamp-body`, `lamp-eyes`, each
with SHA-256 and byte length. Every binary is separately a signed TUF target
at `binaries/<sha256>/<name>`; local storage uses the same hash/name layout.
The manifest's SHA-256 identifies the complete set. No archive, arbitrary
executable list, shell command or server-provided filesystem path is accepted.
Core application sets are covered; updating Surround itself or a whole image
needs a separately qualified installation mechanism.

Default bounds are 256 KiB metadata, 64 KiB manifest, 128 MiB per binary,
384 MiB binary total, 1024 targets, 64 transport requests, 16 root updates per
verification, 16 retained accepted sets and 128 binary directories. Network
waits are capped at 120 seconds. This timer cannot preempt stalled local
hashing/fsync; neither it nor these host tests is a storage-latency guarantee.
Tough's working cache is temporary; authenticated progress, roots, accepted
sets and rollback/time floors live in a strict atomic envelope. An interrupted
verification marker remains until target reads finish. Corrupt partial trust
blocks new verification while already accepted releases remain recoverable.
Root history is deliberately capped at 64 proofs and is never silently
forgotten; reaching it requires an explicit trusted-root maintenance procedure.

`Selector::collect` holds the selection lock, protects current, previous,
explicit staged candidates and the highest accepted set, then takes the trust
lock. It durably removes unselected acceptance records before deleting only
checked private hash directories. A power cut leaves collectible leftovers;
it does not erase a referenced accepted set. Root chains, signature evidence,
version and time floors are never collected. Thus repeated normal updates do
not consume a permanent 16-update quota. Foreign files, symlinks and hardlinks
block collection rather than being removed.

Installation journals every next action before it can be released:

1. Inspect fresh supervisor/owner/selection state, then observe a complete
   45 seconds of neither hearing nor speaking.
2. Atomically close Voice admission under an exclusive supervisor fence,
   stop Voice and verify all its workers are gone before proceeding.
3. Ask Body to block new goals and prove calibrated rest; then stop Body and
   Eyes with exit receipts. Killing Body is not proof its servos stopped.
4. Under the same fence, CAS one whole release-set selection. Rehash the
   candidate before selecting; never switch three independent paths.
5. Start Body, Eyes and Voice. Accept only fresh, generation/session-bound
   capabilities, preserving privacy inhibition, before releasing the fence.

A failed candidate is blocked, recovery reinspects under fresh ownership and
restores the original accepted set through the same order. A missing first
installation returns to no selection, not fictitious healthy service.
Unproved recovery remains failed and requires explicit `retry_update`, with
at most four recovery retries, 256 steps per operation and 32 blocked digests.
A manual rollback that fails and recovers the original set is a failed
request, not successful delivery of the requested older set. Durable
selection generations prevent an A→B→A stale CAS. Corrupt current binary data
does not prevent authenticating an intact previous set for rollback.

Tough 0.24.0 is source-pinned in `vendor/tough` with both upstream licenses and
per-source hashes. Its runtime source is unchanged; the manifest disables
Rustls's default provider so Cargo feature unification does not break the
existing Gemini Ring client. Surround's HTTPS client explicitly selects Ring
and platform certificate verification. Tough still uses native AWS-LC for
TUF signature operations; Linux ARM64 therefore needs a matching C/C++
compiler. The pinned non-FIPS `aws-lc-rs` 1.18.2 / `aws-lc-sys` 0.46.0 use
pre-generated ARM64 bindings; CMake, Go and bindgen are not requirements for
this selection ([upstream Linux requirements](https://aws.github.io/aws-lc-rs/requirements/linux.html)).
The wrappers use ISC/Apache-2.0 licensing; the bundled native source's full
SPDX expression additionally includes MIT/BSD-3-Clause/MIT-0 alternatives,
as recorded in `aws-lc-sys`'s pinned manifest. `vendor/tough/LAMPOS.md` records the precise packaging change and
omitted upstream dev fixtures. The repository's own release tests exercise
signed metadata, corruption, expiry, root rotation, interruption and rollback.

## Reset and durability

Reset requires the literal confirmation `erase-personal-data`. Voice is
prepared first, then every configured personal store; no store is erased
until all owners have durably frozen new writes. Voice's receipt includes
quiet, closed admission and gone workers. Erase actions carry the expected
owner session and stable reset identity. A changed owner, stale receipt,
storage failure or lost response cannot become success. Progress records a
completed prefix, and explicit resume prepares every owner again before
continuing. On restart an unfinished reset is Incomplete, not automatically
replayed. Barriers must remain durable until a separate explicit reprovisioning
procedure; this crate does not release them by restarting a process.
The owner integrations must also preserve existing software privacy inhibits,
as required by [Controls](controls.md); clearing one remains a separate explicit
owner operation, never a side effect of personal-data reset.

The default schema lists Voice, Memory, Schedule, Tasks, Settings,
Configuration and Network. First must include every actual personal store;
an omitted store is outside the claim. Surround never recursively erases
foreign directories. Erasure confirmation describes each owner's records,
not secure removal of flash wear-leveling history, backups or cloud copies.
Reset identity history has a deliberate 64-operation lifetime bound; it
refuses rather than forgets old erasure identities. Ordinary maintenance
retains the most recent 128 IDs plus a monotonic admission generation, so
repeated network probes do not exhaust a lifetime command counter.

Writes use bounded private files, no-follow directory traversal, a temporary
file, file sync, atomic rename and directory sync. Directory locks prevent
concurrent owners. Partial temporary files are never accepted as committed
state. A corrupt trust, selection or maintenance record fails closed; no
empty-state fallback manufactures permission. Filesystem durability still
depends on the lamp's filesystem/device and needs destructive power-cut
qualification on disposable lamp data.

## Host verification and lamp handoff

The [host record](verification/surround-host-20261014/results.json) preserves
exact commands, source hashes and outputs; the
[failure ledger](verification/surround-host-20261014/failures.md) keeps failures
and their corrections. On the Intel i7-10700K development Mac (64 GiB,
macOS 26.6.2, Rust 1.96.0), all 260 default-feature and 263 all-feature Surround
tests passed, with none ignored. The existing Gemini regression passed 250
tests with its two pre-existing long soaks ignored. Strict all-target Clippy,
formatting, workspace build and nine browser-script tests passed. A separate
source-only copy built the entire workspace and passed the same 263 all-feature
tests and nine script tests. Task integration is recorded in
[Third's log](https://github.com/autonomous-ai/lampOS/blob/lamp-third/lampOS/docs/work-log-third.md). Tests include real
private Unix sockets, scripted NetworkManager calls, signed TUF fixtures,
state-machine power-cut replay, private filesystem failures, strict HTTP
origin/session/bounds and the actual browser script. These are development
Mac results, not lamp measurements. No real provider credential, Wi-Fi change
or signing service is used. Authored replay time is always labeled separately
from measured execution time.

First can run the deterministic installation scenarios with one command:

```sh
cargo run --locked -p lamp-surround --example update_replay
```

It must report the happy installation, failed-candidate recovery, missing Body
rest refusal, disabled operation and post-switch power-cut recovery. The
fixture's action durations are written values, not physical measurements.
The corresponding local-handler/persistence benchmark is:

```sh
cargo run --locked -p lamp-surround --example operator_bench
```

It reports first and 100 repeated status/admission measurements, including
file and directory sync before admission, then status during an authored
500 ms minimum dependency delay. It verifies 102 redacted durable records.
This boundary excludes sockets, browser, provider, network and physical
effects; run it on the lamp before assigning its numbers to that machine.
The actual worker's stalled-idle-probe test has a different boundary, channel
send to durable admission: it measured 331.266 ms in its final focused run,
330.900 ms in an earlier isolated run, and 1605.898 ms under concurrent fsync
load. Thus the provisional 100 ms admission target is **not established for
the complete service**, even on this Mac. The fixture benchmark's lower
numbers do not override that result. HTTP reports uncertainty if admission
has not been acknowledged within two seconds; status runs independently.
For a source-only build, copy `lampOS/` including `vendor/` to a separate tree,
exclude `target/`, keep credentials/calibration outside it, and run the same
Cargo gates there. NetworkManager integration is compiled separately with
`--features network-manager`; on a non-Linux host it remains unavailable.

Before customer handoff, First must provision the service UID/private paths,
unique setup code and TLS origin, actual manager/device/recovery profile,
qualified provider defaults, independent component units, trusted release
root/feed and all supervisor/store contracts above. Then run `doctor` and
the disabled service, observe status with physical privacy both ways, and
enable only the adapters whose receipts have been qualified.

On the lamp, measure first/repeated page status and command admission
(provisional target: each within 100 ms on the local network), separately
from admission-to-completion, while the provider is slow and a release is
downloading. Record network path, binary hash, sample count and percentiles.
Confirm that speaking resets the 45-second quiet window; old Voice workers
cannot survive handover; Body reaches rest before exit; no stale capability
unlocks output; failed starts return to the whole previous set; and power
cuts at each journal/selection boundary recover without mixed binaries.
Test first Wi-Fi join, wrong credentials, lost DHCP, retained Ethernet,
lost page connection, manager restart after profile save, full disk, corrupt
staging, expired metadata, stale UTC, duplicate clicks and interrupted reset.

**Most likely wrong on the lamp:** NetworkManager ownership/checkpoint and AP
reachability assumptions, then the supervisor's real worker-exit/servo-rest
proofs. Qualify those first. A successful Mac test, live PID, D-Bus return or
supervisor receipt without measured hardware evidence proves none of them.

## Not shown

Third has no lamp. Network handover, hardware-owner quiescence, actual service
startup, power-loss recovery on the device's storage, a production signing
root/feed and customer setup on the assembled lamp have not been qualified.
No deployment or hardware-owner implementation is changed by this task.
