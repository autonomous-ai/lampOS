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
| Turns | The newest 20 turns, newest first, as V1's Flow: what took the turn (`local_speech`, or the lamp's own: `skill_says`, `lamp_tells_news`...; `lamp_speaks_first` in records from before #2825), then from the end of speech the final words, the first token and the first audio, each with its own time in brackets and the slowest marked; then how it ended (`played`, `no audio`, `failed`, or why it was cut) and what held it, if anything did | the voice's own session records ([below](#turns-from-the-voices-records)) |
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

Peek ignores a `transcript` record or any field not named. This reader-side
filter does not make a file safe for another account to open: shared files
use the producer projection described below. A name that is not plain (letters, digits, `-_./`) shows as `?`. A newer session starts
the turns again. A file first read when it is longer than 4 MB is read from
4 MB before its end: only the newest turns are kept. On this Mac, in
release, the first read of a 63 MB session took 7.5 ms, a read of one more
turn 0.26 ms, a read with nothing new 0.08 ms, and making the section of 20
turns 0.03 ms. Not yet timed on a lamp.

The rows carry a read status (#2789, piece 1). If the sessions root,
newest session's metadata folder, or an event file refuses access, Turns
says **"Can't read the voice's sessions: permission."** A missing path
says the sessions are not available yet; another I/O failure says the read
failed. These are fixed messages: no path, OS error text, or record's words
are sent to the browser. A readable empty session still says there is no
turn yet, so emptiness is distinct from a failed read.

Previously read rows remain visible with **"Last read is stale."** while
access is denied. This also holds when a newer session cannot yet be read:
its first successful read replaces the old session's rows. Overview marks
its first-audio summary unavailable or stale instead of presenting old
numbers as current. The next successful read clears the warning, including
on an already open event stream; no restart or page reload is needed.

One blocking worker reads the files, shared by all streams and cached for
one second after completion. Waiting streams yield the runtime thread, and
closing the requesting stream does not release the in-flight read or start
another one. The host target is under 2 s from a permission change to the
changed Turns and Overview events, with ordinary local file reads and a
responsive or cached unit-status read. The existing unit-status deadline
can still delay an entire push by up to 10 s ([below](#what-status-costs));
this is not a deadline for arbitrary filesystem stalls or a lamp timing
claim. This status reporting grants no new filesystem permission. The dedicated
reader policy is described below; the deployment unit and authorized lamp
check remain separate parts of #2789.

### Session-reader access

#2789's service-event permission contract pairs Voice's explicit
`LAMP_SESSION_READ_GROUP` with the same per-root `session_read_group` in
Records. Unset remains private. A dedicated local group can read only new
service-session event files (0640) through their invocation and metadata
directories (0750); health stays 0600. Records also accepts private historical
sessions and retains its ownership, filesystem, link, ACL and deletion
identity checks. [Records](records.md#optional-session-reader-group-2789)
describes the exact policy and rollout order.

After a successful grant, Voice serializes a separate flat metadata
projection before an event enters the writer queue (#3211). Every data row
has `shared_metadata: 1`. Only the twelve Turns kinds listed above retain
their kind; every other kind, including transcripts and malformed/unknown
kinds, becomes the fixed `service_event`. `service_sequence`, `turn` and
`at_us` are kept only as unsigned integers. The stage kinds may also keep
unsigned `decided_at_us`, `words_ms`, `cue_ms`, `chat_first_ms`,
`first_answer_audio_ms` and `after_decision_ms`; `turn_finished` may keep
unsigned `playback_gaps`. `admission_basis` and `outcome` come only from
fixed producer vocabularies. Unknown values and wrong types are omitted.
No nested data, transcript words, names, person/face/worker identifiers,
paths, arbitrary status text or unlisted fields are serialized. Segment
headers contain only the writer's fixed evidence label and counters.

This deliberately is not the full diagnostic trace. Generic rows preserve
sequence/count observations without revealing their original kind or data;
tools needing endpoint, yield or other detailed receipts must use a private
trace. New diagnostic fields need an explicit producer-schema review before
sharing. Peek's existing numeric Turns reader needs no schema change.

Unset, finite runs, failed group lookup and verified private grant rollback
keep their existing private trace format. The projection changes no live
bookkeeping, receipt, cue, admission or output authority. Health remains
private and uses its existing schema. It applies only to newly created
shared invocations, and does not redact any historical file already shared
by an older build.

An authorized office installation must prepare the voice parent for only
that group's traversal (0710) and the sessions root for its read/traversal
(0750), with trusted ownership and no ACL. Voice validates this preparation;
it does not widen ancestors or historical sessions. Group lookup and grants
happen before service workers, never on each turn or rotation. A failed
grant reports degraded access and verifies a private rollback before allowing
metadata writing to continue. If rollback cannot be verified, metadata writing
stops; healthy voice continues. Finite/diagnostic sessions do not opt in.

Projection uses fixed field lookups and bounded serialization with the existing
32 KiB packet, 64-packet queue and four 4 MiB segment limits. It performs no
filesystem work on the control/capture thread. The host target is under 1 ms
per record for record admission, projection and enqueue, measured separately
from asynchronous file writing. This is not a filesystem or device latency
bound. The permission setup still runs only at service startup.

Voice's service-trace tests keep a constant-size, per-writer phase snapshot
only in test builds. A failed completion observation reports fixed phase
labels, counts, microseconds relative to writer creation, segment slot indexes (0..n, not generations) and numeric health;
it carries no event values, words, identities or paths. The phases distinguish
event/header writes, segment syncs, final health write/sync, the writer-loop
and destructor return boundary, and acknowledgement. Counts are concurrent
observations, not a filesystem latency or scheduling diagnosis. Production
has no diagnostic fields or calls, and keeps its 500 ms finish limit. Test
observation budgets and the existing assertions stay unchanged: a timeout
still fails.
The stalled-writer handshake checks that a zero-budget observation reports
failure before release, cannot rewrite the original bounded report, and only
reports completion after the real acknowledgement. These diagnostics do not
explain or waive the earlier ARM64 timeout.

Before granting parent traversal, the installer must verify that every other
immediate child is an owned private 0700 directory or single-link 0600 file,
with no ACL. A known filename can be opened through a search-only parent
without listing it. Voice checks these siblings before preparing and granting
the new session and refuses extra-readable files, links or special nodes;
it never repairs their permissions. Its refusal cannot undo exposure from
an already unsafe external setup.

This does not give the reader membership in `lampos` or authorize a product
default. The dedicated account still needs a separate bus contract that
excludes raw speech and private state before delivery. Peek currently filters
raw `voice/heard` and `voice/said` payloads after receiving them, so a
subscriber-only grant alone would not protect their contents. Its own socket
must also prevent write/unlink access to another subscriber. The producer-side
metadata view and its owner remain a separate decision for #2789. The
own-account unit and an authorized lamp install/restart showing Turns plus an
unreadable outside
file remain separate acceptance steps. No physical acceptance is implied by
host permissions tests.

### Two sessions' stages side by side: `tools/day-stages.sh`

For comparing two sessions stage by stage (the first builder, 22 Oct: day
lines whose median grew from 1.59 s to 2.06 s), `tools/day-stages.sh
[--turns 3,5,7] [--last N] [--lines [GAP_MS]] SESSION [SESSION_B]` reads a session's
`metadata/events-*.jsonl` (its folder, its `metadata/`, or one file; a copy
with text blanked does). One row a turn, in ms: **wait**, end of speech to
the line's end; **words**, **cue**, **chat** and **audio** after the line's
end; **sound**, the first sound written after the end of speech. Then each
column's median and p90, and with two sessions the second's medians less
the first's. Of a record it keeps only `kind` and these fields:

- `local_endpoint`: `at_us` (the line's end, the decision),
  `last_block_host_read_us` less `includes_silence_wait_ms` (the end of
  speech), `early_end.before_this_ms`; one with `not_speech` or
  `onboarding_near_call` is no line;
- `answer_stages`: `decided_at_us`, `words_ms`, `cue_ms`, `chat_first_ms`,
  each less `early_end.before_this_ms` when the line went early (row
  marked `e`), so all count from the decision;
- `answer_first_audio`: `after_decision_ms`;
- `speaker_first_write`: the turn's earliest `at_us` (a wait cue counts).

A folder's `events-0..3.jsonl` are one run's rolling segments (one turn's
records can span two), so turns are joined across them by number; a voice
restart is a new session folder, and two sessions are never merged.

`--lines [GAP_MS]` (the first builder, 22 Oct: twelve day lines heard as
23 turns against 18) then groups the kept turns (after `--turns` and
`--last`) into the lines said: in time order, a turn whose speech start
(`input_admitted.prefix_first_host_read_us`, else its `at_us`) is less than
GAP_MS (default 2000) after the previous turn's end of speech is the same
line. One row a line: its first turn, its turns, each turn's
`endpoint.basis` (`-` if none) and whether it was answered
(`answer_first_audio`). Under it: `lines N`, `heard as 2+ turns M (turns in
them K)`, the bases that ended a line's non-last parts (fragment ends) and
all turn ends; with two sessions, one line comparing "heard as 2+ turns".

Tests: `tools/day-stages-test.sh`. Needs `jq`.

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
set up, and a reset that reboots leaves it off with that marker gone. A
reset started in place (#4500, setup.md) stops it with the lamp: it is not
part of `lampos.target`, so it is among the reset's `WRITERS` (`reset.rs`),
and it is not started again, since on port 80 it would keep the port from
setup, which ends serving at once on a port still held (`serve.rs`). It
comes back at the lamp's next boot once setup is done. Before this, an
in-place reset on such a lamp left setup with no page on the hotspot
(#4536). Product lamps don't install it. It has no `Conflicts=`
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
- `turns::tests::denied_root_metadata_and_file_keep_the_read_status_and_recover`:
  real permission refusals at all three boundaries, first-read unavailable
  and later stale rows, and an unfinished line recovered without losing bytes.
  Permission tests assert that the OS refuses the read; run them as an ordinary
  user, as native ARM64 CI does.
- `turns::tests::a_denied_new_session_keeps_the_old_rows_stale_until_it_can_be_read`
  and `turns::tests::missing_and_failed_reads_are_distinct_from_an_empty_readable_session`:
  a denied new session keeps the old rows until recovery; missing, failed,
  and successfully empty reads stay distinct.
- `serve::tests::a_cancelled_turn_read_keeps_its_cache_lock_without_blocking_the_runtime`:
  a held filesystem-side lock and a cancelled requester keep one read in
  flight while another task can still run on the single runtime thread.
- `the_stream_reports_denied_sessions_and_recovers_without_words_or_paths`:
  one authenticated HTTP event stream moves from denied to readable, then
  stale and recovered; an unauthenticated request sees no events or error
  status. Synthetic words and paths stay out, old metrics are marked stale,
  and unread bytes appear once after recovery. It prints the first denied
  read and each permission-change-to-event duration and checks the host's
  under-2-second target with a fast synthetic unit-status reader.
