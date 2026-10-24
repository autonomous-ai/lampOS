# Published Skills routine-state readers

Part of #2809. This narrow fixture uses the actual published Linux ARM64
`lamp-skills` from releases 6408 and 6498. Each direction runs A → B → A
against private state; released processes own the live writes.

[Artifact identities](artifacts.json) retain source commits, public asset IDs,
size and digest. Downloaded bytes were checked against the API metadata and
the corresponding decoded release manifest, including an AArch64 ELF header.
This is artifact consistency, not verification against an independently
trusted signing key. The latter remains #1812.

## Scope and boundaries

- Daily claims: the released daemon persists a claim before its Policy answer.
  A replacement reader must not claim it again, while a distinct trigger
  supplies a positive control. The original release then reads both claims.
- On/off preferences: the released `asked` helper enables, disables and enables
  a synthetic local skill through the existing private protocol. Its manifest
  and the `on`/`off` companion files must agree after every replacement.

The fixture uses private synthetic peers and no account, provider, Store
activation, live skill, lamp, microphone, speaker or actuator. It does not
qualify updater installation, the full skill loader/provenance area, routine
rested-night continuity, real clock synchronization or power-loss durability.
Those boundaries remain separate; no build-row increase follows from merely
adding these tests.

Both released sources have identical daily, routine and package persistence
code. Their executable hashes differ, so this qualifies process replacement
across those fixed published artifacts, not every newer implementation.
The intervening Skills presence-duration change is outside these cases.

No parser rules change. Daily schema 1 remains readable and migrates only on a
new claim; malformed or newer schemas remain refused without resetting their
file. Added fields are tolerated but discarded on rewrite. The unversioned
on/off files ignore malformed names. No new enum value is made acceptable.

## Execution

The four named cases are ignored in the ordinary crate suite. The dedicated
`lampOS published routine state compatibility` workflow executes them on
native Linux ARM64 with pinned public binaries, saves output and exit status,
checks source hashes before and after, and retains regular evidence files.
It excludes sockets, FIFOs and copied executable binaries from the archive.
Ordinary ARM64 and strict Linux Clippy are separate required gates.

```sh
LAMP_COMPAT_SKILLS_6408=/artifacts/6408/lamp-skills \
LAMP_COMPAT_SKILLS_6498=/artifacts/6498/lamp-skills \
LAMP_COMPAT_OUTPUT=/tmp/pair-output \
cargo test --locked --release -p lamp-skills --test release_pair -- --ignored --nocapture --test-threads=1
```

The output parent must exist and be short enough for Unix socket paths.
Results will be bound to the exact PR head and native run. Compilation or
ordinary-suite skips are not execution evidence; failed attempts are retained.

The first executed native run, `40053783946` on `51cc5dfb9`, passed both on/off
directions and failed both daily directions: **2 passed / 2 failed / 0 ignored**.
The fixture omitted the consumed watch's negative reset when changing phase.
It now sends a separate neutral phase and requires exactly that named negative
reset before the next positive control. Exact three-event refusal checks and
all durable-state assertions remain intact. The
[original text evidence](https://github.com/autonomous-ai/lampOS/tree/a2e456672/lampOS/docs/verification/routine-state-failed-20261024)
is preserved. The corrected execution on `561c6a21b` passed all four cases
in [native run 40054631773](https://github.com/autonomous-ai/lampOS/actions/runs/40054631773):
**4 passed / 0 failed / 0 ignored**. Ordinary ARM64 run `40054631836`
separately reports **11,128 PASS + 1 LEAK**, zero failed or unrun, and 75
skipped; the existing leak is not counted as PASS. Strict Linux Clippy run
`40054631774` passed. These results belong to that authored head on base
`59c4ac795`; a later folded/current-base head must receive its own gates.
Rested-night, installation and physical output remain outside these four cases.
