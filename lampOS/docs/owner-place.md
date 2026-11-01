# The owner's place (#4333)

**What exists and what we take.** Before this, "find a coffee place near
me" made the lamp ask where "near you" was, and it kept no place said
earlier. A lamp had only its unit's static `LAMP_PLACE` ("Hanoi, Vietnam"),
told beside every request, or the city its time zone names. We take the
app's settings path as the names and the zone already use it (`lamp-cloud`
to root's helper `lamp-setup change`, `cloud.md` and `setup.md`, "Changed
after setup"), the person's own folder (`/etc/lampOS/person/`) and the
reset's wipe of it, the second route's note beside a line's words (as the
staff note, `second-voice.md`), the weather's function, and the speech
key terms. Nothing new runs; everything is off unless
`LAMP_OWNER_PLACE=on`.

## The app's contract

| Kind | `data` | Replies |
|---|---|---|
| `owner.place` | `{"lat": number, "lon": number, "name": "Thao Dien, Thu Duc", "set_at": "2026-10-29T08:30:00Z"}`: where the lamp stands, set by the person | `starting` at once; then `success` with what was kept, `{"name", "lat", "lon", "set_at"}` (rounded, `set_at` to the second with `Z`), or `failure` with `error` |
| `owner.place.clear` | `{}` (not read) | `starting`; then `success` with `{"cleared": true}`, also when nothing was kept, or `failure` |

- Every reply is `{"type": "data", "kind", "status", …}`, as every
  settings kind's (`timezone.set`). A refusal of the request's shape comes
  at once as `failure` with no `starting`: `lat must be a number from -90
  to 90`, `lon must be a number from -180 to 180`, `name must be 1 to 64
  characters on one line`, or `set_at must be when it was set, in ISO 8601
  UTC, such as 2026-10-29T08:30:00Z` (`Z`, `+00:00` and a fraction of a
  second are taken; another offset is not; years 2020 to 2099). No error
  holds a value. Fields it does not know are ignored and kept nowhere.
- `name` is an area name: the neighbourhood, district or city as the
  phone's geocoder gives it, never a street address. The app builds it
  from the geocoder's sub-locality and locality ("Thao Dien, Thu Duc").
  The lamp checks only its length and that it is one line; it does not
  refuse digits, since "District 1" is an area's name too, and it cannot
  tell an address from an area, so the app must never send one.
- `lat` and `lon` are where the lamp stands, set by the person: at setup,
  when they are with the lamp, or when they tap Update now. They are never
  the phone's position sent on its own as the owner moves; the app sends
  no place again when the phone moves.
- `lat` and `lon` are rounded by the app to 2 decimals before it sends them
  (app v3), so the exact position never crosses the relay; the lamp rounds
  again itself (below) and checks the ranges.
- **Capability:** with the switch on, `system.info`'s `can` lists
  `owner.place` and `owner.place.clear`. Without it neither is listed and
  `owner.place` is answered `failure` with `not on this lamp: owner.place`,
  as any kind this lamp lacks. `owner.place.clear` is still taken, unlisted,
  whenever root's helper is there (`LAMP_CLOUD=on`, `LAMP_SETTINGS_CHANGE=on`):
  a place kept while the switch was on is never left behind when it is
  turned off or a unit drops it (#4356). The app sends it only to a lamp
  whose `can` lists it, so this is for the lamp's own sake.
- Setting and using the place need `LAMP_CLOUD=on`, `LAMP_SETTINGS_CHANGE=on`
  and `LAMP_OWNER_PLACE=on` in `lamp-cloud`'s unit, and `LAMP_OWNER_PLACE=on`
  in `lamp-voice`'s. Clearing it needs no `LAMP_OWNER_PLACE`.

## The rounding

`lamp_msgs::owner_place::rounded`, run by `lamp-cloud` before the line
leaves it, again by root's helper before it writes, and again by every
reader:

- latitude in steps of 0.01° (1.11 km);
- longitude in steps of 0.01° times the least whole number that keeps a
  step at least 1 km wide at the cell's edge nearest the pole: one
  hundredth up to about 25° from the equator, two up to about 63°, more
  nearer the poles (`longitude_step`; at a pole, longitude is 0).

So a cell is never finer than a kilometre each way, whatever the phone
sends; rounding a rounded place gives it back. Ba Dinh's
`21.035123, 105.834987` is kept as `21.04, 105.83`.

## Where it is kept

`/etc/lampOS/person/place`, beside the person's name (setup's `name` path,
`beside_name`), root's, 0600, one JSON line:
`{"name","lat","lon","set_at"}`. Nothing else keeps it: not memory, not
the journal. The journal says only `the owner's place was set` or
`… cleared` (`lamp-setup change`), and the voice `its words need a place;
the model is told the owner's place`, never the name or where.

The lamp's own answers that name the area ("the weather around Thao Dien
…") are kept like any reply it gives: in the conversation and in the
memory, as everything said at the lamp is. An app clear removes the place,
not those answers; the owner's "forget everything" removes both.

It is gone with, each whether the switch is on or not (#4356):
- `owner.place.clear` from the app;
- the factory reset: the product's reset plan wipes it
  (`crates/setup/fixtures/reset-plan-product.json`);
- the owner's "forget everything about me" said at the lamp, on the second
  route, once their memory is emptied (`owner_place::forget_with_everything`,
  the same mark as the face's, `memory.md`): the answer says
  `"place": "forgotten too"`. With the switch off a kept place still goes,
  so "everything" never leaves it behind. Only the owner's own line: the person whose
  memory id is the typed name's or the owner the onboarding kept with their
  face, either name they go by (`person::owner_ids`). Another
  person's, a room's, Live's and texts' leave it.

## What the voice does with it

On the second route (`LAMP_VOICE_ROUTE=second`), with the switch on:

- **A line that needs a place** (`owner_place::asks_for_a_place`: "near
  me", nearby, nearest, the weather, opening hours, how far, a few
  Vietnamese phrases) is asked with a note beside its words: the area's
  name and its rounded position, to answer for there unless they named a
  place, and to name the area, never the numbers. Any other line, and
  every text from the app, is told nothing of it. The file is read again
  for each such line, so a set or a clear holds from the next question.
  With none kept, nothing is told and the model asks where, as before.
- **The lamp's place:** while one is kept, the unit's `LAMP_PLACE` is not
  told beside every request, so the model is never given two places, and
  the weather's function called with no place answers for the kept place's
  rounded position, by its name (never looked up by name). A clear puts
  `LAMP_PLACE` and the zone's city back. The time's "Here in …" stays the
  unit's. In its place beside the time the model is told
  `local_time::place_told_owner_kept`: that where they are is told beside a
  request that needs it, to ask which area they mean only when not sure it
  heard a place, and to say where it looked when nobody named it. So it is
  never told it knows no place beyond its zone, nor to ask before every
  search (#4337's words for a lamp with no place).
- **Speech to text** listens for each part of the name between commas
  ("Thao Dien", "Thu Duc"), after the unit's `LAMP_SECOND_KEY_TERMS`, read
  every 10 s on a thread of its own (`owner_place::start_terms`): a set or
  a clear reaches the next stream opened after it; the owner's forget
  clears them at once. The names go to the speech service with every
  stream, whatever is asked, as the unit's words do.
- **Where it goes off the lamp:** the area's name and rounded position go
  to the model's provider only in the note beside a place question; the
  rounded position goes to the weather service (Open-Meteo's forecast,
  `api.open-meteo.com`) when the weather is asked with no place; the area's
  name may go to a search the model makes; the name's parts go to the speech
  service as key terms. The exact position never reaches the lamp.

Every answer that uses it names the area (the weather's `place`, the
note's instruction, the line beside the time), so the lamp can say which
place it used (#4337).

## Not done

- The Live route (`LAMP_VOICE_ROUTE=gemini`) is told nothing of it.
- A chat begun on a line's early words (`LAMP_SECOND_PREPARED`, off in the
  product) is dropped for a place question, as for a staff name.
- No `owner.place.get`: the app shows what its last `success` said; after
  a forget at the lamp the app's copy is stale.
- Not yet played on a lamp: "what's a good coffee place near me?" with a
  place kept, then after a clear.
