# The bus

`lamp-bus` (`crates/bus`) is how the lamp's processes tell each other what
they sense and what they make of it. A sensor publishes its stream once
(`mic2/audio`, `camera/image`, `body/joint_states`), any number of consumers
subscribe, and a consumer publishes what it derives (`humans/faces/tracked`,
`voice/heard`) the same way. It is decision 15 and step 0 of
[brain-build.md](brain-build.md).

There is no broker: the lamp is several processes with different owners, one
failing must take no other down, and a process in the middle is one they
would all need. A topic is a directory, a subscriber a datagram socket in it,
and a publisher sends each message straight to every socket it finds there.

## Layout

Under a root, `/run/lampos-bus` unless `LAMP_BUS_ROOT` or the caller names
another (an absolute path, on a filesystem with hard links):

    <root>/<topic>/subs/<subscriber>.<pid>.<count>.sock   one for each subscriber
    <root>/<topic>/pubs/<publisher>.<pid>.<boot>          one for each publisher that is alive
    <root>/<topic>/declaration                            what the topic carries, if a publisher said
    <root>/<topic>/latest                                 the newest message, kind `latest` only

Directories are made when first needed, mode 0770 unless the `Bus` is told
another; sockets and files are 0660. A topic's name follows ROS's rule
and only it: tokens of `[a-z][a-z0-9_]{0,31}` joined by `/`, each a directory
under the one before, grouped by the part the data comes from and named for
the data (`mic2/audio`, `body/joint_states`, `room/state`). It is 96 bytes at
most, and no token is `subs`, `latest`, `pubs` or `declaration`, which a
topic's own directory uses. A subscriber's or publisher's name is one such
token (`body`, `lamp_bus_cli`). A `.` between tokens and a `-` inside one,
which the first version of the crate took, are refused. So is anything else,
before the filesystem is touched, and so is a socket path over 103 bytes, the
less of what macOS and Linux hold.
A subscriber binds as `<...>.new` and links that to its `.sock` name, the
only names publishers look at: a socket's file is there a moment before the
socket can be sent to, and on the Mac 61 of 400 bound directly were removed
as dead by a publisher that looked with every message.

A publisher's entry in `pubs/` is an empty file whose time says when the
publisher last showed that it was alive. It is there from the publisher's
first message, or from the start if the publisher declared the topic. The
publisher gives it a new time at most once a second while it publishes, and
when it calls `Publisher::alive()` with nothing to publish, and removes it
when it ends. An entry not refreshed for a lease (3 s unless the `Bus` is
told another, the same for every process on a root) is a dead publisher's:
whoever lists the publishers removes it. A publisher that was only silent
puts its entry back with its next message.

### Publisher cadence in local fixtures

`Bus::publisher_with_cadence_clock` and
`Bus::publisher_declared_with_cadence_clock` accept an explicit pure
`Fn() -> Instant + Send + Sync + 'static` for a local fixture's publisher.
The caller advances that clock; reading it must never advance time. It
controls only the existing subscriber-rescan and presence-refresh cadence:
200 ms and 1 s with the default bus settings. Undeclared construction does
not read it. Declared construction reads it once at the original first
presence refresh, after the real declaration write; each valid delivery
and each `alive()` call takes its own original sample.

The ordinary constructors still use `Instant::now`. These explicit
constructors leave header UTC, caller-supplied `at_us`, real filesystem
mtime leases, sockets, delivery results and error suppression unchanged.
They do not advance a subscriber or a worker, or establish that its work
completed. No existing test is migrated by this constructor seam.

## A message

One datagram: a line of JSON, a line end, then the payload's bytes untouched.

    {"topic":"body/joint_states","kind":"latest","enc":"json","type":"lamp_msgs/JointState","source":"body","boot":"9f3a6c1e04b7d25a","seq":412,"at_us":98123456,"at_unix_ms":1791878400123,"expires_ms":500}
    {"name":["yaw"],"position":[0.12]}

`enc` says how to read the payload: `json`, `none` (there is none), or
`json+pcm` for sound (`lamp_msgs::Audio::ENCODING`): a JSON `Audio` header
line, then the samples. `type` (optional) says what
the payload is, by the name its publisher declared for the topic; a publisher
that declared nothing leaves it out, and so does a recording made before
there were types. `source` is the publisher's name, `boot` sixteen hex
characters it picked when it was made, `seq` its count on this topic from 1,
`at_us` the caller's monotonic clock in microseconds, `at_unix_ms` the wall
clock, `expires_ms` (optional) how long after `at_unix_ms` the value means
anything. The `kind` is one of two:

- `latest`: a value that replaces the one before it (where the joints are).
  The publisher sends it, then puts the same bytes in `latest`, written
  beside and renamed over, so a subscriber that starts late reads one whole
  current value at once (`Bus::latest`).
- `event`: something that happened and is over (a piece of sound, a word
  heard). Nothing is kept: whoever was not listening does not get it.

A whole message is at most 60,000 bytes; a larger one is refused to the
publisher, never cut and never sent. Sound goes as byte payloads in small
pieces (20 ms of 16 kHz mono is 640 bytes). A camera frame stays off the bus:
its topic publishes a small message naming a file the publisher keeps.

## Declaring a topic

ROS checks, when a subscriber connects, that both ends mean the same
message. Here nothing connects, so the agreement is a file. A publisher made
with a declaration (`Bus::publisher_declared`) writes it to
`<topic>/declaration` before it is returned: one line of JSON, written
beside and put in place whole.

    {"type":"lamp_msgs/JointState","version":1,"kind":"latest","period_ms":50,"lifespan_ms":500}

| Field | What it says | What the bus does with it |
|---|---|---|
| `type` | The message's name: a package of topic-like tokens, `/`, a name that begins with a capital | Put in every header. A second publisher or a subscriber that names another is refused |
| `version` | Which version of the type the publisher writes | Kept and shown. Not checked: a newer version adds fields an older reader ignores |
| `kind` | `latest` or `event` | A second publisher that says the other is refused |
| `period_ms` (optional) | A message comes at least this often | A subscriber calls the topic late after twice as long with nothing |
| `lifespan_ms` (optional) | How long a message means anything | Put in every header as `expires_ms` |

- A publisher whose declaration names another type or kind than the one
  there is not made. The error names both and the file. With the same type
  and kind the file is replaced: the version, the period and the lifespan are
  the newest publisher's to say.
- A topic's first declaration is put there by a link, which fails where a
  rename would replace. Of two publishers that start at once and disagree,
  one is told.
- The declaration is the topic's, not its writer's. It stays when the
  publisher ends, until the lamp restarts (the root is in memory) or somebody
  removes the file. Changing a topic's type means removing it.
- A subscriber that names the type it expects (`Bus::subscribe_typed`) is
  refused, with nothing created, if the topic is declared as another. If the
  topic is not declared yet it is subscribed: a subscriber may start before
  its publisher. In both cases the type in each message's header is checked,
  and a message of another type is passed over and counted. A message that
  names no type is taken.
- `Bus::publisher` and `Bus::subscribe`, which say nothing about types,
  work on any topic as they did.

The types themselves are not the bus's business: it compares names. Those
of the first topics are in `lamp-msgs` ([msgs.md](msgs.md)).

## What is promised, and what is not

A publisher never waits and never fails because of a subscriber. Each send is
non-blocking and ends one of three ways: taken; dropped and counted, because
that subscriber's queue is full; or refused because nobody holds the socket,
whose file is then removed. `publish` answers `{delivered, dropped, removed}`
and keeps totals, for each subscriber too. It is an error only for a message
too large or badly labelled, and once, not with every message, for a root it
cannot use or more than 64 subscribers. `subs/` is read at most every 200 ms
and when a subscriber is found dead; between reads, an event with nobody
subscribed costs no system call but the publisher's own entry, once a second.
A publisher that declares its topic is the one case that fails at the start:
it has to write the declaration, so it needs a root it can use.

A subscriber is not promised delivery. It is promised order within one
publisher, none between two, and that loss shows: each message has `gap`, how
many of its publisher's were missed since the last one received, and
`publisher_restarted` when that publisher is a new instance whose count began
again. A new subscriber waits up to 200 ms to be found, and what was sent
before is not a gap. On a `latest` topic, `recv` with nothing waiting returns
the file's value if the subscriber has not had it, so the newest value
arrives though its datagram was dropped.

A program that waits on several topics at once can `poll` their sockets: a
`Subscriber` is `AsFd`, readable when a datagram waits. One already read
ahead (to find out whether anything was waiting) is no longer in the socket,
so it asks `holds()` first, and it still calls `recv` at least every 200 ms,
since the topic's file and its publishers are looked at only there (the
voice's controls pump with `LAMP_CONTROLS_WAIT=on`, [controls.md](controls.md)).

A subscriber also learns what no message can say:

| What | When | How it is told |
|---|---|---|
| The source has gone | No publisher has an entry refreshed within the lease, and no message has come for a lease. At once when a publisher of a quiet topic ends; a lease after subscribing to a topic nobody publishes | `Notice::SourceGone`, once |
| The source is back | After that: a publisher's entry is there, or a message came | `Notice::SourceBack`, once |
| The topic is late | It declares a period and nothing has come for more than twice as long | `Notice::Late`, once |
| It is on time again | After that: a message came | `Notice::OnTime`, once |
| A message had expired | It arrived more than 50 ms after `at_unix_ms + expires_ms`, by the subscriber's wall clock. It is not handed over | Counted: `expired()` |
| A message was of another type | The subscriber named a type and the header names another. It is not handed over | Counted: `wrong_type()` |

`recv` returns messages and nothing else, as it did. The notices come from
`Subscriber::notices()`, asked between two `recv`s, and
`Subscriber::is_late()` and `source_gone()` say how things stand now. They
are apart from `recv` so that no caller written before them changes, and
because a notice is about the topic and belongs to no message. They are
called notices because `event` is already a kind of topic. Three limits
follow from asking and not being called:

- A notice is as late as the caller's own loop: the state is judged when the
  caller asks and when a message is taken, not by a thread of the bus's.
- A subscriber that has not read for a while is not told that its topic went
  quiet. Silence is said only when nothing is waiting in the socket, and a
  silence found afterwards is measured between the times two messages were
  sent, not the times they were read.
- A publisher that is alive and silent has to say so. One that neither
  publishes nor calls `alive()` for a lease is gone to its subscribers,
  though its process runs. A publisher in another language that keeps no
  entry is there for as long as its messages are less than a lease apart.

A topic's kept value is not hidden when it has expired. `Bus::latest` and
`Subscriber::latest` return it with `expired` set: it is the last thing
known, and whether an old value is worth anything is the caller's to say.
What a passed-over message would have said of its publisher, a gap or a new
instance, is said with the next message handed over.

`Subscriber::statistics()` gives what came since the subscriber was made:

| Field | What it counts |
|---|---|
| `received` | Messages `recv` handed over |
| `datagrams` | Of `received`, those that came as datagrams, not from the topic's `latest` file: none until a publisher has found this subscriber |
| `gaps` | Messages known to have been sent and not received |
| `expired`, `wrong_type` | Messages passed over for either reason |
| `mean_interval`, `largest_interval` | The time between taking one message and the next, by the subscriber's monotonic clock. The stream's rhythm for a subscriber that reads as fast as messages come, the reader's for one that does not |
| `mean_age` | Wall clock at arrival less `at_unix_ms`: how old a message is when it gets here |

Liveness and lifespan are judged by the wall clock, the only one two
processes share. A clock stepped by more than the lease makes every publisher
look dead until its next refresh, at most a second later, and a topic quiet
for a lease is then reported gone and back.

The queue is where this differs from ROS on purpose for now: each
subscriber's socket is its own queue and the kernel cannot evict from it, so
a full one loses the newest message, where ROS drops the oldest.

## The tool

    lamp-bus topics                                 # body/joint_states subscribers=2 latest=yes type=lamp_msgs/JointState
    lamp-bus info body/joint_states                 # declaration, publishers, subscribers, the kept value
    lamp-bus hz mic2/audio --seconds 5              # rate, gaps between messages, age, what was missed
    lamp-bus tap voice/heard --count 5              # header, then the payload if JSON, else <N bytes>
    lamp-bus logs --part body --level warn           # validated operational logs, warn and above
    lamp-bus keep-logs /var/lib/lampos/logs          # the same, kept on disk in bounded files (logging.md)
    lamp-bus latest body/joint_states               # status 1 if the topic keeps none
    lamp-bus pub room/state latest '"quiet"'        # as lamp_bus_cli, at_us 0 unless --at-us
    lamp-bus pub room/state latest '"quiet"' --type lamp_msgs/RoomState
    lamp-bus record /var/tmp/rec mic2/audio body/joint_states --seconds 60
    lamp-bus replay /var/tmp/rec/body_joint_states.jsonl --speed 2

Each takes `--root PATH`; bad input prints the usage and exits 2.

`logs` follows the optional `logs` event topic. It accepts `--part`, a minimum
`--level` (default `info`), `--count` and `--seconds`; only matching validated
records count. The type, kind, encoding, source and bounded payload must agree.
Text is escaped for a terminal, and the output shows the producer's admission
attempt separately from the bus sequence and transport gap. It reads live
events, with no retained history. See [shared logging](logging.md) for the
explicitly constructed helper, overload and shutdown contracts, privacy rules,
and the remaining persistence/image work. Existing processes do not start
logging merely because this command exists.

`info` prints one fact to a line, and exits 1 for a topic nobody has
published, subscribed to or declared. Its shape, with numbers made up:

    topic body/joint_states
    declaration type=lamp_msgs/JointState version=1 kind=latest period_ms=50 lifespan_ms=500
    publisher body pid=412 boot=9f3a6c1e04b7d25a alive_ms=312
    subscriber follow.420.0 listening
    latest source=body seq=412 age_ms=31 expired=no

`alive_ms` is how long ago the publisher refreshed its entry. A subscriber
whose process died is `stale`. What is missing is `declaration none`,
`publishers none`, `subscribers none` or `latest none`.

`hz` listens for `--seconds` (5) and prints one line, and exits 1 if no
message came. The gaps are between one message's arrival and the next,
`missed` is the sum of the messages' `gap`, and a value the topic already
kept is not counted. A rate takes two messages: before that it is `-`. This
is what it printed on the Mac for five messages sent by hand a tenth of a
second apart:

    body/joint_states messages=5 seconds=1.00 rate_hz=8.69 gap_mean_ms=115.1 gap_max_ms=115.9 age_mean_ms=0.4 missed=0 expired=0

`tap` and `hz` say on standard error when the topic's source goes or it is
late. `latest` prints an expired value and says on standard error that it
has expired. `pub --type NAME` publishes as a publisher that declares the
topic: it joins a topic already declared as that type and kind without
changing what was promised of it, and exits 1 where the topic is declared
otherwise.

`record` appends to `DIR/<topic with / as _>.jsonl` until killed or
`--seconds` have passed, and stops a file at `--max-mb` (64 MiB) saying so.
A message is a line `{"header":{...},"payload_b64":"..."}`. A topic's
declaration is a line `{"topic":"...","declaration":{...}}`: the first line
if the topic was declared when the recording began, else the line before the
first message of the publisher that declared it. `record` cannot catch a
signal: SIGTERM ends it with whole lines on disk and its sockets left for the
next publish to remove. `replay` publishes the messages again as `replay`,
spaced by `at_us` over `--speed`, and declares each topic again where the
recording does: it fails at that line on a bus where the topic is declared
as something else. A recording without declarations replays as before. One
of a topic named the old way, with a `.`, is refused by the name rule.

## From another language

```python
import json, os, socket
name = f"/run/lampos-bus/servos/subs/py.{os.getpid()}.0"
os.makedirs(os.path.dirname(name), mode=0o770, exist_ok=True)
sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
sock.bind(name + ".new")                    # a name publishers pass over
os.chmod(name + ".new", 0o660)
os.rename(name + ".new", name + ".sock")    # its own, now that it can be sent to
try:
    while True:
        head, _, payload = sock.recv(60001).partition(b"\n")
        header = json.loads(head)
        print(header["source"], header["seq"], json.loads(payload) if header["enc"] == "json" else len(payload))
finally:
    os.unlink(name + ".sock")
```

A publisher in another language sends that datagram to every `*.sock` in
`subs/`, without waiting. To be listed, and to be there while it is silent,
it keeps an empty file `pubs/<name>.<pid>.<16 hex>` and gives it a new time
every second; to be held to a type it puts `type` in each header.

## What is taken from ROS

The owner's direction on 2026-10-13: do not use ROS 2 itself, which is
built for robots without a brain of this kind, but study how ROS designed
publish and subscribe over ten years and bring the design over. The study
is in three documents kept outside the repository (ROS's publish and
subscribe design with its reasons, its conventions, and what else it has
worked out), each claim with its source. This table is what is taken from
it. **It is a design decision record: except where the last column says
so, the crate does not do it yet.**

| What ROS does, and its reason | What the lamp's bus does about it | In the crate |
|---|---|---|
| Names are `/`-separated lower-case tokens with underscores, grouped by the part the data comes from and named for the data, never for who uses it | The same rule, and only it: `mic2/audio`, `camera/image`, `body/joint_states`. The `.` separator and `-` in a token are refused, in a publisher's or subscriber's name too | Yes |
| One declared message type per topic, checked when a subscriber connects: both ends "must agree on the layout" | Each topic carries a declaration written by its publisher (type name, kind, how often, how long a value lasts). A subscriber names the type it expects and is refused another. The types are Rust structures in one shared crate, sent as JSON | Yes. The bus compares names and does not check a payload against its type. The types of the first topics are in `lamp-msgs` ([msgs.md](msgs.md)), and the parts publish with them: the voice's `voice/*` below, the body's, the ears', Controls' and the others' |
| The registry only introduces; data goes straight from publisher to subscriber: "no central point of failure" | The filesystem is the registry and each message goes straight to each subscriber's socket | Yes |
| A bounded queue per subscriber, so that one slow subscriber cannot hold the publisher or the others | Each subscriber's socket is its own queue; a full one loses that subscriber's message and is counted. ROS drops the oldest, this drops the newest; to be revisited for sound | Yes, with that difference, which is unchanged |
| Latched topics: "new subscribers always get the latest state" | The `latest` kind: the newest value is kept and read by a late joiner. It also outlives its publisher, which ROS's does not | Yes |
| Liveliness and deadline: a publisher must show it is alive, a topic may promise a period, and missing either raises an event | Each publisher keeps a small entry it refreshes; the subscriber's library reports "the source has gone" and "this is late". This is how a part says whether it is ready | Yes: `pubs/`, a lease of 3 s, `Notice`. A subscriber is told when it asks, not called. The publishers call `alive()` between messages: the voice (`bus_out.rs`), the body, the ears, Controls, Expression, the eyes and the others |
| Lifespan: a message older than its limit is dropped, not delivered | The header already carries how long a value lasts; the subscriber's library is to drop what has expired | Yes: `recv` passes an expired message over and counts it. A kept value is returned marked `expired`, not hidden, which ROS does not do |
| The graph: any tool can list topics, their types, who publishes and who subscribes | `lamp-bus info TOPIC` and `lamp-bus hz TOPIC` beside the tools there are | Yes, and `topics` shows each type. The same from a program: `Bus::declaration`, `publishers`, `subscribers`. Nothing says when the graph changes: a tool asks again |
| Three patterns, not one: topics for streams, services for ask-and-answer, actions for long work with progress, one result and an acknowledged cancel | Topics on the bus. A service stays a direct ask-and-answer on its own socket (the executive's is one, [tools.md](tools.md)), and publishes each call as an event so that it can be watched. A task handed to an agent takes the shape of an action: an id made by the asker, accepted or refused, progress, one result, cancel acknowledged, its status latched | Topics only |
| Recordings store each message's type and when it was sent and received | `record` and `replay` exist; the type name is to be stored with each topic | Yes: a recording holds each topic's declaration and each message's type and send time, and `replay` declares the topic again. The time a message was received is not stored |
| People (ROS4HRI, REP-155): a permanent id for a person, passing ids for a face or a voice present now, lists of who is here, and engagement as five levels | The same shape for what the eyes and the voice know of people (`humans/...`). Engagement is what "is this said to me?" needs ([attention.md](attention.md)) | No |
| Several requesters, one actuator: priorities and time limits at the actuator's owner | For the ring, the servos and the speaker: the owner arbitrates, and a request that is not renewed lapses | The ring's light, with `LAMP_LIGHT_REQUESTS=on` (#940); not the servos or the speaker yet |

Not taken: negotiation of delivery settings between each pair of ends,
a binary encoding with generated code, renaming topics at start-up,
security beyond file ownership and modes, and a replaceable clock. One
machine and one team do not need them yet.

### The first topics

Named by the rule above. Rates are those the parts already run at. The type
of each, and its kind and period as a declaration, are in
[msgs.md](msgs.md): a stream is declared as `event`, a latched topic as
`latest`.

| Topic | What it carries | Kind | Published by | How often |
|---|---|---|---|---|
| `mic1/audio`, `mic2/audio` | Sound, 20 ms at a time (16 kHz, one channel), as `json+pcm`: what the voice hears after its echo canceller | stream | The voice, only with `LAMP_BUS=on` and `LAMP_BUS_AUDIO=on` (`bus_out.rs`, `MicOut`); the capture worker itself never touches the bus | 50 a second |
| `camera/image` | Where the newest picture is, and when it was taken. The picture itself stays in a file | latched | Eyes | 2 to 4 a second |
| `body/joint_states` | Each joint: where it is, how fast, how loaded (ROS's `JointState`) | latched | Body | 10 to 20 a second |
| `humans/faces/tracked` | The faces in view: a passing id, where, and whether it is turned to the lamp | latched | Eyes | With each look |
| `humans/familiar` | How familiar the remembered person at the lamp is, as a level, with no name | latched | Eyes, with `--visits on` (#1670) | At each recognition, every 2 s |
| `humans/seems` | How the near face seems, as a kind and a confidence, for measuring; Mood reads its kind with `LAMP_MOOD_SEEMS=on` (#1671, mood.md); the second voice tells a known person's turn how they seem with `LAMP_SECOND_SEEMS=on` (second-voice.md) | latched | Eyes, with `--seems` or `--seems-blendshapes` (#1671) | At most every 2 s while a face is near |
| `voice/heard`, `voice/said` | What was heard and what was said, as text | stream | Voice | With each thing said |
| `voice/state` | Resting, listening, thinking or speaking | latched | Voice | When it changes, and once a second |
| `voice/turn` | A turn's verdict: its request, and whether it was `answered` or its answer `held_back` as not said to the lamp, as the voice's journal says it (#1155, [policy.md](policy.md#the-quiet-counts-talk-to-the-lamp-1155)) | stream | Voice, on the bus as `room/state` is | Once for each turn the rule judged, as its answer is let out or held back; again, `held_back`, when the model declines the line (`not_for_me`) |
| `voice/tone` | How a line sounded against the person's own way of speaking: its request, its arousal (calm -1 to lively +1, from its prosody), how sure, and its valence (down -1 to glad +1, from its words, when a word says it). Numbers only ([voice-tone.md](voice-tone.md)) | stream | Voice, with `LAMP_VOICE_TONE=on` | Once a line, when its words come |
| `memory/forgot` | Memory has forgotten everything under a person, asked by the app or by voice: their id only, or none for every person ([memory.md](memory.md#forgets-told-on-the-bus-memoryforgot)) | event | Memory, with `--bus-root` | Once a forget |
| `voice/link` | Whether the provider can be reached | latched | Voice, with `LAMP_BUS=on` and `LAMP_VOICE_LINK=on` (#830): unavailable once down for 5 s, reason `transport` | Once a second |
| `lamp/online` | Whether the lamp can reach the internet, worked out from the parts' links | latched | Health, with `--online on` (#830) | Once a second |
| `room/state` | Quiet or busy | latched | Voice, later the situation | When it changes, and once a second |
| `room/presence` | Whether anyone is at the lamp: someone (a face or a voice within 20 s), or nobody for how long (#1677, [presence.md](presence.md)) | latched | Eyes, with `LAMP_PRESENCE=on` | Once a second while they look, and when someone comes or goes; each good for 3 s |
| `room/activity` | What the face at the desk shows: at the desk, leaning in, head down, or away, with how long and the sitting's length (#1678, [eyes.md](eyes.md)) | latched | Eyes, with `LAMP_ACTIVITY=on` | Once a second while they look, and on a change; each good for 3 s |
| `room/stood_up` | Someone at the desk stood up and left, and the sitting's length (#1678) | event | Eyes, with `LAMP_ACTIVITY=on` | Once each time |
| `eyes/camera_closed` | The camera is shut for a close the voice asked a receipt for, and whether the device's last stop released it (#1802, [onboarding.md](onboarding.md)) | latched | Eyes | Once per asked close, after the stop |
| `voice/privacy` | Whether the microphones may capture now: a lease from the voice's privacy worker with its controller, microphone generation, whether the switch is closed (or unknown), and when it expires, at most 50 ms on (`MicrophonePrivacy`, [msgs.md](msgs.md)). Read by `lamp-ears`, which captures only under a fresh lease, and by Expression, `lamp-skills`, the add-ons and `lamp-cloud` | latched | Voice, only with `LAMP_PRIVACY_CONTROLS=on` or `no-slide` (`on` in the product; `privacy_bus.rs`) | On each change, and again once 10 ms have passed: about 56 a second through a turn on lamp-52e6 (stable 8670, 30 Oct), the busiest topic after the sound |
| `account/state` | Whether this lamp's login is no longer accepted, and for how long (`AccountState`, [msgs.md](msgs.md)) | latched | `lamp-cloud`, only with `LAMP_SIGNED_OUT=on` | When it changes, and every 5 s |
| `light/command`, `light/state` | The light asked for, and the light shown | latched | The executive and `lamp-cloud` (the app's Light row); the ring's owner | When it changes, and once a second |
| `light/test`, `light/drawn` | A self-test's colour, shown for 2 s at most; the light the ring actually drew | latched | The self-test; the ring's owner | Asked once a colour; drawn when it changes, and once a second |
| `light/request` | A light asked for that lapses, with its asker, priority and time, how it moves (`pattern`: breathe, candle or one pulse, #940), and how slowly it is faded into (`fade_ms`, #1677) | event | Skills and other parts (`lamp-cloud`'s preview, [cloud.md](cloud.md)) | Asked, and asked again to keep it, at most 30 s ahead |
| `light/reached` | A requested light the ring has shown at rest for 350 ms, with its asker (#1146) | event | The ring's owner, with `LAMP_LIGHT_REQUESTS=on` | When a request's light has been shown 350 ms, and again after each renewal while it is; never inside a turn |
| `light/mark` | A computer seeing or hearing through the lamp: its asker, `camera` or `microphone`, and a `blink` for a snapshot ([control.md](control.md), "The camera") | event | `lamp-control` | Each second while a stream runs, at most 3 s ahead; once for a snapshot |
| `light/outranked` | A request the ring holds but doesn't show: its asker, and `by`, the asker shown over it or `person` (#940) | event | The ring's owner, with `LAMP_LIGHT_REQUESTS=on` | When it is taken hidden, loses its place, or what is over it changes, and again after each renewal while it stays hidden |
| `diagnostics` | Each part's health: ok, warning, error or stale | stream | Every part | Once a second |
| `policy/decisions` | What the policy decided, with the probability and each judgement behind it, and whether it is only watching ([policy.md](policy.md)) | stream | The policy | With each thing heard |

## On the lamp

On lamp-4ace since 2026-10-13: the root is `/run/lampos-bus`, made at
start by `/etc/tmpfiles.d/lampos-bus.conf` as `root:orangepi` with mode
2770. The lamp's programs run as two users, root (the voice) and
orangepi (the rest), and the set-group-ID bit on the root is what lets
both use it: everything made below takes the group. The tool is
`/usr/local/bin/lamp-bus`. A unit with `ProtectSystem=strict` needs
`ReadWritePaths=/run/lampos-bus` to publish or subscribe.

The first time two users used one root, it failed: a topic made by root
could not be entered by the other user, and a kept value written by root
could not be read. A new directory's mode was set outright, which took
the bit off. A directory now keeps its parent's bit. After that, on the
lamp: an event in each direction between the two users, a kept value
written by each and read by the other, one message of 58,955 bytes, and
`hz` over twenty messages a tenth of a second apart (gaps of 103.6 ms on
average and 105.1 at most, 0.2 ms old on arrival, none missed).

The voice publishes `voice/state`, `voice/heard` and `voice/said` when
its unit has `LAMP_BUS=on` (`crates/voice/src/bus_out.rs`), each declared
with its type from `lamp-msgs`. The state comes when it changes and once a
second (on the lamp: one a second, gaps of 1,000.9 ms on average, 0.2 ms
old on arrival); `voice/said` carries the lamp's words a sentence at a time, with the
number of the request it answered; `voice/heard` carries what was heard, on
the second route one whole, finished message per line, with the number of
the turn whose input began last before it, and between things
said the voice says once a second that it is alive on them. Before it did,
their subscribers were told "no publisher is alive" before and after
every sentence.

The four Voice bus fixtures for state cadence, heard/said routing, function
call filtering and room cadence subscribe before their first publication.
Subscription has bound and named the socket when it returns; first delivery
scans synchronously, so these fixtures need no startup delay for the 200 ms
rescan interval. Their original timestamps, exact event assertions, 30/100 ms
drains and 500 ms receive bound remain. The separate silence/lease scenario
still uses its real waits.

The eyes publish `humans/faces/tracked` the same way
(`crates/eyes/src/bus_out.rs`): with each look while the camera is open,
the faces in view, each with a passing id, its box as fractions of the
picture, how sure the detector was, and, for a face large enough to
measure, how far it is turned and whether that counts as turned to the
lamp. A list lasts two seconds. On the lamp, at night with nobody in the
office: an empty list 1.66 times a second. No list with a face in it has
been published yet, and the judgement of "turned to the lamp" uses V1's
numbers, which have not been checked on this lamp.

Since 2026-10-14 the lamp's other topics, each described with the part
that owns it:

| Topic | Published by | Read by |
|---|---|---|
| `room/state` | the voice's provider worker, on each change and once a second | the policy, and Expression |
| `room/presence` | the eyes, with `LAMP_PRESENCE=on`, from their looks and `voice/heard` ([presence.md](presence.md)) | `lamp-skills` (Light out when you leave; Who was here's record, #1685); Messages (#1673) |
| `room/activity`, `room/stood_up` | the eyes, with `LAMP_ACTIVITY=on`, from their looks ([eyes.md](eyes.md)) | `lamp-skills` (Stretch break waits for a look up); "watches you leave" (#1669) |
| `voice/timing`, `voice/activity`, `voice/playout` | the voice, with `LAMP_EXPRESSION_OUT=on` ([expression.md](expression.md), "What the voice says, as built") | Expression |
| `expression/rest_request` | `lamp-skills`, with `LAMP_SKILL_ROUTINES=on` and an enabled routine | Expression, with `LAMP_REST_REQUESTS=on` |
| `expression/rest_state` | `lamp-expression`, with `LAMP_REST_REQUESTS=on` | Skills' routine manager |
| `expression/sleep_claim` | `lamp-skills`, with `LAMP_SKILL_ROUTINES=on` and the `night-sleep` folder, every 60 s | Expression, with `LAMP_REST_REQUESTS=on`: fresh under 180 s, the night is the skill's (#940) |
| `output/caps` | `lamp-actions`, with `LAMP_OUTPUT_CAPS=on` | Voice's ring worker, with `LAMP_OUTPUT_CAPS=on` |
| `policy/decisions` | the policy ([policy.md](policy.md)) | the voice (the second route's policy and its holds, task news), `lamp-skills`, the add-ons |
| `body/joint_states` | the body ([body.md](body.md), "On the bus") | the voice (`body_motion.rs`), Expression, the eyes, `lamp-peek`; recorded to measure how the lamp moves |
| `light/command` | the actions program ([actions.md](actions.md)); `lamp-cloud`, the app's Light row ([cloud.md](cloud.md)) | the voice's ring worker |
| `light/state` | the voice's ring worker ([ring-choreography.md](ring-choreography.md), "The lamp's own light") | the actions program; `lamp-cloud` |
| `name/heard`, `name/rising` | `lamp-name` (the lamp's name heard in `mic*/audio`: its final word, and the first sign while it is still being said, #782) | the voice (`name/rising` for the call's wait, `name/heard` in `body_ask.rs`), the body, Expression, `lamp-mood`, the policy, `lamp-control`, and skills whose `skill.toml` names it (`night-sleep`) |
| `stop/heard` | `lamp-name`, only with `--stop-least` (its `stop.onnx`, #828, [stop-word.md](stop-word.md)). The product's `lamp-name.service` does not pass it and no models release carries `stop.onnx`, so a product lamp's bus has no `stop/` topic: a "stop" there is heard as any talk over the reply is ([duplex-listening.md](duplex-listening.md)) | the voice, only with `LAMP_STOP_HEARD=on` (`bus_out.rs`), not set in the product: a "stop" while the reply sounds yields it |
| `voice/remarked` | the voice, one for each remark about the lamp it holds in a busy room (`LAMP_REMARK_ABOUT_IT`) | Expression, with `LAMP_REMARK_GLANCE=on`: one glance |
| `light/test` | the self-test (#812) | the voice's ring worker |
| `light/drawn` | the voice's ring worker, from its own write receipts ([ring-choreography.md](ring-choreography.md), "The lamp's own light") | the self-test |
| `light/request` | `lamp-skills`, a skill's light, with `LAMP_LIGHT_REQUESTS=on` ([skills.md](skills.md)); `lamp-cloud`, the app's preview ([cloud.md](cloud.md)) | the voice's ring worker, with `LAMP_LIGHT_REQUESTS=on` |
| `light/reached` | the voice's ring worker, with `LAMP_LIGHT_REQUESTS=on` ([ring-choreography.md](ring-choreography.md), "The lamp's own light") | nothing yet; `lamp-skills`, to wait for a skill's light before its look (#1146) |
| `light/mark` | `lamp-control`, while a computer streams the camera, and once for each snapshot ([control.md](control.md), "The camera") | the voice's ring worker, whenever it has the bus: one small steady red dot over every face, a turn's included |
| `light/outranked` | the voice's ring worker, with `LAMP_LIGHT_REQUESTS=on` ([ring-choreography.md](ring-choreography.md), "The lamp's own light") | `lamp-skills`, a run's own subscription: the run says what was over its light in its journal and at its give-back ([skills.md](skills.md)) |
| `diagnostics` | every part that says how it is: the voice, the eyes, the body, the policy, and the board through the health program | the health program |
| `lamp/health`, `board/state` | the health program ([health.md](health.md)) | `lamp-health`, the command; `lamp-skills` (Not feeling well reads `lamp/health`'s `level` and `quiet_ms`, [skills.md](skills.md)) |

The policy also reads `voice/heard`, `voice/said`, `voice/state` and
`humans/faces/tracked`.

The voice says how it is about once a second under the name `voice`: well
while it can reach the speech service or has lost it for under five
seconds (it loses it for a second or two at every start and every
renewal of its session), and otherwise `it cannot reach the speech
service, and keeps trying`, with for how long.

## Optional Expression topics (Third, 2026-10-14)

The new `lamp-expression` process subscribes to existing context and new
`voice/timing`, `voice/activity`, `voice/playout` and
`attention/concentration` messages. It publishes `expression/state` and
its `diagnostics`, and only with `--enable` may publish leased
`expression/body` and `expression/light` requests. It does not publish on
the existing Actions-owned `light/command`. Their types, periods and
lifespans are in [msgs.md](msgs.md), with the contract in
[expression.md](expression.md). The voice publishes its three since
2026-10-14 when switched on. First has added Body's optional consumer;
the light and the resumable Voice contract still need First's integration.

The task 18 catalogue option `resumable_voice: true` selects the additive
`voice/progress` latest topic and `voice/sound` event topic in place of the
original timing/playout pair. The process subscribes to only its selected
pair; replay recognizes both but rejects a protocol mismatch. Progress carries
the complete state/readiness heartbeat, a positive audio generation and a
held flag. Sound carries that generation on each committed interval. A hold
discards earlier queued sound; a later resume cannot revive the old generation.
This is an optional producer/consumer contract, not a changed lamp publisher.
The existing bus transport and its per-topic ordering are unchanged.

The new payload timestamps explicitly use the shared host monotonic clock
and boot identity. This does not change the meaning of existing bus header
`at_us`: that remains a publisher-private timestamp. Expression's replay
importer warns when it estimates a shared timeline from legacy wall stamps.

`expression_topics()` also declares `expression/rest_request` (event,
one-second period, 500 ms lifespan) and `expression/rest_state` (latest,
200 ms period, 500 ms lifespan), and `expression/sleep_claim` (latest,
60 s period, 180 s lifespan, #940). Skills' request is a clock/session/episode
bound rest, wake or cancel intent with at most a three-second lease.
Expression publishes the correlated planning receipt; completed does not
prove that an actuator moved. The feature switches default off. Both
messages accept additive fields, including nested requests and stamps, but
refuse unknown operation or phase values. The owners still enforce freshness,
privacy and cancellation; declaring a topic grants no actuator authority.

## Optional output restrictions

`output_caps_topics()` declares `output/caps`, an Actions-owned latest
`OutputCaps` snapshot every second with a three-second admission lifespan.
It contains a persisted revision and up to eight source-owned light/volume
ceilings. With `LAMP_OUTPUT_CAPS=on`, Actions publishes the full set, including
an empty set after release, and Voice's ring worker consumes it for ordinary
light output. Actions applies the volume ceiling itself. A lost heartbeat
does not release an admitted restriction; unknown wall time retains it.
Ceilings combine by minimum and do not grant output authority. Snapshot and
nested entry fields are additive; the separate private Actions socket
commands stay strict. See [msgs.md](msgs.md#optional-output-restrictions)
and [Bedtime and Morning](bedtime-morning.md) for the validation and clock
boundaries. These declarations are recognized by `declaration_of()` and
leave the original seventeen topics unchanged.

## Optional Controls topics (Third, 2026-10-14)

The additive [Controls](controls.md) contract names two topics, separate from
the original catalogue and from Voice's physical privacy worker:

| Topic | Kind | Published by | Intended readers |
|---|---|---|---|
| `controls/gesture` | event, 500 ms lifespan | `lamp-controls` | Voice for Stop/SoftwareMute, body/output owners for Stop, Expression for Pet |
| `controls/state` | latest, 100 ms period, 300 ms lifespan | `lamp-controls` | The same owners for independently retained latest Stop/mute identities; observers for input faults and hold feedback |

The [message types](msgs.md#optional-controls-contracts) carry shared monotonic
clock/boot stamps, a Controls session and increasing event identities. An event
and its retained copy are one request. A state heartbeat does not renew the
request's time, and a slow subscriber can still lose events on this bus. Owners
must deduplicate, reject stale or replaced sessions and retain any accepted
software inhibit themselves. Shadow messages cannot act; no Controls message
is a microphone/camera permit, an applied-mute receipt or proof of silence.
First wires the physical adapter and final owners; these topics have not been
observed on the lamp in this review.

## Optional Schedule topic (Third, 2026-10-14)

| Topic | Kind | Published by | Intended reader |
|---|---|---|---|
| `schedule/due` | latest, 1 s period, 3 s lifespan | `lamp-schedule` | Voice, to discover due work before asking Schedule's private socket |

The additive [`ScheduleDue` message](msgs.md#optional-schedule-contract)
carries a shared monotonic `ClockStamp`, persistent store identity and
revision, enabled/ready status, bounded pending/waiting counts, and at most
one oldest notice. That notice contains only its item/delivery identity,
scheduled Unix time and lateness metadata. Reminder text stays off this
topic. A disabled or unavailable Schedule reports no oldest notice, even
when pending work remains.

Schedule's own persistent queue survives Voice being busy or down. The bus
does not supply that durability: an event can be lost and a latest value
replaces its predecessor. Publishing a value, reading it, or observing a
new heartbeat is not an acknowledgement of speech. The unchanged delivery
token remains offered until an explicit durable acknowledgement, cancel or
snooze is accepted over the private socket.

Before considering speech, the consumer checks the current host boot and
stamp, publisher identity and ordering, enabled/ready status, and expiry,
then retrieves the exact `store_id`/`id`/`delivery` from that socket. This
second check prevents a cancelled or superseded retained offer from being
treated as current work. It acknowledges only confirmed completion. Voice
remains the owner of privacy, turn admission and actual speaker output;
this topic grants none of them. First owns that integration. The existing
bus header clock, delivery statistics, lifespan grace and transport behavior
are unchanged.

## Optional tasks topic (second builder, 2026-10-15)

| Topic | Kind | Published by | Intended reader |
|---|---|---|---|
| `tasks/status` | latest, no period, no lifespan | `lamp-tasks serve --progress` | The policy's progress question (`lamp-policy --progress`); the voice's task news, for a task newly settled (#560, [tasks.md](tasks.md), "Telling") and, with `LAMP_PROGRESS_NEWS=on`, for progress |

The [`TaskStatus` message](msgs.md#the-tasks-contract) says, as typed state,
how each piece of work the lamp handed over is going; the agent's words stay
in the tasks part and are read from its socket only when the lamp will speak
([tasks.md](tasks.md#progress-on-the-bus-serve---progress-732)). One value
holds every task, so a subscriber that starts late has them all. Without
`--progress` the topic is never made.

## Optional skills topic (seventh builder, 2026-10-16)

| Topic | Kind | Published by | Intended reader |
|---|---|---|---|
| `skills/events` | event, no period, no lifespan | `lamp-skills` | People and the lamp's app: what each physical skill did and why ([skills.md](skills.md)) |

The [`SkillEvent` message](msgs.md#the-skills-contract) says one stage of a
run: triggered, asked, answered (with whether it was allowed) and done,
each with why in words. `lamp-skills` also subscribes to the topics its
skills' triggers name; Welcome back's is `humans/faces/tracked`. Without
the service installed the topic is never made.

## Optional add-on topic (blue, 2026-10-21)

| Topic | Kind | Published by | Intended reader |
|---|---|---|---|
| `addon/proposal` | event, no period, no lifespan | `lamp-skills`, with `LAMP_ADDONS=on` | `lamp-addons`, the add-ons' judge ([addons.md](addons.md)) |
| `addon/ask` | event, no period, no lifespan | `lamp-addons`, with `LAMP_ADDONS=on` | `lamp-policy` with `--gate --addons`: an add-on's say to judge, with no words |
| `addon/decision` | event, no period, no lifespan | `lamp-addons`, with `LAMP_ADDONS=on` | People, the journal and the app: each proposal kept or dropped, and why |
| `mood/nudge` | event, no period, no lifespan | `lamp-addons`, with `LAMP_ADDONS=on` | `lamp-mood`: a kept add-on's mood, held inside Mood's own bounds until its end (#2530) |

[`AddonProposal`](msgs.md): what a partner's add-on suggests, never an
order. Without `LAMP_ADDONS=on` the topic is never made.

## Optional arrival topic (second builder, 2026-10-15)

| Topic | Kind | Published by | Intended reader |
|---|---|---|---|
| `voice/arrival` | event, no period, no lifespan | nobody since #2825 (`lamp-voice` with `LAMP_HELLO_ASKS_POLICY=on` before; the voice's own hello was removed) | The policy's hello question (`lamp-policy --hello`) |

The [`VoiceArrival` message](msgs.md#the-arrival-contract) says that a
hello is due and held, with counts and times only. The answer comes back on
`policy/decisions`, naming the arrival. Without the switch the topic is
never made.

## Not shown

Everything in this section was run on one development Mac (x86_64); what
has run on a lamp is in the section above and is all that has.
On 2026-10-13 the crate's 64 tests passed ten times in a row there: 18 of
its parts, 2 of the tool's, 32 of the library through real sockets, 10 of
the built binary run and killed, and the 2 examples in its documentation.
The 32 alone then passed 40 times in a row, and 60 times run six at once.
The Python above was run once against this version, with its root changed
to a temporary directory.

On Linux this version has been type-checked for the lamp's processor
(`cargo check --target aarch64-unknown-linux-gnu`) and not run. The 43 tests
of the first version passed once, on 2026-10-13, as a Linux ARM64 build
under emulation in a container: that run is the first in which a
60,000-byte message was delivered and a bound socket was linked to a second
name on Linux. Everything since has run on the Mac only: the name rule,
declarations (put in place by a hard link), publishers' entries (whose
times the standard library sets), notices, lifespan, statistics, `info` and
`hz`. On a lamp, only what the section above lists: not the notices, a
lease running out, a lifespan, or a subscriber that falls behind.

Not exercised at all:

- A wall clock that is stepped. The rule for an entry whose time is ahead
  of the clock is tested by setting a file's time, not the clock.
- The lease of 3 s. The tests of liveness use leases of 150 to 300 ms so
  that they end quickly; the default was in use only where the tool was run
  against a publisher.
- What an entry costs a publisher (an open, a time set and a close, once a
  second) is read from the code, not measured. Nor is how late a notice is.
- The rates `hz` prints were compared with a publisher that sleeps 10 ms
  between messages, within wide bounds (10 to 101 a second), not against a
  clock.

Socket buffers are the system's defaults, which
the standard library cannot change. On this Mac a subscriber that does not
read held 25 messages of 141 bytes, or 5 pieces of sound, before the next was
dropped, and the system takes no datagram over 2,048 bytes: the test of a
60,000-byte message asserts delivery on Linux and that refusal here. Each subscriber is sent to from a socket of its own
because Linux charges a queued datagram to its sender (`unix(7)`), so one
socket for all would let one stuck subscriber starve the rest: that is read,
not measured, and macOS charges the receiver, so no test here tells the two
apart. How many messages a subscriber that does not read holds on the lamp
is not known.

The Controls topic addition has not been measured on a lamp. This source review
does not establish hardware acquisition, consumer integration or hand-to-silence
latency, and adds no test or benchmark result to those above.

The Schedule topic adds a discovery contract, not a measured delivery path.
No Voice integration, power-loss recovery, clock accuracy or due-to-speech
latency on a lamp is established by this topic declaration.

## CLI test recording observation

The recording tests wait for a complete live snapshot before checking the
original record counts. A partially written final record means observation
is still pending; the post-exit reader still validates every byte as complete
JSONL. The kill/reap, replay, size and stale-socket assertions are unchanged.
[The #846 regression and gate evidence](verification/bus-live-lines-20261015/README.md)
cover this test-only change; no recording or crash-recovery guarantee changes.


## Guarded Event bytes

`Publisher::prepare_guarded_event()` performs discovery, presence refresh and
retired-link cleanup before the caller supplies sensitive bytes. It may do
filesystem I/O and must stay outside the microphone owner and slot lock.
`publish_bytes_guarded(at_us, enc, bytes, authorized)` then uses the prepared
links only: it checks the supplied authority before/after encoding and before
each send/retry, with at most two attempts per link. It performs no filesystem
maintenance in that path and rejects Latest publishers, which would persist a
payload. Existing publication APIs keep their behavior.

The guarded encoder has a bounded temporary buffer, cleared on return or
unwind; it does not leave sensitive bytes in the reusable publisher datagram.
This is best-effort application buffer clearing, not secure physical erasure.
`PermissionDenied` may follow partial delivery: earlier kernel/receiver copies
cannot be retracted. Callers must preserve original source permission in the
callback; a fresh heartbeat must not grant old samples a new lifetime. Neither
this API nor ordinary expiry grace establishes consumer retention authorization.

## Unplug completion (#1674)

The opt-in core skill uses kept `body/unplug_request` and three short-lived
receipts: `body/unplug_state`, `expression/unplug_state`, `ring/unplug_state`.
The request survives service restarts until reboot or a matching kept
`body/unplug_cancel` from Skills ("never mind"); each receipt expires after
500 ms and carries the matching request identifier. [unplug.md](unplug.md)
defines the payloads, ownership and proof. No readiness reader treats ordinary
`body/joint_states`, pause or successful publication alone as safe completion.
