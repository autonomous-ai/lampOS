# Gotchas we hit

Each of these cost someone a run on a real lamp.

## Light

- **`breathe` needs level 2 or more; `candle` 3; `pulse` 2.** Below that the
  lamp refuses it (`refused`, "a breath's low is from 1 to under the light's
  level"). Steady light works at level 1.
- **One pulse a minute per computer.** A second `pulse` within a minute is
  answered OK, and shown as steady light, without its pulse. Nothing tells
  you. Use `breathe` for repeated attention, or wait a minute.
- **A light lasts 30 s at most** (`for_ms` up to 30,000). Renew it about
  every 25 s to keep it; when you stop, the person's own light comes back.
- **Closing the connection ends your light at once** (and a `say` still
  waiting). A one-shot script must stay connected while the light should
  show: `await asyncio.sleep(20)` before leaving `async with`.
- **`get("light")` has two lights.** `on`, `colour`, `level` are the
  person's own setting; `shows` is what the ring draws right now (yours
  while it lasts). Read `shows` to see your light, not the top level.
- **At night the lamp dims it,** and `express` may skip its light when the
  person's light is at its dimmest (the answer says so).

## Body

- **One motion at a time.** A gesture, move or feeling's move asked while
  one still plays is refused `busy` with `retry_ms` (whoever asked it).
  Wait `retry_ms`, or don't ask so often: a gesture is 2 to 10 s.
- **`look`, `move`, `play` and `joints` need a `body` lease;** `gesture` and
  `express` don't. Hold the lease around a sequence of moves, not per move.
- **The lamp takes the body back** when someone talks to it, touches its
  head, or sets it private. Listen for `lease` events with `state: lost`.
- **Not every lamp routes every op yet.** A lamp may refuse `joints` or
  `play` (and a build from before the hum, `sound`) with `refused`, "not
  routed yet: waits for …". Handle it; don't retry.
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

- **Quiet hours** (`limits.quiet_hours`, by default 22:00 to 07:00) refuse
  `say` and `sound`. Use light then.
- **`say` is refused while the lamp talks with someone, or is private.**
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
