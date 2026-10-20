# Peek: a read-only page on an office lamp

`lamp-peek` (`crates/peek`, #2141) is a page for builders and testers to see
what an office lamp sees and is doing. It came out of `where_you_sit` failing
twice on lamp-52e6 on 20 Oct 2026. The cause was a person sitting to the side,
and only a Mac's camera pointed at the lamp showed it.

It is **off unless `LAMP_PEEK=on`**. It is **never in `deploy/product`**, and
**never on a person's lamp**: putting it there would loosen privacy, which is
the owner's call.

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
  value. A diagnostic's message is never shown.
- **No audio, no recording, and nothing kept.** What `/bus` holds is in memory,
  60 lines at most.

## It reads, it never writes

- Every view is a GET, and every other method is refused (405).
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

## What `/status` costs

A read runs `systemctl show` once and `journalctl` once per unit (20 to 30
on a lamp), and reads the newest session's `events-*.jsonl` for its
`endpoint_worker` lines. So the rows are kept for 30 s
(`serve::STATUS_KEPT`), and a tab refreshing every 5 s reads them at most
twice a minute. A read hasn't been timed on a lamp yet. Time one on the
office lamp before a `/status` tab stays open beside a timed row.

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
