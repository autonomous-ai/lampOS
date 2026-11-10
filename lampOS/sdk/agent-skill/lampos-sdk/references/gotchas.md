# Gotchas we hit

Each of these cost someone a run on a real lamp.

## On a customer lamp today (stable 11450)

Customer lamps run stable build 11450 (lamp-voice `135bc417d4`, promoted 10
Nov). lamp-control is on every lamp and answers once the owner turns on
developer access in the app. Five gaps below. Already in 11450: skill push
(from 11091; the owner says yes at the lamp), `joints` for `roll`, `shoulder` and `elbow`
(the body's pose, only while the lamp stands), a head tap and pet as `touch`
events, and a new motion that replaces the one playing.

1. **A motion's end.** `wait=True` on `gesture`, `move` and `express` returns
   when the lamp takes the motion, not when it ends; the next build ends it
   at the body's `done` (hello's `limits.motion_done`). `look` and `joints`
   with `wait=True` already return the body's `done`.
2. **Play and a look at a face.** `play` is refused, "not routed yet: waits
   for client packages". `look` at `face` and `name` are refused, "not in
   this release". Send a recorded move as `move`; look at `ahead` or an
   angle. With the body silent for 15 s, name both yaw and pitch in
   `joints`. `sound` works.
3. **Limp and pose** (letting the arm go limp, holding a pose) can't be
   reached from a computer.
4. **No mDNS.** No lamp announces itself: `lamp find` finds nothing, and
   `lamp pair` without `--host` finds nothing. Pair with
   `--host lamp-xxxx.local` (the lamp's host name) or its IP address. The
   pairing keeps the address, so later connects need nothing.
5. **Event times are UTC.** `at` ends in `Z` (`2026-10-27T08:40:01Z`).
   Convert to local time yourself.

And on older stables (10824 and before):
- **A motion asked while another plays was refused `busy`**, "busy: a motion
  plays for N ms more", with `retry_ms`: wait `retry_ms` and ask again. 11450
  replaces the playing motion instead (below).
- **`look` on a resting lamp could end `cut_short`**, "not there in time:
  something holds the head", with no hand on it. From 11048 it ends `finished`
  or `clamped` with where the head stopped. `look` at `name` is refused, "not in
  this release" (no sound direction), and `face` is refused on 11450 and older,
  while on the next build it turns once toward the face in view or ends
  `dropped` "no face in view"; `ahead`, a yaw and pitch, and `hand` work.
- **The camera** sends the eyes' newest frame, about 1 to 2 a second; the
  fast path (`fast: true`) is not on customer lamps.

`lamp fake` refuses every `look` (but with `--hands`) and every `joints` by
default, which a customer lamp takes in part; and it takes a skill push and
an overlapping motion, which a customer lamp refuses. Test those paths
against the gaps above, not only the fake.

## Light

- **`breathe` needs level 2 or more; `candle` 3; `pulse` 2.** Below that the
  lamp refuses it (`refused`, "a breath's low is from 1 to under the light's
  level"). Steady light works at level 1.
- **One pulse a minute per computer.** A second `pulse` within a minute is
  answered OK, and shown as steady light, without its pulse. Nothing tells
  you. Use `breathe` for repeated attention, or wait a minute.
- **A light lasts 30 s at most** (`for_ms` up to 30,000). Renew it about
  every 25 s to keep it; when you stop, the person's own light comes back.
- **Closing the connection ends your light at once** (and your `say`,
  waiting or playing, as `stop` does). A one-shot script must stay connected while the light should
  show: `await asyncio.sleep(20)` before leaving `async with`.
- **`get("light")` has two lights.** `on`, `colour`, `level` are the
  person's own setting; `shows` is what the ring draws right now (yours
  while it lasts). Read `shows` to see your light, not the top level.
- **At night the lamp dims it,** and `express` may skip its light when the
  person's light is at its dimmest (the answer says so).

## Body

- **A new motion cuts the one playing short** (from stable 11048). A
  gesture, move or feeling's move asked while another plays (whoever asked
  it) replaces it, eased out from where the body is under its own caps.
  While the person is using the lamp (a conversation, a hand on it), the
  body drops the new one ("the person is using the lamp"); the door has
  already answered `{"to":"body"}`. **Stable 10824 and older** refuse it
  `busy` with `retry_ms` instead: wait `retry_ms` and ask again.
- **`look`, `move`, `play` and `joints` need a `body` lease;** `gesture` and
  `express` don't. Hold the lease around a sequence of moves, not per move.
- **The lamp takes the body back** when someone talks to it, touches its
  head, or sets it private. Listen for `lease` events with `state: lost`.
- **Not every lamp routes every op yet.** A lamp may refuse `joints` or
  `play` (and a build from before the hum, `sound`) with `refused`, "not
  routed yet: waits for …". Handle it; don't retry. `joints` takes `yaw` and
  `pitch` (degrees from home, as `body_info()` gives them, both named until
  the body has said where it is); `roll`, `shoulder` and `elbow` go as the
  body's pose, only while the lamp stands (else `done` `dropped`, with why);
  a build from before that refuses them, "not routed yet".
- **A move must start and end at home** (all five joints 0) and last 1.5 to
  10 s, within each joint's reach and speed. The body's words say which
  frame was wrong.
- **A move's cells are whole counts** (integers). A value like `"0100"` or
  `1.5` from a form is refused by the SDK before sending (`bad_request`),
  so it never reaches the lamp and the lamp's journal has nothing. Validate
  and convert what a person typed before you call `move`.
- **A body lease you hold keeps the lamp's own life standing back** (its rest
  glance, its feelings' small moves). A program that looks now and then
  should let the lease go when idle, not hold it for hours.

## Voice

- **Quiet hours** (`limits.quiet_hours`, off unless this lamp sets them)
  refuse `say`, `sound` and `turn.start`. Use light then.
- **`say` is refused while the lamp talks with someone, or is private.**
- **Turns and your mute: the person wins.** A turn you start answers the
  person's next line as spoken to the lamp, but what they say never reaches
  your computer: you get `done` (`finished` once the answer sounded,
  `dropped` with why, e.g. "dropped: nobody spoke"), never the words.
  `end_turn` ends only your own turn (`busy` otherwise). Your mute
  (`set_listening(False)`) is yours alone: a hand on the head lifts it
  (`done` `cut_short`, "a hand on the head", on the request that held it),
  and it ends when your program's last connection closes or it is revoked,
  so a crashed program never leaves the lamp deaf. A `stop` doesn't lift it.
- **A restart of the lamp's voice can leave it muted** by its restart guard:
  the ring shows it muted and the lamp doesn't hear. One tap on its head
  clears it. Expect it after a lamp update or a voice restart.

## Your handlers

- **Catch errors inside every event handler.** The lamp keeps talking, but
  an error your handler throws isn't returned to you: Python logs a
  coroutine handler's as a task exception, JS emits it as `handlerError` on
  the socket. Wrap the
  body in `try`/`except LampError` (JS `try`/`catch`) as the patterns do.
- **A handler can run while the last one still does:** events don't wait for
  your handler. Keep a flag or a time if two greetings must not overlap.

## Connection

- **The lamp restarts on every update:** the connection closes
  (`wait_closed()` returns, a call fails `closed`). Reconnect with backoff.
  A reconnect is a new session: it loses every subscription (subscribe
  again), every lease (take again, only when you need it) and a camera
  stream (start it again). See "A program that runs for days" in
  [patterns.md](patterns.md).
- **`get` with several states fails as a whole** when one of them isn't
  known to this lamp (a newer state on an older lamp): `bad_request`, and
  you get none. A program for many lamps should `get` them one at a time, or
  catch `bad_request` and show "not on this lamp".
- **A connection that only listens is kept open by pings** the SDK sends;
  don't add your own keepalive.
- **Grants are read at hello:** after the owner changes them, reconnect.
- **Each event carries `at` in UTC** ending `Z`, not the lamp's local time.
  Parse it with a timezone-aware parser: an older fake stamps its own offset.
- **`lamp.limits` keys can be missing** (an older lamp, the fake): read them
  with `.get()`, never `limits["quiet_hours"]`.
