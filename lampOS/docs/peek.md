# Peek: a read-only page on an office lamp

`lamp-peek` (`crates/peek`, #2141) is a page for builders and testers to see
what an office lamp sees and is doing. It came out of `where_you_sit` failing
twice on lamp-52e6 on 20 Oct 2026. The cause was a person sitting to the side,
and only a Mac's camera pointed at the lamp showed it.

It is **off unless `LAMP_PEEK=on`**. It is **never in `deploy/product`**, and
**never on a person's lamp**: putting it there would loosen privacy, which is
the owner's call.

## The one page

`/` is one page with a sidebar: **Overview, Turns, Camera, Body, Health**
(the lead's second step on #1711, 20 Oct, after V1's monitor). One event
stream (`GET /events`, server-sent events) keeps every section fresh: the
camera's frame four times a second (`serve::PUSH_EVERY`), the others once a
second. Each section is HTML made and escaped on the lamp (`crates/peek/src/page.rs`).
The page's few lines of script only put it in its place, and read nothing
from the address but which section is shown.

| Section | What it shows | Where it comes from |
|---|---|---|
| Overview | The voice's state; someone here or not; the camera's state, faces in view and who is followed; the lamp's health level and its worst item; the newest turn's first audio and the median of the last ten; any unit running another build than its release | the kept values below, the turns, `/status`'s rows |
| Turns | The newest 20 turns, newest first, as V1's Flow: what took the turn (`local_speech`, or the lamp's own: `skill_says`, `lamp_speaks_first`...), then from the end of speech the final words, the first token and the first audio, each with its own time in brackets and the slowest marked; then how it ended (`played`, `no audio`, `failed`, or why it was cut) and what held it, if anything did | the voice's own session records ([below](#turns-from-the-voices-records)) |
| Camera | The frame `/cam.svg` draws, faces as boxes | as `/cam` |
| Body | Each joint where it is against where it was last asked to be (`commanded`), how far off (marked past 0.05 rad) and its effort; the body's own word: its level and sentence, its state (`ready`, `paused`, `stopping`), `standing` from 0 folded to 1 standing, the warmest joint, "met resistance" in the last minute and since it started; its last stop and why, as it said it | `body/joint_states`; the body's diagnostic, sent by the body about the body (`watch::body_of`) |
| Health | Each part's item as `lamp-health show` gives it: its level, its message as it says it (cut to 160 characters, escaped), and how long ago; the parts never heard from; each unit's build | `lamp/health`; `/status`'s rows |

### Turns from the voice's records

The newest session's `metadata/events-*.jsonl` is read on from where the
last read ended, at most once a second whoever asks (`turns::Turns`). Only
these kinds are taken, and of each only the fields named:

- `input_admitted`: `admission_basis`;
- `answer_stages` (the second route's own count, each from the end of
  speech): `words_ms`, `chat_first_ms`, `first_answer_audio_ms`;
- `answer_first_audio`: `after_decision_ms`, the first audio on either route;
- `turn_finished`: `outcome`;
- what held a turn or a sound in it: `floor_held`, `answer_waits`,
  `input_let_go_answer_waits`, `input_let_go_own_line`,
  `input_let_go_body_moving`, `asker_first_answer_held`,
  `provider_turn_unclear`, `provider_turn_failed`.

A `transcript` record, or any field not named, is never read. A name that
is not plain (letters, digits, `-_./`) shows as `?`. A newer session starts
the turns again. A file first read when it is longer than 4 MB is read from
4 MB before its end: only the newest turns are kept. On this Mac, in
release, the first read of a 63 MB session took 7.5 ms, a read of one more
turn 0.26 ms, a read with nothing new 0.08 ms, and making the section of 20
turns 0.03 ms. Not yet timed on a lamp.

## The three views

| View | What it shows | Where it comes from |
|---|---|---|
| `/cam` | The camera's frame (320 × 256, the detector's picture) with each face as a box, asked for again four times a second (`/cam.svg`). Each box shows its passing id (`f3`), `facing` or `turned away`, and `speaking`. The face the lamp follows is marked `(followed)`, with its point. The caption says `someone here` or `nobody for N s`. | `humans/faces/tracked`, `expression/body`'s gaze, `room/presence`: kept values, read when asked |
| `/bus` | The kept values of `voice/state`, `humans/faces/tracked` (a count), `room/presence`, `expression/body` (who it follows) and `body/joint_states` (positions). Then the events' counts, and the newest 60 events, filtered by `?topic=`. | Kept values when asked; `voice/heard`, `voice/said`, `voice/turn` and `diagnostics` by a subscriber |
| `/status` | Each `lamp-*` unit and `lampos.target`: its state, the build its main process runs against the build its unit starts (a row in red when they differ), and systemd's last error line about it. Then the voice's end-of-speech worker's newest state. Read at most every 30 s, and the page says how long ago. | `systemctl show`, `/proc/<pid>/exe`, `journalctl -p err`; the newest session's `endpoint_worker` records |

### What it never shows

- **No picture.** The page never opens the camera: the eyes keep it. A box is
  four numbers, and the frame is a plain rectangle.
- **No names.** The faces topic carries none: the eyes keep names to
  themselves. Putting names on the page would need the eyes to publish them,
  which is the lead's to decide (#1711).
- **Nothing while the camera is closed.** Boxes are drawn only while the eyes'
  own `diagnostics` (sent by the eyes, about the eyes) say `camera open` and
  have said so within 3 s. When they
  say `closed` (the privacy switch, a mute), `not opening` or `failed`, or say
  nothing, the frame shows just that, with no box, point or presence. The
  faces kept on the bus are not even read.
- **No words.** Of `voice/heard` and `voice/said` it keeps only whether the
  piece is finished, the request number and the locale, never `text`. Of a
  diagnostic it keeps the part's name and level, and the eyes' `camera`
  value. A diagnostic's message is never shown on `/bus`. The one page
  shows two sentences the lamp makes about itself: the body's own (its
  diagnostic, sent by the body about the body, with only its named values),
  and each part's health item, as the lead asked (#1711, 20 Oct). Both are
  cut to 160 characters and escaped. The turns are numbers and kinds only.
- **No audio, no recording, and nothing kept.** What `/bus` holds is in memory,
  60 lines at most.

## It reads, it never writes

- Every view, and the one page's stream, is a GET, and every other method is
  refused (405).
- It never publishes on the bus. Its one mark there is the subscriber socket
  each reader of an event topic binds (`peek.*` in `<topic>/subs/`).
- No setting, no button, no update. V1's `/monitor` lost a lamp's install to
  two clicks.
- Its unit makes the whole system read-only to it (`ProtectSystem=strict`),
  except the bus root.

## Turning it on (office lamps)

The unit is a template beside the crate, `crates/peek/lamp-peek.service`. It is
filled in as `deploy/units/README.md` says, and is not part of
`lampos.target`. A drop-in switches it on:

```ini
# /etc/systemd/system/lamp-peek.service.d/on.conf
[Service]
Environment=LAMP_PEEK=on
```

It answers on `127.0.0.1:8077`. **Reach it through ssh:**
`ssh -L 8077:127.0.0.1:8077 <lamp>`, then open `http://127.0.0.1:8077/`.

### One address on an office lamp: `http://lamp-xxxx.local/`

On an office lamp the drop-in can put it on port 80, as V1's monitor was,
with the token (below):

```ini
# /etc/systemd/system/lamp-peek.service.d/on.conf
[Service]
Environment=LAMP_PEEK=on
Environment=LAMP_PEEK_LISTEN=0.0.0.0:80
LoadCredential=peek-token:/etc/lampOS/peek/token
```

avahi already announces the lamp's name (`lamp-xxxx`, `docs/setup.md`). The
unit takes port 80 with one capability, `CAP_NET_BIND_SERVICE`, and no
other (`AmbientCapabilities=` and `CapabilityBoundingSet=`). It never runs
beside setup, whose hotspot page and nginx have port 80:
`ConditionPathExists=/var/lib/lampos-setup/done` keeps it off a lamp not
set up, and a reset reboots with that marker gone. It has no `Conflicts=`
with `lamp-setup.service`: both are wanted by `multi-user.target`, and at
boot after a reset systemd would keep this unit's start and drop setup's,
and then this unit's condition fails and nothing listens (the reviewer on
#2259). A lamp set up while it was off starts it at the next boot, or by
hand.

Before the drop-in, check nothing else holds port 80 on that lamp: an
office lamp that still has V1's nginx enabled keeps it, and the page then
fails to bind until its start limit (`StartLimitBurst=5`).

Without a token it answers only when `Host` is `127.0.0.1`, `localhost` or
`[::1]` (403 otherwise). Then no other page in the same browser can reach it
under a name of its own pointed at `127.0.0.1` (DNS rebinding).

### Beyond the lamp: the token

**The page is plain HTTP.** On the network, the token in the form, and then
the cookie, cross it in clear with every request. Anyone on that network can
read the token and open the page. The ssh tunnel above is the way to reach
it. A token is only for a network the office trusts.

To answer on the network (`LAMP_PEEK_LISTEN=0.0.0.0:8077`) it needs a token,
passed by `LoadCredential=peek-token:/etc/lampOS/peek/token`. The token is 16
characters at least, made of letters, digits, `-`, `_` and `.`. Without a
token, or with one too short, it refuses to start (exit 2).

- **The token is never in the address.** An address lands in a browser's
  history and a proxy's log, so `?token=` is refused (400) even when it is the
  right one. The page asks for the token in a form (POST `/token`), then keeps
  it in a cookie (`HttpOnly`, `SameSite=Strict`). A script may send
  `Authorization: Bearer <token>` instead.
- Without the token, a request gets only the form: nothing of the lamp.
- Every answer is `Cache-Control: no-store`, `Referrer-Policy: no-referrer`,
  `X-Frame-Options: DENY`.

## What it cannot see

- **A part's own errors.** "systemd's last error" is the unit's newest line at
  priority error (`journalctl -p err`). The parts write their errors to
  stderr at info priority, so the column holds only systemd's own lines about
  a unit: a main process that exited, a start limit hit. A part's own error,
  or a panic, never shows there. An empty column is not "no trouble".
- **Builds of parts that run as another user.** `/proc/<pid>/exe` is readable
  only for its own user's processes, so a part running as another user shows
  `?` for its running build. Its release still shows.
- **The turn's owner.** It lives inside the voice, not on the bus.
- **Torque on or off.** The body doesn't say it on the bus. Its state says
  `stopping` as it stops, and its sentence then says why, "stopped with
  its torque off" for an overload. `effort` is how hard each joint works.
- **Seated.** The body says `standing`, from 0 folded to 1 standing; the
  seated measure (#2163) isn't on the bus.
- **A Live turn's steps.** Gemini Live sends no `answer_stages`, so a Live
  turn shows only its first audio after the decision.

## What `/status` costs

A read runs `systemctl show` once and `journalctl` once per unit (20 to 30
on a lamp), and reads the newest session's `events-*.jsonl` for its
`endpoint_worker` lines. So the rows are kept for 30 s
(`serve::STATUS_KEPT`), and a tab refreshing every 5 s reads them at most
twice a minute. A read hasn't been timed on a lamp yet. Time one on the
office lamp before a `/status` tab stays open beside a timed row. The one
page's Overview and Health use the same rows, kept the same 30 s, and one
read runs while the others wait for it (the reviewer on #2259), so an open
page reads them twice a minute however many tabs show it.

**A read has a deadline** (`units::READ_WITHIN`, 10 s in all; the reviewer
on #2259). While it runs, every `/status` and every push of the one page
waits for it, so a hung `systemctl` or `journalctl` would hold them all.
Each command is killed at the read's deadline (`units::output_by`), and
then tells nothing. The rows not read by then have no last error, and a
`systemctl show` past it gives no rows. So the page waits at most 10 s,
then goes on. A command's output is read as it comes, so a long one never
fills its pipe and stalls.

## Tests

`cargo test -p lamp-peek` runs the page on a real bus in a directory of its
own, and asks for it over HTTP:

- `every_view_reads_only_and_answers_only_get`: the bus's directory is the
  same after every view is asked for twice; POST, PUT, DELETE and PATCH get
  405; and no topic lists `peek` as a publisher.
- `with_the_camera_closed_no_box_is_drawn`: another part saying the eyes'
  camera is open is not taken; no word from the eyes means no box; `open` draws both faces; `closed` draws no box, id, point or presence,
  though the faces are still kept on the bus.
- `the_face_the_lamp_follows_is_marked_with_its_point`.
- `a_word_heard_never_reaches_any_view`: a planted word, sent heard, said and
  in a diagnostic's message, is on no view, while the counts are.
- `without_a_token_it_answers_only_as_this_machine`: `localhost`,
  `127.0.0.1` and `[::1]`, with or without the port, get 200; other names get
  403. With a token, any name does, the token being the gate.
- `with_a_token_only_the_form_or_the_header_opens_it`: a token in the address
  is refused, even the right one; the form sets the cookie; the cookie and the
  header open the views; a wrong token does not.
- `units::tests::a_unit_running_another_build_than_its_release_shows_both`:
  a unit running another build than its release is the one red row, and an
  error line of `<script>…` is shown escaped, never as markup.
- `beyond_this_machine_without_a_token_it_does_not_start`: the binary exits 2
  for the network with no token or a short one, and 0, saying it is off,
  without `LAMP_PEEK=on`.
- `the_one_page_streams_each_section_and_never_a_word`: on a real bus and a
  session folder of its own, the stream carries all five sections:
  - a turn's steps with their own times and the slowest marked;
  - a turn the lamp began itself, and what held it;
  - an elbow 0.2 off where it was asked to be;
  - the body's values, and not another part's speaking as the body;
  - a health item's `<script>` escaped, and a part never heard from.

  A word planted in a `transcript` record, in `voice/heard` and among the
  body's values is in no section, nor on the page.
- `the_stream_reads_only_and_asks_for_the_token`: POST, PUT, DELETE and
  PATCH get 405; without the token, 401 and no event; with its cookie, the
  sections.
- `turns::tests::a_turns_steps_and_end_are_read_from_its_records_and_nothing_else`
  and `turns::tests::the_records_are_read_on_from_where_the_last_read_ended`:
  - only new lines are taken, and half a line waits for its end;
  - only the newest 20 turns are kept;
  - a long file is first read from near its end;
  - a newer session starts again.
- `tests::the_unit_takes_port_80_only_once_setup_is_done`: the unit's
  setup condition, no `Conflicts=` with setup, and its one capability.
- `the_units_are_read_once_however_many_ask_at_once`: four asking at once
  while the rows have run out make one read.
- `units::tests::a_command_past_its_deadline_is_killed_and_tells_nothing`:
  a command that ends tells its output; `sleep 30` with a 200 ms deadline
  tells nothing, well within 5 s; past the deadline none is started.
