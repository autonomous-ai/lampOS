# Actions: doing what the lamp is asked

`lamp-actions` (`crates/actions`) carries out what the lamp is asked to do
to itself. On Codex's design page it is the "executive"; in the loop
drawing it is the arrow from the lamp to its actions. It is step 3 of
[brain-build.md](brain-build.md). What the voice expects of it, and the
wire between them, is in [tools.md](tools.md).

## What it is for

The model that holds the conversation can talk and cannot do anything.
When a person says "a bit quieter", the model names an action
(`volume_down`). This program then does five things, and is the only
place where they are done:

1. checks that the action is one it offers now, and that nothing came with
   it but what that action takes;
2. (later) asks "shall I?" for the actions the owner said need a yes;
3. hands it to the part that owns the hardware, or changes the one control
   it owns itself;
4. reads back that it happened;
5. answers with what was established, in a sentence the lamp can say.

It is general. The conversation is its first caller; the decision model,
the touch pad and schedules are to call it the same way. It knows nothing
about the provider: a call is one line on a local socket.

## The process

    lamp-actions --dir ABSOLUTE_DIR [--mixer-card NAME --mixer-control NAME [--volume-ceiling 1..100]] [--bus-root ABSOLUTE_DIR]

In `--dir` it keeps `tools.json` (what it can do now), `call.sock` and
`ready`. Calls are taken one at a time, and two changes to one control
must not cross. A caller has
one second to send its line. A second `lamp-actions` on the same directory
is refused while the first answers. It cannot catch a signal; killed, it
leaves the call it was in unanswered, and the voice tells its model so.

Only what can be done now is offered. With no mixer given there are no
volume actions; the light's are offered only while the ring's owner is
saying what it shows, and are withdrawn within about two seconds of it
stopping. The list is looked at again every two seconds
(`server::LOOK_AGAIN`). `server::serve_with_clock` reads that period, and
the once-a-second health report's, on a clock its caller gives. The lamp
runs on the host's clock; the light's test moves its own on
(docs/realtime-tests.md).

## Optional reminders and timers

With the exact environment value `LAMP_SCHEDULE_TOOLS=on`, Actions appends
six Schedule tools to its ordinary catalog. Other values, including `ON`,
leave its original catalog and dispatch unchanged. Voice separately gates
the same declarations and routing; enabling Actions alone is insufficient.
`LAMP_SCHEDULE_SOCKET` selects the absolute Schedule socket, defaulting to
`/run/lampos-schedule/call.sock` only when enabled. A disabled configuration
does not inspect that setting or connect to Schedule. No deployment file is
changed by this integration.

```sh
LAMP_SCHEDULE_TOOLS=on \
LAMP_SCHEDULE_SOCKET=/run/lampos-schedule/call.sock \
lamp-actions --dir /run/lampos-actions
```

Schedule must already be configured with its own private store, trusted clock
and IANA default zone. Actions does not start Schedule or infer its clock.
The tools remain declared while enabled; an unavailable store produces an
honest failure when called. It does not disable unrelated volume/light calls.

| Tool | Exact model arguments | Private operation |
|---|---|---|
| `set_timer` | `text`, whole `seconds` | Set with an elapsed `After` deadline |
| `set_reminder` | `text`, exact `time` in `HH:MM`; optional `day`, `zone`, `fold` | Set one civil `At` deadline |
| `repeat_reminder` | `text`, `time`, `every` (`daily` or `weekly`); `weekday` required only for weekly; optional `zone` | Set a local daily/weekly recurrence |
| `list_reminders` | Optional boolean `pending_only`, optional nonnegative `after_id` | Read one bounded page |
| `cancel_reminder` | Positive `id` from the current list | Cancel that timer or whole series |
| `snooze_reminder` | Positive `id`, whole `seconds` | Get the exact current pending delivery, then snooze it |

The catalog directs relative requests to `set_timer`, even when the person
says "remind me": "in two minutes" uses the requested label and `seconds:120`,
"in ten minutes" uses 600, and "in an hour" uses 3600. `set_reminder` is for
a requested clock time such as "at nine tomorrow morning". Converting a
duration to its minute-only `HH:MM` argument can make it fire almost a minute
early. Both descriptions explicitly rule out that conversion. The published
catalog regression checks this guidance; live model selection still needs
the [lamp follow-up](schedule.md#relative-time-follow-up-on-the-lamp).

Labels contain 1–300 UTF-8 bytes, nonblank, with no control characters.
Durations are 1–31,536,000 seconds. Time is exactly `00:00` through `23:59`.
`day` is `today`, `tomorrow`, `next`, or a valid `YYYY-MM-DD`; omission means
the next strictly future occurrence. `zone`, if supplied, must name an IANA
zone. `fold` is `earlier`, `later` or `reject`; Schedule rejects an ambiguous
one-off time unless the caller makes that choice. Weekdays are full lower-case
English names. Daily requests cannot carry a weekday. Unknown arguments and
malformed values are refused before any Schedule connection.

The six new model schemas use the existing provider's small Schema vocabulary.
They deliberately omit `additionalProperties` and `maxLength`; Actions still
enforces exact fields and UTF-8 byte bounds at runtime. Provider schema
acceptance must be qualified on the configured lamp model.

For mutations, Actions generates a local random request ID and takes the
store ID/revision from a fresh Status reply. Snooze obtains both that guard
and the pending delivery from a single Get reply. The model never supplies a
guard, delivery token, Unix timestamp or acknowledgement. Schedule resolves
the time and returns the durable result. Success includes `status:committed`,
the item ID and the resolved local deadline where relevant; Actions checks
the returned store/revision, identifiers and canonical deadline before
confirming it. Setting an item does not claim its future audio was played.

A successful response uses the existing Actions envelope with a structured
`result`, for example:

```json
{"ok":true,"result":{"status":"committed","id":1,"cancelled":true,"message":"The selected timer or whole reminder series is cancelled."}}
```

The timestamp and local string in real replies come from Schedule, never from
model arithmetic. List results contain complete labels, item IDs, state,
resolved local time and recurrence metadata. They fit Voice's 4,096-byte
provider budget with its 16-byte reserve, including JSON escaping. If a page
is shortened, `next_after_id` is the last included item, `complete` is false,
and another call with that cursor reads more. A complete `pending_only:true`
page describes the filtered list, not every active reminder. No label is
silently cut to fit a page.

Each accepted model call gets one bounded adapter attempt. The 1,250 ms
absolute deadline covers all status/get/mutation hops, nonblocking socket
connection, request writes and response reads. JSON on the Schedule link is
at most 8,192 bytes per object, plus its newline. A full Unix accept queue is
reported unavailable rather than waited on indefinitely or retried. The
healthy local target is 250 ms from adapter entry to durable receipt, distinct
from Voice's spoken feedback and future reminder playout. The target has not
been measured on a lamp; an operating-system stall is not made impossible by
a userspace deadline.

A conflict is returned without obtaining a new guard. A lost/malformed reply,
storage failure after possible commit, or an unknown mutation result says:
“I could not confirm that change. It may have happened. Please list the current
reminders before asking me to make the change again.” An unavailable store
does not produce a success sentence. Arbitrary service errors and reminder
text are not copied into Actions' failure logs.

There is no new durable Actions call ledger. A call lost between Voice and
Actions may already have changed Schedule; neither side automatically repeats
it with a fresh guard. Reissuing the same model intention as a new call can
create a second reminder, so inspect the current list first. This does not
promise exactly-once tool execution. The separate due-speech consumer owns
durable delivery tracking and acknowledges only qualified completed playout;
these six tools never send `ack` or speak a due reminder themselves. See
[Schedule](schedule.md) for that boundary.

## The actions

| Action | What it does | What it says back |
|---|---|---|
| `volume_up` | One step louder, to the ceiling at most | "My volume is now 70 out of 100; it was 50." or that it is already as loud as it is allowed to go |
| `volume_down` | One step quieter, to the floor at least | The same form, or that it is already as quiet as it goes without being muted |
| `volume_mute` | Volume to nothing, remembering where it was | "My speaker is muted." |
| `volume_unmute` | Back to where it was, or the middle if that is not known | "My speaker is back, at 70 out of 100." |
| `light_on`, `light_off` | On, in the colour and at the level it had; off, which leaves a faint glow | "My light is on, blue." / "My light is off. The faint glow that is left shows I am still listening." |
| `light_colour` | One of ten named colours, and nothing else; its description names all ten and says white and warm are colours, not brightness ("make your light white" and "make it whiter" went to `light_brighter` on lamp-4ace, the first builder, 17 Oct 15:26). Switches the light on | "My light is blue now." |
| `light_brighter`, `light_dimmer` | One level of five either way, keeping the colour. Brighter brings a light that is off back on; dimmer stops at the lowest and does not switch off. Dimmer is also "softer" or "lower"; brighter is not for "white" or "whiter" | "My light is a step brighter, at level 4 of 5." / "My light is already at its dimmest. I can switch it off if you like." |
| `light_warmer`, `light_cooler` | One step along the light's warmth, white, the amber warm, orange (`light::WARMTH`, each with less green and blue), keeping its level. A light in another colour becomes warm (warmer) or white (cooler), said as what it is now, not as a step. Each stops at its end and switches a light on. For "warmer", "cosier", "more golden", "cooler", "whiter", "a cooler white", "more like daylight"; "softer" is `light_dimmer`'s, and a colour by name ("make it warm", "make it white") is `light_colour`'s (the first builder, 17 Oct 14:16: "Can you make your light a little warmer?" was answered "my light is a fixed color", since nothing stepped the colour) | "My light is a step warmer, orange now." / "My light is white now." (from blue) / "My light is already as cool as it goes, white." |

A failure is said as one: "I could not read my speaker's volume.", "I
could not confirm my speaker's volume changed.", "I asked for it but could
not confirm my light changed.", and for anything not offered "I can't do
that."

While the ring draws something over the person's light (a skill's glow on
`light/request`, #940, or a self-test's colour), what it last drew
(`light/drawn`) differs from the person's light (`light/state`). Then
nothing is "already" so: the person's light is asked for even when it is
theirs, since the person's request is what ends the glow, and the answer
is said as a change ("My light is blue now."). The newest light drawn
counts whatever its age, since during a turn the ring draws the turn's
faces; at worst it asks once more for the light that is already theirs
(the reviewer on #993).

The speaker's volume is one ALSA control (on lamp-4ace, `Speaker` on the
card `allwinnerhdmi`: a hundred points over 30 dB). This program is its
only writer. The light is not written here: the wanted light is published
on the bus (`light/command`, `lamp_msgs/Light`: on or off, a colour by
name, a level from 1 to 5) and what the ring's owner reports
(`light/state`) is read back for up to 0.6 s. The ring's owner is the
voice's ring worker ([ring-choreography.md](ring-choreography.md), "The
lamp's own light"), which is also where each colour's and each level's
look is decided.

## The app's two calls (#1396)

The Autonomous app's volume slider is answered through the executive too, so
it stays the speaker's only writer: `lamp-cloud` asks on the same socket for
`volume_level`, and `volume_to` with `{"volume": 0..100}`, a share of the
allowed range. They answer in numbers, `{"ok": true, "level": {"volume",
"raw", "max_volume"}}`, as V1's `volume.*` has them, and a set is read back
within a point or not done (`Volume::level`, `Volume::to_share`). They are
never in `tools.json`, so the voice's model is never offered them, and the
voice never asks them. The journal says their names and whether they were
done, as for every call.

## What was taken from V1, and what was not

The owner's rule (2026-10-13): V1 is not the reference; each part of it
is judged before anything is carried over. For V1's intent component
(`system/intent`), read in full by a second reader and spot-checked:

| In V1 | Verdict | Why |
|---|---|---|
| The model names a command and never a payload; steps and limits are the code's | Kept | It is what makes a wrong or hostile request harmless |
| Read, change, read back; a result that says what happened | Kept | "Done" must mean done |
| A closed list of ten colour names | Kept | A name a person says, and nothing to get wrong |
| Finding the command by matching the words of the request against lists | Not taken | The conversation model hears the request itself and names the action. In V1 only exact strings ran locally; the rest went to a hosted model with a three-second budget |
| The model moving the body by writing `[HW:/path:{json}]` tags into its reply, extracted by a pattern | Not taken | A reply is for a person; an action is a call with a result |
| Louder by a tenth of the range, quieter by half | Rebuilt | One "a bit quieter" took three or more "louder" to undo. Here both are one step of 20 points (about 6 dB), and quieter stops at a floor of 10; silence is mute's. The step is reasoned, not yet heard by a person |
| The light as arithmetic on red, green and blue (halve; a quarter more; rounding that ends in black) | Rebuilt | A colour by name and one of five levels. The pixels are the ring's owner's, and "dimmer" can no longer round a light into darkness: off is asked for by name |
| On is always warm white | Rebuilt | On is the light as it was left. A person who set it blue and switched it off expects blue back |
| Seven scenes, following, music, "stop talking" | Not yet judged | Each when it is built |

## Not shown

The 2026-10-14 Mac x86_64 checks passed 33 Actions tests, including 14 new
Schedule cases, and seven shared-client tests. They exercise all six tools
through a real Actions server and a real Schedule service, malformed input
without IPC, bounded escaped list pages, conflicts, lost replies, slow
dependencies, and a real Schedule engine whose persistence fails before or
after writing its snapshot. The additional full-accept-queue client test is
Linux-only and was not run on this Mac. Commands, from `lampOS`:

```sh
cargo test --offline -p lamp-actions --all-targets -- --test-threads=1 --nocapture
cargo test --offline -p lamp-schedule --test client -- --test-threads=1 --nocapture
cargo clippy --offline -p lamp-actions --all-targets -- -D warnings
cargo clippy --offline -p lamp-schedule --lib --test client -- -D warnings
```

Both Clippy checks and scoped formatting passed. The timing fixture measured
21 successful calls from client entry before connecting to the real Actions
socket until its parsed durable Schedule response. With a private SQLite
`FULL` store and an injected Schedule clock, the first mutation after both
new host services became ready took 135.160 ms; the next 20 had p50
88.388 ms, p95 143.321 ms and maximum 143.420 ms. Process/store startup,
provider calls and audio are excluded. These are host results, with no lamp
or provider involved, and are not a filesystem latency guarantee.

On lamp-4ace the volume's functions have been called by the real
conversation model ([tools.md](tools.md)). The step sizes as a person
hears them, and what a volume change does to the voice's hearing of
someone who speaks over it, are unmeasured. The light's rules were
rewritten on 2026-10-14 (a colour by name and five levels); what was run
of them on a lamp is in [ring-choreography.md](ring-choreography.md).
