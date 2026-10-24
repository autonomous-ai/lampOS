# Published Cloud provenance readers

Two ignored tests exercise actual published Skills `pack`/`install` writers and
the opposite published Cloud `skills.list` consumer. Today's Skills or Cloud
domain reader never substitutes for a released process. The Rust driver uses
only generic JSON/file inspection and the existing MQTT-v5 dependency's codec.
No new dependency is added.

| Part | Release | Bytes | SHA-256 |
|---|---|---:|---|
| Skills | 6408 | 8,807,104 | `2c6bc8f34c9fd4a56784a54fa2d03b1e76be9d1833ccf37db905f88dba67d3d5` |
| Skills | 6498 | 8,786,048 | `87e500127793db88df8bcd7af793879faeee95f36a8e0d2ef8483c59fef26984` |
| Cloud | 6408 | 10,802,168 | `9f3abad362555ad11177bdf41706e91d740bca8689928a8555c4ce3f257fc664` |
| Cloud | 6498 | 10,804,480 | `6d4d566d837dad9df137b475557f330deb62a84405128b879686368a9de30bf4` |

The payloads come from public tags `lampos-lamp-voice-6408` and
`lampos-lamp-voice-6498`, source commits
`ee53765c8d014eb00e41c1a5a7054d2546c18bdb` and
`846b3d203fc00ec3886d71f7673d8cc29cc4a634`. Cloud executable bytes differ;
Cloud's `skills.rs` and Skills' `package.rs` provenance reader are identical
between those sources. This qualifies a shipped pair, not a parser change.
All four inputs require exact SHA-256, size and ELF64 little-endian AArch64
identity before execution. Only private copies are made executable; originals
and copies are rechecked, including modes, after use.

## Isolation contract

Never run these ignored tests on a lamp or an ordinary host. They refuse unless:

- Linux/aarch64 has only the loopback interface and no IPv4 default route.
- `/var/lib` is a tmpfs mount, and `/var/lib/lampOS` and
  `/var/lib/lampos-skills-state` are absent before each case.
- `/run/lamp-cloud-pair-isolated` is a plain, singly linked, root-owned file,
  unwritable by group or others, with the exact bytes
  `lamp-cloud-provenance-pair network-none tmpfs-v1` followed by a newline.

The dedicated runner must supply a fresh network-disabled container with a
read-only root, private `/tmp`, `/run` and `/var/lib` tmpfs, and no host bind
mounts, devices or credentials. The marker is a workflow-created isolation
assertion, not independent proof of Docker configuration. Fixed paths are
created only after all admission checks and removed only after seven exact
owned child reaps. A failure retains those paths and all partial proof for the
container's collector; the fixture never cleans an inherited fixed path.

Set `LAMP_COMPAT_SKILLS_6408`, `LAMP_COMPAT_SKILLS_6498`,
`LAMP_COMPAT_CLOUD_6408`, `LAMP_COMPAT_CLOUD_6498` to absolute artifact paths.
`LAMP_COMPAT_OUTPUT` must be an existing absolute short private evidence parent
(the runner uses `/tmp/pair-output`). Build the exact test target during the
runner's preparation phase, then execute its selected binary serially inside
the isolated runtime:

```sh
cargo test --locked --release -p lamp-cloud --test release_pair --no-run \
  --message-format=json
# Inside the isolated runtime, using the executable selected from Cargo JSON:
/tmp/pair-input/cloud-release-pair --ignored --nocapture --test-threads=1
```

Ordinary Mac tests compile these cases but do not execute them. Dedicated
native execution remains required; local compilation is not qualification.

## What each case checks

A's actual Skills CLI packs and installs an inert synthetic version `1.0.0`.
B's actual Cloud reads the fixed product Skills folder through a loopback-only
MQTT peer. B's actual Skills CLI then packs and replaces it with `1.1.0`;
A's Cloud reads that replacement, and B's Cloud reopens it again. The tests run
this sequence in both 6408 → 6498 → 6408 and reverse directions. The row must
remain loaded, installed, removable, enabled, with its exact title, description,
permission, version and actual package SHA-256.

Each Cloud has an entirely cleared environment plus minimal PATH/LANG and only
`LAMP_CLOUD=on`, `LAMP_SKILLS_PAGE=on`, private `CREDENTIALS_DIRECTORY` and
`STATE_DIRECTORY`. Its dummy account names `127.0.0.1`, the peer's ephemeral
port, dummy login and `compat/fa`/`compat/fd`; the backend base and key are empty.
No HTTP ping URL exists. Only `--bus-root` points at a private local bus. Every
other feature switch is absent. The peer uses CONNECT/CONNACK,
SUBSCRIBE/SUBACK and QoS-1 PUBLISH/PUBACK, distinguishes initial `info`, and
requires both acknowledgement and the exact successful `skills.list` reply.
There is one outstanding request at a time.

In both initial and original Cloud sessions, the driver explicitly substitutes
malformed JSON and an added unknown metadata field in `.package.json`. These
are **synthetic negative controls**, not released-writer output. Both must
return `sha256: null` while preserving the same loaded row. Original metadata
bytes are restored and the positive SHA is required again. No claim is made
that a changed manifest format number is rejected: the released reader does
not validate that number when consuming provenance.

Every read compares complete installed membership, bytes, modes, inode/device
and modification times, plus all source/package inputs and the off state.
Before/after snapshots retain raw bytes and original metadata receipts.
Definitions are made read-only. Cloud never executes these definitions.
Each live Cloud is cut and reaped; each CLI requires actual successful exit.
Finite socket/packet/loop/log and case deadlines bound the fixture. Private
files replace process output pipes, so an undrained output pipe cannot block
child exit. Failure cleanup targets only the directly owned child; these
specific released pack/install/list paths start no subprocesses.

## Evidence and limits

Each unique `cv-*` directory retains raw command stdout/stderr, actual MQTT
JSON, immutable inputs, actual packages, metadata counterexamples, snapshots,
artifact pins, transcript and terminal `result.json`. Success requires seven
matched spawn/reap identities: four CLI exits with code 0 and three Cloud
SIGKILL exits. Nine Cloud queries and four synthetic metadata substitutions
are asserted. The collector must exclude executable copies named
`lamp-skills-6408`, `lamp-skills-6498`, `lamp-cloud-6408` and `lamp-cloud-6498`.

This is software-only provenance continuity. Dummy broker authentication is
not real account authorization or TLS acceptance. Local package installation
does not establish Store signing trust. The fixture establishes no provider,
device, sound/motion, updater installation, clock migration, newer-schema
policy, mid-write cut, filesystem power-loss durability or whole-lamp update
acceptance. Retained hashes establish artifact consistency, not an independent
trust root for the public release.

## Executed qualification

Original head `2ad786f91` passed dedicated ARM64 run
[40070585559](https://github.com/autonomous-ai/lampOS/actions/runs/40070585559):
**2/0/0 ignored**. Ordinary ARM64 **40070585476** passed **11,194 PASS +
1 LEAK**, zero failures, 92 skipped; strict Linux Clippy passed. These
results remain bound to that head and tested preview `9bb699cf5`, rather
than automatically qualifying later documentation carries.

The container's resolved base/runtime image identities and package-build
logs are retained. Rustup, apt, the image tag and major-version actions are
not fully locked inputs; these results do not establish reproducible harness
bytes across a future rerun. Released application payloads themselves remain
strictly pinned and checked before execution.

Case metadata now names each actual Skills and Cloud stage explicitly.
`pack-a`/`install-a` use the initial writer, `pack-b`/`install-b` use the
replacement. Cloud runs `opposite-cloud` on the reader release,
`original-cloud` on the writer release, then `return-cloud` on the reader
release. This removes the ambiguous former `return_reader` field; per-stage
spawn pins remain the execution evidence.
