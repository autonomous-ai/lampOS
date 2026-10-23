# Head-reader transition review preparation

Read on 23 October 2026 at integration `0f6f728b2`. This is an independent
source audit for Second's forthcoming update fix, not a failure claim
against an unpublished implementation. The existing settings-on reproduction
and its HOLD on #2948 remain separate.

## Two conditions

Let V mean Voice's `LAMP_HEAD_TOUCH=on`, M mean a valid absolute
`--controls-mute` path, and E mean Expression's `--head-touch`.

| V | M | E | Source behavior and required treatment |
|---|---|---|---|
| Off | Either | Off | Legacy delivery is permitted. |
| On | Off | Off | Voice reads taps but ignores Hold; legacy is excluded. |
| On | On | Off | Voice can mute for head sleep; legacy is excluded. |
| Off | Either | On | Expression can sleep without Voice's head mute: reject. |
| On | Off | On | Expression can sleep without Voice's head mute: reject. |
| On | On | On | Coherent head sleep; legacy is excluded. |

`voice/src/hand_controls.rs` requires V and M in `head_sleep`; its
`head_event` ignores Hold without the mute record. Voice's option parser
requires the mute path to be absolute. `expression/src/alive.rs` accepts a
head Hold independently and can enter sleep without a fresh Voice mute.
The existing install census checks the embedded product units; it does not
prove these combinations for units already installed on a lamp.

A conservative transition may reject other partial combinations too, but
Voice-only is not itself evidence of a sleeping-face/open-microphone fault.
The two conditions are: either reader excludes legacy, and Expression's
head sleep requires both Voice prerequisites.

## Resulting units and unknown readers

`round::switch` applies settings before writing carried units. Reading only
the unit files at apply time can therefore choose legacy for old readers
and retain it beside newly enabled readers later in the same round. Review
the accepted resulting unit plan as well as the installed state, including
files preserved because they are locally customized.

`installed_units` skips symlinked unit files and executables it cannot
recognize. `linked_program` recognizes relative 64-hex current-link targets.
A reader absent from this inventory is not proven disabled.
`unit::environment` is a limited text parser: it does not resolve drop-ins,
EnvironmentFile, resets or section scope. Unknown configuration must remain
explicitly unknown rather than silently becoming an off reader.

## Actual bytes and recovery

`apply::plan` protects customized files by comparing the actual file hash
with the stored build hash. `apply::apply` stores `step.setting.sha256` in
both `Before.wrote` and the next settings record. `apply::back` restores a
file only when its current hash matches `Before.wrote`, or recognizes it
as already restored when it matches the old bytes.

If the fix derives different pins bytes from the reader combination, the
journal and state must record those actual bytes. Review a repeat update,
crash rollback and interrupted-update recovery, preserving calibration,
unrelated fields and file mode. A successful first update alone cannot
prove the later update and rollback paths.

No lamp was accessed and no runtime behavior was changed for this audit.
