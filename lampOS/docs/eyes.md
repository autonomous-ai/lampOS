# The lamp's eyes

`lamp-eyes` (crate `lamp-eyes`: `src/picture.rs`, `src/faces.rs`,
`src/sight.rs`, `src/kept.rs`, `src/main.rs`) owns the camera in the lamp's
head, finds a face in what it sees and tells the body where in the picture
that face is. The body (`lamp-body`, [body.md](body.md), "Looking at a face")
turns the head toward it while the lamp listens or speaks. Started with
`--sight-file` it also keeps its newest frame for the voice service to show
the model, so that the lamp can say what it sees.

Status on 2026-10-13: running on lamp-4ace since that morning. With a
photograph of a face on a screen in front of the lamp it reported the face in
66 of 66 looks, 53 to 55 ms a look. With people standing at the lamp for
seven minutes it reported a face in 580 of 2,732 looks, and the body turned
toward one as far as its yaw reach goes (250 counts) with the face still at
0.44 to 0.75 of the half-picture off centre: they stood further round than
the head turns. The picture for the model is described under "What the model
sees" with what has and has not been shown of it.

## What it does and does not do

- One process owns the camera. It opens it through `lamp-camera`'s Linux
  port, which checks the device against the USB identity given (vendor,
  product and port) and takes the camera's lock before the first frame:
  320 by 240 MJPG at ten frames a second.
- It looks at the newest frame at most every 250 ms while there is a face
  to follow and for four seconds after the last one, and every 600 ms with
  nobody there: decode the JPEG (`zune-jpeg`), put it into a 320 by 256
  picture, run the face detector, pick one face. It was every 150 ms
  always, and a look takes 52 to 55 ms of one core on lamp-4ace. Measured
  there on 2026-10-13: the processor read 63 to 64 degrees C with the eyes
  stopped and 70 to 75 with them looking six times a second, above the
  70 degrees at which its kernel's first thermal trip point sits. The voice
  service had ended once that afternoon on a late audio reference, in the
  same second as the SD card retuned itself; whether heat had a part in
  that is not known.
- With `--steady-rate on` (off unless given, so four a second stays the
  default and one install can show both), while one face has sat still
  for 3 s it looks every 500 ms instead (`src/steady.rs`, #705). Still
  means the one face near enough to follow (at least 0.15 of the height;
  with `--recogniser`, a known face, and the same name throughout) has
  kept its centre within 0.06 of a half-picture and its height within
  0.03 of where it settled. Those are the eyes' own numbers: a person
  sitting at the desk wanders 0.01 to 0.02 of the picture from look to
  look, `still.rs` holds a place to 0.03 of it, and the body turns for
  nothing under 0.15 across. With `--recogniser` (the product unit and
  lamp-4ace), someone who never asked to be remembered is never steady,
  and a missed print or a name flip starts the 3 s again.
- It goes back to every 250 ms when the face moves past those margins, a
  second face comes near, it is lost, looks stop coming for a second (a
  camera closed), another name is known, or a full look or a remember is
  under way. The change is seen at the first look after it, and while
  steady that look comes up to 500 ms after the last one, so a move, a
  second face or a face leaving is seen up to 250 ms later than before.
  Turning toward someone who moves starts up to 250 ms later too: the
  body waits 400 ms of stillness and then three sights, and from the
  second sight on they are 250 ms apart again. A face that settled
  within 0.06 of the body's 0.15 line (common, since a correction takes
  out 70 % of the offset and leaves a face at 0.09 to 0.15) can stay
  steady while it waits for a correction, and its three sights then
  take 1.5 s instead of 0.75 s.
- Downstream, the body is still sent every look and its gaze has no
  staleness for a face but `LOST_US`, 6 s without one. Two places come
  closer to their edge: the ring's eye (`crates/voice/src/ring_sight.rs`,
  `FRESH_MS` 700) gets looks 500 to 600 ms apart, so its margin before a
  look counts as stale falls from about 450 ms to 100 to 200 ms, and on a
  hot, busy board a late look sends the eye toward the middle for about
  100 ms; Expression's `faces_stale_us` 500000 (`deploy/*/expression*.json`)
  runs out near the end of each gap, which nothing reads today. The
  presence file (stale after 1.5 s), the bus (`LASTS`, 2 s) and the
  others that keep a face for 2 s or more are fresh at 500 ms.
- The cost, measured at four looks a second with someone at the lamp:
  on lamp-4ace 44 % of a core; on lamp-52e6 (1280x960, no
  `--recogniser`) 61 to 66 ms a look and 26 % of a core at 39 to 40 looks
  per 10 s. The half rate itself is not measured on any lamp yet.
  Recognition (one print every 2 s, over 100 ms on the lamp) does not
  halve, so the saving is less than half of those numbers.
- With `--look-on-change on` (off unless given; `src/change.rs`, #705, the
  lamp's heat), once no face has been found for 4 s, and no full look or
  remember is under way, a glance whose frame is like the last one the
  detector ran on is said to hold no face without running the detector;
  the detector still runs at least every 3 s, and at every glance until
  the picture has been still for two glances in a row. So someone who
  walks in and sits down is detected at each glance while they move and
  at the two after: a frame the detector misses (dim light, half turned,
  at the edge) costs one glance, not 3 s (the reviewer on #1441). It was
  3 s of glances after each change, which on lamp-4ace's empty office kept
  64 % of the glances detected as "settling" (#1460, 18 Oct, 11:22 to
  11:42). With nobody in view the
  glances cost lamp-4ace 104 to 130 ms of a core each, 16 or 17 every 10 s
  (02:00 to 05:00 on 18 Oct, 0 faces): about 20 % of one core all night,
  which a look every 3 s brings to about 4 % for a still room and a
  still head. The frame is compared as a
  32 by 24 grid of cells, each the mean grey of its pixels (ten by ten in
  the working frame), against the last frame the detector ran on, not the
  last one seen, so a slow change in one place adds up. Each cell's change
  is taken less its row's median change, so what the whole frame does
  (the camera's exposure, the room's light) and what a whole row does (a
  light's flicker, drawn by the rolling shutter as bands across) are not
  a change; and each cell learns its own noise from the empty room's
  glances, so a screen or a plant that flickers in one place stops
  counting once learned. A cell's noise falls about twenty glances back but
  rises five times slower, so a person moving in one place unseen (their
  back turned, pacing, fidgeting) is not taken for noise in a few
  seconds: their first eight seconds of glances are detected, and after
  that the 3 s look still finds them. The lamp check should try it (the
  reviewer on #1460). Three cells or more past 12
  greys and eight times their own noise are a change. Rising slower than
  it falls, the noise settles low, near 0.4 of a steady noise's spread:
  at four times it (as it was), about 48 of 768 cells pass on noise
  alone in a simulation of steady noise, which is consistent with
  lamp-4ace's empty office (33 cells a glance after eleven minutes, #1479,
  18 Oct) but not yet shown to be its cause: the 10 s line's new numbers
  will show it; at eight times, with rates of 0.01
  up and 0.05 down, under one does, a person moving in one place stays a
  change for fourteen glances, and a cell is deaf for about 26 glances
  after a learned flicker stops (the 3 s look still runs). A test with
  every cell moving by a spread of 8 greys a glance counts 99 of 100
  glances changed by the old rule and at most 5 by this one. The 10 s line
  says the learned noise's median and p90 over the cells, and how far a
  glance's cells move, to check it on the lamp. On lamp-4ace's empty
  office the plain rule, against the previous frame, saw 49 cells change
  a frame (#1441, 18 Oct, 10:00 to 10:20), and no glance was skipped;
  this is the rule that followed (#705). A sensor's noise averages out over a
  cell (no false change on a dark wall with ±20 greys a pixel, in the
  reviewer's probe); a face at 0.15 of the height (`sight::LEAST_SIZE`,
  the smallest the body turns to) changes six cells or more from 15 greys
  of contrast. A smaller or fainter face, such as a co-worker's at 0.08
  to 0.14 across the office, may change fewer and wait for the 3 s look. Someone coming into
  view changes the picture, so the glance after it runs the detector, as
  before; the head moving is a change too, and each change past the bar
  brings back detection for the changed glance and the two after: a head
  that idles every few seconds keeps more of today's rate. The whole room's light
  changing is not a change; the 3 s look still covers it. Nor is a change
  while the lamp's own light changes: at each glance the eyes read
  `light/state` from the bus, and a glance whose light differs from one of
  the two before (the change and the ring's fade) counts a change as the
  lamp's own (`OwnLight`): not detected, and its noise not learned. The
  first glance after is compared with the frame from before, so the new
  light counts once. On lamp-4ace the twenty minutes with the light scene
  and the battery changed far more cells than a quiet twenty (#1479,
  18 Oct), but the two ran different builds, so the light's share is for
  the next run to show. The lamp speaking does not count: people come
  over or turn to a lamp exactly when it talks (the reviewer on #1479).
  What it costs: someone who comes in during the light's two glances is
  seen at the first glance after. Any face found, even
  one too small to follow (but not one `--leave-still-faces` leaves out),
  keeps every glance detected for 4 s. A glance not detected tells the
  body, the bus and the known file what a look with no face tells them,
  as often as today. **The trade the switch makes:** someone in view but
  not found (their back turned, reading) who has kept still for two
  glances and
  then turns slowly to the camera, changing the picture less than the
  bar, is found at the next 3 s look, not the next 600 ms one; the lamp
  check should try it. Tests: `change.rs`'s
  `noise_is_no_change_and_a_face_is_and_the_frames_light_is_not`,
  `flicker_bands_exposure_and_a_screen_are_no_change_but_a_face_is`,
  `a_flicker_that_stops_does_not_leave_its_cells_deaf_for_long`,
  `a_person_moving_in_one_place_is_not_learned_as_noise_in_seconds`,
  `the_lamps_own_light_changing_is_not_a_change`,
  `an_empty_still_room_is_detected_every_three_seconds_and_a_change_at_once`,
  `a_walk_in_and_sit_down_is_detected_at_every_glance_until_still_for_two`,
  `without_the_switch_every_glance_is_detected` and
  `a_face_an_ask_or_a_remember_is_never_an_empty_room`. Raising the bar to
  40 cells, lowering it to 2 greys, waiting 6 s instead of 3, comparing
  with the last frame seen, or never detecting a changed frame each fails
  one of them. Not yet run
  on a lamp: the saving and a sit-down's time to a look are to be measured
  on lamp-4ace.
- With `--change-persist on` (only with `--look-on-change on`; off unless
  given; `src/change.rs`, `Rule::Persist2`) the change bar uses the
  "persist-2" rule: a cell counts only if it was past the bar (12 greys and
  eight times its noise, after the row's median change) in this compared
  glance and in the one compared before it, and the frame has changed when
  three or more such cells are; the first compared glance has none before
  it. Noise in a dark room lands in a few scattered cells for one glance
  at a time, so it stops running the detector; a person who walks in
  changes the same cells two glances running, so they are a change at the
  second glance: at most one glance (about 0.6 s with nobody in view)
  more before the first detection. The 3 s look, the two settling glances
  after a change, the lamp's own light and the busy room (a face under 4 s
  ago, a full look asked for, someone being remembered) are as without it,
  and without it the bar is exactly as before. Why: `--glance-log` on
  lamp-c0a4's dark empty office, 1027 glances replayed by
  `examples/glance_replay.rs` (the same definition as the code), skipped
  22.8% of the empty-room glances by the current rule and 64.2% by
  persist-2. Not yet run live on a lamp: a night's looks and a walk-in's
  time to a look are owed.
- With `--glance-log /var/lib/lampos-eyes/glance.log` (only with
  `--look-on-change on`; off unless given; `src/glance_log.rs`) each
  compared glance is written to that file as one JSON line of numbers, so the change bar's rules can be
  replayed offline: `at_us` (monotonic), `changed` (the active rule's
  verdict), `rule` (only with `--change-persist on`: `persist-2`; absent,
  the line is the current rule's, as before), `lit` (the lamp's own light changing), `cells` (cells past the
  bar), `mask` (the 768 cells past the bar by the current rule, one bit a
  cell in row order, as hex), `ratio` (each cell's change over its learned
  noise before this glance teaches it, times 8, floored, at most 255; 255
  while the noise is 0; hex, a byte a cell), `resid_greys_q` (each cell's
  change in greys after its row's median change is taken out, floored, at
  most 255; hex, a byte a cell), `frame_shift` (the frame's median change
  in greys), `detect` and `why` (`changed`, `settling`, `every`, `face`,
  `asked`, `remembering`, `lit` or `skipped`) and `faces` (how many faces the
  detector found that `--leave-still-faces` kept, so the faces the body
  was told of; 0 when the detector did not run). It holds no picture: no pixel and
  no cell's grey, only how much each cell changed after the frame's and
  its row's shift are taken out (test
  `a_line_per_compared_glance_with_no_grey_in_it`). It is still a
  timeline of when someone was at the lamp (`faces` and `at_us`, with each
  glance's change map), so it is kept in one place only: the path is
  pinned to `/var/lib/lampos-eyes/glance.log` (any other is refused at
  start), beside `seems.log`, and the reset plan
  (`reset-plan-product.json`) names it, so a reset wipes it. Each start
  makes it new: a log an earlier start left is removed (a link is removed,
  not followed), then the file is created 0600 with `O_EXCL` and
  `O_NOFOLLOW`, in a folder no one else may read (0700) (test
  `each_start_makes_the_log_new`); it stops 20 minutes after its first line or at
  50 MB, whichever comes first (a line is about 3.4 KB). `cargo run -q -p
  lamp-eyes --example glance_replay LOG...` scores each log's glances
  under the current rule, `persist-2` (a cell counts only if past the bar
  in this glance and the one before), `ratio-12` and `ratio-16` (12 or 16
  times the noise), and the noise learned again at other rates: the share
  of empty-room glances each would skip and, with a face in the log, how
  many looks after the first glance with a face it would have run the
  detector. A replay cannot know what a rule that detected other glances
  would have compared later glances to: the log's changes are against the
  frames the eyes really ran on. It is a test and measurement flag: no
  product unit passes it, and no file under `deploy/` may (test
  `nothing_deployed_passes_glance_log`); a builder adds it by hand for a
  measure, copies the log off and takes the flag away again.
- It sends one datagram per look to the body's gaze socket and nothing else
  anywhere:

  ```json
  {"face":{"x":0.05,"y":-0.43,"size":0.41,"turned":0.04}}
  {"face":null}
  ```

  `x` and `y` are the face's centre from the middle of the picture, -1 at the
  left or top edge to 1 at the right or bottom. `size` is its height as a
  share of the picture's. `turned` is how far its nose is from the point
  between its eyes, in eye distances: about 0 for a face turned to the
  camera.
- Without `--sight-file` no frame is written anywhere, kept between looks
  or sent anywhere. With it, the newest frame and no other is kept in that
  one file ("What the model sees", below). A line in the journal every ten
  seconds says how many frames came, how many were looked at, in how many
  there was a face and how long a look took, and ends `, steady` while it
  looks half as often (only with `--steady-rate on`). With
  `--look-on-change on` it also says how many of the looks were unchanged
  and not detected, and the detector's own time as "ms a detected look":
  a skipped glance costs only the JPEG decode and the grid (not measured
  on a lamp; the line's two numbers show it), so the line's "ms a look"
  is then per glance, far under a detection's cost, and the heat
  comparisons read "ms a detected look". With a sight file it also says
  the file's size a frame and what its check, write and rename each took.
  With `--full-look`, the "sight file" stage also holds the working
  frame's decode and re-encode at 320 across (`full_look::working`), which
  those three do not time. That was about 41 ms of every look on lamp-4ace
  (#1441), while the write itself is about 0.01 ms. **With
  `--sight-from-look on`** (with `--full-look` and `--sight-file`, off unless
  given), the working frame is encoded from the look's own picture, decoded
  and shrunk to 320 across for the detector already
  (`full_look::working_from_picture`). That's an encode alone: on an x86
  laptop, 0.72 ms against the shrink's 5.9 ms for a 1280 by 960 frame (Ten
  on #2602, 22 Oct 05:4x). It's the same file and size, and no reader
  changes. It's kept just after the decode, so a frame that doesn't decode
  leaves the one before it, and the stage still counts it as "sight file".
  The product passes `--full-look 1280x960` with `--sight-from-look on`
  since #2791's product half: on lamp-75c9 a look went from about 125 ms
  at 320 by 240 to 137.5 ms (an empty desk, 4 minutes each), for a
  "what do you see" still 1024 across instead of 320.
  It goes on to say why each
  glance was detected, or not because the lamp's own light changed
  (the picture changed, it was settling within two glances of
  a change, the 3 s look, the first glance, a face found under 4 s ago, a
  full look asked, or someone being remembered), the most and the mean
  cells changed against the bar of 3, over the empty-room glances it
  compared (and how many: none while a face keeps the room busy), the
  same glances counted the plain way and with only the rows taken out,
  and the frame's mean shift in greys, where a look's time went (the sight
  file, the decode, the grid, the detector and the rest), and how many
  looks found faces but none to follow. On lamp-4ace's first hour with the
  switch (18 Oct, 08:30 to 09:30) almost no glance was skipped and half a
  look was not the detector; these say which reason held and where the
  rest of the time goes.
- It never moves anything and decides nothing about the conversation. It
  recognises a face only for someone who asked the lamp to remember them
  ("Who is there", below), and nobody else.

## When the camera is open

Only while the voice service says so. `lamp-voice serve`, when
`LAMP_EYES_PERMIT_SOCKET` is set, sends `{"camera":true}` to that socket twice
a second while the microphone is permitted, and `{"camera":false}` once, at
once, when it is not (`crates/voice/src/body_link.rs`, `EyesLink`). The eyes
open the camera on the first yes and close it on a no or when no yes has come
for 1.5 s. A mute (the switch, the button or a head put to sleep) tells the
eyes no at once, as the voice's conversation ends and before anything is
written about it, and so does any other end of that conversation (a fault, a
stop): the camera closes on the mute, not 1.0 to 1.5 s after it (#1802). The
no is one datagram the voice never waits on, sent once; with no eyes
listening it is nothing. The journal marks it with `lamp-voice: the eyes are
told no at once: software mute` (or `head sleep`, `physical mute (privacy
event)`, `physical mute (privacy gate)`), and the eyes' own `lamp-eyes:
camera closed: not permitted` marks the camera shut. A voice service killed
outright says nothing, and the eyes close on the 1.5 s. The rule is the same as for its ears: the camera may look
exactly while the microphone may listen.

For the onboarding privacy step the voice may follow its no with
`{"close_receipt":N}`. Once the camera is shut, after the device's stop has
returned and never while it is open, the eyes say so on `eyes/camera_closed`
(`lamp_msgs::CameraClosed`, `{"close":N,"released":true|false}`, with
`LAMP_BUS=on`): `released` is whether that last stop (STREAMOFF, buffers
freed, file closed) returned without a fault; a camera not opened since the
eyes started is released. An ask that comes while the camera is open waits
for the next close. The eyes hold one ask at a time: a second ask before the
first is answered replaces it, and the first is never answered. Onboarding
then reads the first close as camera not proven when its 2 s wait ends,
which is the safe side. The publisher is kept alive on every turn of the
eyes' loop, camera open or shut (before, the looks kept only the faces'
alive, so with the camera open it lapsed), which is how the voice knows
these eyes give receipts
([onboarding.md](onboarding.md#physical-privacy-step-841)).

This is a narrower arrangement than the camera worker described in
[camera-runtime.md](camera-runtime.md), which takes its permission from the
interaction authority's own camera grant and hands frames to other
processes. The eyes use that crate's port for the device and nothing of its
frame link. Joining the two is still to do.

## What the model sees

People standing at lamp-4ace on 2026-10-13 asked it "What you see in front of
you?" within their first minutes, and it answered that it could not see.

With `--sight-file PATH`, each frame the eyes look at (about six a second)
also replaces the file at `PATH`: written beside it and renamed over it, so a
reader gets one whole frame or the one before, mode 0600. The camera's frames
are complete JPEGs of 12 to 17 KiB at 320 by 240 (checked on lamp-4ace: each
carries its own Huffman tables); a frame that is not whole, or is over
64 KiB, is not kept and the last good one stays. The file is removed when the
camera closes, for whatever reason, and when the program ends. On lamp-4ace
`PATH` is in the service's runtime directory, which is memory, not disk.
There is never more than that one frame, and no history.

The voice service reads it (`crates/voice/src/sight.rs`). When
`LAMP_EYES_SIGHT_FILE` is set in the service's environment, the provider
worker is started with `--eyes-sight PATH`, and while a person is speaking
it offers the file's frame to the model through `Supervisor::try_image`
([gemini-visual-input.md](gemini-visual-input.md)) as a person begins to
speak, at most once every thirty seconds. A file last written
more than a second ago is not offered, so closed eyes show the model nothing
rather than something stale. The offer has the limits that call already
has: only for the request whose input is open, 64 KiB, never sent again on a
later request or another connection. The worker's line in the journal says
for each look whether a picture went and how old it was, or why none did.

What a picture costs. The service counts a picture as tokens of the
conversation, and bills the whole conversation again with every turn. In the
first run on lamp-4ace the worker looked twice with every request, and over
a request with two pictures the prompt outside audio grew by 506 tokens. In
an office where the lamp hears something every few seconds that adds up
quickly, which is one reason looks are spaced out, and why
`"small_pictures": true` in the provider configuration asks the service to
read pictures at its low resolution (`mediaResolution` in the setup). What
that saves on this service is not measured yet.

The other reason is time. A picture in a question makes its answer later.
Measured on lamp-4ace on 2026-10-13 with `sight_cost.sh`: seven ordinary
played questions in each of six fresh sessions, three with the eyes shut
(no picture) and three with a picture in every question, in the order shut,
open, open, shut, shut, open. First sound came a median 1.32 s after the
end-of-speech decision without pictures (19 replies; the three sessions'
medians 1.29, 1.24 and 1.61 s) and 1.78 s with them (20 replies; 1.75, 1.86
and 1.75 s). So looks were moved from four seconds apart to thirty.

That did not bring the speed back, and the reason is worse than a picture
costing its own answer. In a fresh session, one question with a picture
(answered after 1.39 s) and then thirteen without any: medians of 1.73 and
1.83 s in two runs of the seven questions. Once a session's history holds a
picture, every later answer in that session is about as slow as if each
carried one. The first answer, the one the picture came with, is not.

So on lamp-4ace the model's sight is switched off again (13:38 on
2026-10-13): `LAMP_EYES_SIGHT_FILE` is commented out in the voice unit, the
eyes run without `--sight-file`, and the character file says it cannot see
yet. Afterwards the provider's first audio came a median 1.21 s after the
end-of-speech decision and the speaker's first write 0.21 s later, over
nine answers. The code stays. What it needs before it is turned on again
is a way to look only when looking was asked for, and a session without
the picture afterwards (a renewal carries the conversation across, see
[conversation-thread.md](conversation-thread.md)). Neither is built.

On lamp-4ace, 2026-10-13, first run: a picture of 13.7 to 17.7 KiB, 18 to
157 ms old, went with each of six requests. A played "Hey Lamp, what do you
see in front of you right now?" was answered "I see a desk including an iMac
and another display, along with a keyboard, mouse, and some personal items
like a mug and a picture frame", which is what a frame copied by hand a
minute earlier showed (and a man standing beside the desk, whom it did not
mention and was not asked about). That is one question, played. Not shown:
a person asking about themselves or something they hold up, how it does in
poor light, and whether seeing changes what it says when nobody asked.

What this changes about privacy, plainly: with the option on, a picture of
whoever is in front of the lamp goes to the provider with what they say, as
their voice already does; with `LAMP_VOICE_SECOND_CONFIG` set and Gemini
away, a look's picture goes to the second voice's `llm_base_url` instead. Nothing but the one newest frame is on the lamp.
The camera is still open only while the microphone is permitted. Without the
option, and without the environment variable, everything is as it was.

The character file has to say the lamp can see
([character.md](character.md)); without that the model has the picture and
still answers that it cannot.

### A full look (#836, off unless set)

At lamp-4ace on 2026-10-15 the owner held something up and asked "Hey Lamp,
what do you see?". The picture that went with the look was the working
frame, 320 by 240: the office, the person side-on, and nothing to tell what
was held up. The answer was wrong.

With `--full-look WIDTHxHEIGHT` and `--full-look-file PATH` (the two go
together, and the path is not the sight file's) the eyes' stream is
opened at that size instead of 320 by 240, and nothing else about the faces
changes: the detector shrinks every frame to its own 320 by 256, as it
always could, and every face size is a share of the picture. The working
frame is kept in `--sight-file` as before, at each look, 320 across: the
full-size frame is shrunk by averaging and encoded again (decoded a second
time after the detector's decode), so `lamp-policy --images` (#873) and
anything else reading the sight file still finds a picture under a second
old at any time; a stream fallen back to 320 by 240 is kept as it comes.
The still never goes there. `--full-look-file` holds only a still taken
when the voice service asks, `{"look_full":ID}` on `--permit-socket` (an id from 1),
or `{"look_full":ID,"side":PIXELS}` for a larger one (below; anything else in
the ask, or a side outside 320 to 4096, asks nothing):

1. The head is the body's. It already turns to the face the eyes see
   (`--gaze-socket`) and to a call (`--look-at-name on`); nothing new is
   asked of it. The still waits only while the head moves, as
   `body/joint_states` on the bus says (read twenty times a second):
   `base_yaw`, `head_pitch` or `head_roll` turning faster than 25 counts a
   second (two counts in the 80 ms over which the body works out a joint's
   speed), or asked to a place more than 64 counts from where it stands,
   with a reading older than 160 ms (two of the body's 80 ms periods)
   taken as moving. A head that has not moved since the ask and was not
   asked to is taken at once, with no fixed wait: the build before waited
   for 0.3 s of stillness on every look with a body (366 and 836 ms on
   lamp-52e6 with a turn, `verification/full-look-held-lamp-52e6-20261016`),
   and the first build also wanted each joint within 6 counts of its goal,
   so on lamp-4ace every still waited the whole 2 s (2,000 to 2,125 ms,
   #842). A head at rest stands short of its goal under its own weight
   (`head_roll` 7 counts on lamp-4ace,
   `verification/rest-glance-lamp-4ace-20261014/journal.txt`; `head_pitch`
   up to 36 counts on lamp-52e6,
   `verification/look-at-name-lamp-52e6-20261014`), and the body nudges its
   resting goals by up to 7 counts, all inside 64. Once the head has moved
   since the ask, a slow reading may be a pause inside the move (52e6 at
   18.88 s: base_yaw at -263 counts a second, then -25 and 0, then +225),
   so the still waits for 0.3 s with each head joint within 6 counts
   (0.0092 rad) of where it stopped, as it does when the body does not say
   how fast the joints turn. Replaying that 52e6 file (an ask every 10 ms,
   joints read at the ask and every 50 ms, a frame every 33 ms), a still
   taken while the head turned: 15 of 522 asks with the head turning at the
   ask (15 before), 174 of 531 with it moving within 300 ms (82 before), 0
   of 3,070 with it still (73 before), and ask to still 17 ms at rest (349
   before). A head that moves again before the
   frame comes is waited for again. It waits at most 2 s in all. With no
   joint states on the bus (no body, `LAMP_BUS` off on either side) it does
   not wait; a paused body's joints do not turn.

   With `--full-look-movement on` (off unless given) the head settles on
   movement only: no joint's distance to its goal counts, and what is
   waited for is the camera's aim turning faster than 25 counts a second
   about any axis: base yaw, head roll, and in the arm's plane head pitch
   plus shoulder less elbow (the three turn about parallel axes, the
   elbow's the other way round, `robots/lamp/urdf/lamp.urdf`). By speeds
   only; a reading without speeds watches the head's drift alone. An arm
   standing up raises the shoulder and the elbow together (lamp-52e6: both
   at about 150 counts a second, 551 and 474 counts in all, the aim 77
   counts off at the end) and carries the head up with its aim nearly held,
   so it is not waited for; the shoulder or the elbow alone, or one leading
   the other, is. Judged joint by joint, as #1251 first built it, the stand
   held the still to the 2 s bound: replayed on lamp-52e6's
   `look-at-name-lamp-52e6-20261014` (one ask every 10 ms, the joints read
   every 50 ms, a frame every 33 ms; `crates/eyes/tests/full_look_replay.rs`),
   waits at the bound were 14 of 4,113 off, 329 joint by joint and 36 by the
   aim. Of the 315 that only waited joint by joint, 307 were the stand and 8
   a shoulder easing back 25 counts at 38 to 100 counts a second; none was
   noise or a slow creep (a count of jitter reads 12 counts a second). On
   lamp-06c1 playing V1's roll the aim itself turns at 140 to 260 counts a
   second: 188 off, 451 joint by joint, 400 by the aim. The camera's rise as
   the arm carries it is not counted: at 0.24 m of arm and 160 counts a
   second it is under a pixel a frame at 1 m and 1024 across. The 0.3 s wait
   after a move, the 160 ms freshness and the 2 s bound are as above. Off,
   the head settles as described above and the arm is not read.
2. The still is the first frame after that. It is passed as it is when it is
   a whole JPEG of at most 64 KiB and 1024 across; otherwise it is decoded,
   shrunk to 1024 across by averaging, and encoded (`jpeg-encoder`, pure
   Rust) at quality 85, 75, 65 or 50, the first that fits 64 KiB less 64
   bytes, smaller again if none does. 64 KiB is the provider's image bound
   (`lamp_gemini::MAX_IMAGE_BYTES`). Each size is tried at quality 50 first,
   and one that does not fit even then is shrunk without the other three
   (the same still as before, in fewer encodes). An ask with a side
   (`full_look::Ask::bounds`) is made the same way to that side across, never
   enlarged, and to 184 KiB less 64 bytes (`full_look::LARGE_MOST_BYTES`);
   `--full-look-file` takes up to 184 KiB. Without a side nothing changes.
3. It is marked with the ask's id: a JPEG comment right after the start of
   the image, `lamp-look ID` (`full_look::tagged`), a number and nothing
   more. The voice service takes a still only with its own ask's id, so a
   still of an earlier ask that is written late, after a newer ask was
   sent, is never taken for the newer one.
4. It is written to `--full-look-file` (mode 0600, replaced whole as the
   sight file is), and removed 15 s later, or when the camera closes. 15 s
   is how long the voice service waits for a look to begin
   (`sight::LOOK_BEGINS_WITHIN`): the look goes once the lamp has finished
   saying it will look, and the still is made while it is saying so. The
   first build removed it after 3 s, and on lamp-4ace at 03:57 on
   2026-10-16 the first full-size still was made 2 s after the ask and the
   look went without it, its answer 8.52 s after the end of speech against
   4.63 s for the next look, which got its still (#842). One line in the journal: its size, its bytes,
   how long the head was waited for and how long it took to make. No
   picture is logged.

No still within 2.5 s of the ask (no frame, a frame that does not decode)
and there is none: the voice service's look then says the eye was shut
(`NO_LOOK_NOTE`), rather than guess from nothing.

The stream is not switched to a larger mode for the still and back. On UVC a
mode switch is a stop and a start, and lamp-52e6's camera often fails to
start again for 6 to 18 minutes (#763), which would leave the lamp unable to
see faces. One mode, opened once, costs a larger decode on every look
instead, and nothing at the moment of asking.

A camera that does not open is tried at the full size six times (thirty
seconds, one try every five), then at 320 by 240 and the full size in turn
(`full_look::full_next`): a moment's refusal still ends at the full size,
and one that lasts minutes (lamp-52e6 and lamp-4ace refused StreamOn for 6
to 18 minutes after a close, #763; lamp-4ace 169 times in 14 minutes on
2026-10-16) lets the eyes see at 320 by 240 as soon as the camera allows,
with the still then whatever that stream gives. A stream at 320 by 240 for
want of the full size is closed and tried at the full size once
(`full_look::Upgrade`) after it has run 10 minutes with nobody in view and no
look asked for, so nobody sees the eyes shut; if that does not open, it is
back at 320 by 240 by the turns above and the next try is 30 minutes later,
then every hour. One journal line for each try ("closing the camera to try
the full size again (try N)") and the usual "camera open, W by H" after it.
After six opens in a row that did not open, the tries come every 15 s
instead of every 5 (`reopen::retry_after`), said once ("the device seems
stuck"), and "camera open again after N opens in a row that did not" when it
opens: on lamp-52e6 on 2026-10-20 a mute was followed by opens refused at
`StreamOn`, `SetInterval`, `RequestBuffers` or `Controls` until a reboot, a
restart of lamp-eyes included (#1941, row 17). Each ioctl returned, only
late: lamp-eyes was not hung, the device was. A stuck camera's USB
device is reset (`lamp_camera::linux::reset_usb`, USBDEVFS_RESET), as an
unplug would: at the sixth failure, then at most once every ten minutes
(`reopen::reset_now`), never a reboot. It is found by `--usb`'s topology, or with the ids alone the port the camera was found on at that open (also when it has since lost its capture node, as a probe failing after a reset would leave it)
and reset only when its vendor and product are the expected ones; the
journal says "camera's USB device … reset" or why not, and health counts it
(`camera_resets`). The eyes' user needs write access to the camera's USB
node, which the lamp's udev rule gives `video`
(`robots/lamp/rootfs/etc/udev/rules.d/99-lamp-device.rules`). A lamp that
gets the rule by update has it only after its next boot or a replug of the
camera (an update reloads rules and triggers nothing,
[install.md](install.md)); until then each reset says `not reset: Io(13)`. A
reset is never done while seeing is not permitted (a mute or the switch
closed): one that falls due then waits for the first failure after seeing is
permitted again. Health counts the resets since the camera last opened.

**The camera's helper** (#2892, `--camera-helper`, `camera_helper.rs`). On
lamp-52e6 on 22 Oct (11:11, Sixth) a voice restart that dropped the camera's
permit left every open refused (`DeadlineExceeded` at `StreamOn` or
`Controls`) until the camera's USB device was **re-authorized** by hand, a
step stronger than the reset, and lamps whose udev rule predates the line
above (lamp-75c9's) can't reset it at all. So with `--camera-helper` (the
product's, `/run/lampos-eyes-camera/camera.sock`), after three opens in a
row that did not open (about 10 s at the 5 s retry) while seeing is
permitted, the eyes ask the camera's helper, on a thread of their own so the
loop never waits (at most 5 s for its answer), then at most once a minute
while they go on failing (`camera_helper::ask_now`), and say its answer once
("its helper asked to re-authorize its USB device: …"). The reset above runs
as before.
- **The helper** is `lamp-eyes reauthorize-camera --usb
  VENDOR:PRODUCT[@TOPOLOGY]`, root's, started by `lamp-eyes-camera.socket`
  (`deploy/product/`, root and the group `lampos` may connect) for each ask
  as `lamp-eyes-camera@.service`, with the same camera as lamp-eyes' own
  unit (`01da:5875`, held by the install test). With a port
  named it acts at that port only, never at a camera with the same ids
  elsewhere, and checks that the device there under `/sys/bus/usb/devices`
  has that `idVendor` and `idProduct`. With none (#4074: the camera is on
  `3-1.1.4` on lamp-4ace and lamp-52e6, `3-1.3.4` on lamp-a63e and
  lamp-dbda), it finds the one USB device with those ids on any port,
  looking only at devices' own entries (not a root hub's `usbN`, not an
  interface's `3-1.1.4:1.0`), and touches none when there is none or two
  (`camera_helper::reauthorize_any`), the eyes' own rule. Then it writes
  its `authorized` 0, then 1 half a second later, as an unplug and a plug
  would, and answers one line: `{"reauthorized":true}`, or `false` with why
  (`too_soon` with `ago_s`, `no_device`, `not_the_camera`, `several`,
  `error`). The product's helper unit names `01da:5875` with no port, as
  the eyes do: every channel carries a helper that takes the ids alone since
  stable 8250 (#4091), so a lamp whose camera is on another port is
  re-authorized too.
- **Narrow:** it reads nothing from whoever connects, so it can't be pointed
  at another device; a port that isn't digits, `-` and `.` makes no path; at
  most once a minute by the time it keeps in `/run/lampos-eyes-camera/last`,
  written before the device is touched; no capabilities, no network, nothing
  else writable but that folder and sysfs.

The size is at most 3264 by 2448, the camera runtime's bound
([camera-runtime.md](camera-runtime.md)); larger is refused at start. On
lamp-52e6 on 2026-10-15 the first build asked for 1280 by 960 against the
runtime's old bound of 1280 by 720: every open at the full size was refused
before the device was touched ("camera did not open: InvalidData None"),
and the eyes ran at 320 by 240. With the height raised, the second run was
refused at SetFormat: the camera sizes a 1280 by 960 MJPG buffer at
2,457,600 bytes (two a pixel; `v4l2-ctl` on lamp-52e6), over the runtime's
2 MiB frame bound, then 4 MiB. The eyes' frame buffer is that size. Since
#941 the bound is the camera's largest listed mode, 3264 by 2448, and the
frame bound 16 MiB (two bytes a pixel is 15,980,544 there; the frame buffer
is allocated zeroed, and only what a frame uses is touched).

Measured on a laptop (Apple M-series, release build), not yet on a lamp: a
1280 by 960 synthetic frame of 548 KB made into a 768 by 576 still of
58.5 KB in 38 to 65 ms. The camera's own frames compress better than that
fixture, so a real frame should come out larger. The lamp's ARM cores are
several times slower; the eyes' journal line gives the real time. What the
larger decode costs on every look, in time and heat, is to be measured on
the lamp before the switch is left on (the eyes' ten-second line gives the
milliseconds a look): at 320 by 240 a look takes 64 to 81 ms on lamp-52e6
today, and a frame of 1280 by 960 has sixteen times the pixels to decode.
That time is how quickly the head follows a face and what is left for the
voice. Compare it, and the eyes' processor time, at both sizes on lamp-4ace
and lamp-52e6 and write the numbers here; if the full size is too slow,
640x480 is the knob.

On the voice service's side, `LAMP_LOOK_FULL=on` ([speaking-first.md](speaking-first.md),
"Taking a look"). The voice asks for a still when the lamp says it will
look, or when a skill's tool that looks is called (#941), whatever the
model says then. The two are turned on together: eyes with `--full-look`
and a voice service without it look with the working frame, as before. Privacy is as before: the still goes with the one look it
was taken for, is on the lamp for at most 15 s, in memory, and is never kept
or logged.

#### A larger still for writing on paper (#941)

Desk tutor reads handwriting, which a 1024-across still of a page does not
keep. The ask names the still's longer side, `"side":3264` for the whole
frame of a camera run at `--full-look 3264x2448`, and the still is made to
184 KiB instead of 64. Why 184 KiB: the provider worker sends a still as
base64 in one websocket message, and `lamp_gemini` bounds every message at
`MAX_WIRE_BYTES`, 256 KiB (its write buffer `MAX_WIRE_BYTES` + 4 KiB, so a
larger message fails to send). 184 KiB is 245 KiB as base64, which leaves
room for the look's note (`MAX_NOTE_BYTES`, 1 KiB) and the JSON. Google's
own request bound is far above that; the 256 KiB message bound is the real
one. The voice takes it: a full look's still is read up to 184 KiB
(`sight::fresh_up_to`) and goes in the look's written turn up to
`lamp_gemini::MAX_LOOK_IMAGE_BYTES`, 184 KiB, the turn refused if it is not
one message under `MAX_WIRE_BYTES`. Pictures with speech (`try_image`) stay
at 64 KiB. The voice asks for a side with `LAMP_LOOK_FULL_SIDE=N` for every
look, or for one look (`sight::Look::side_next`, a skill tool's
`look = { side = N }` once #984 carries it; [speaking-first.md](speaking-first.md)).
What the model makes of it is set by its own media resolution:
`small_pictures` (`MEDIA_RESOLUTION_LOW`) would undo the detail.

The camera runs at the large size all the time rather than switching for
the one look. A switch is a stop (`DRIVER_TEARDOWN_BUDGET_US`, 250 ms
bound, #921) and a start (about 50 ms on lamp-52e6, past 100 ms for minutes
after some closes, #911, up to `OPEN_BUDGET_US` 2 s), then the first frames
while exposure settles, and the same again to go back: from about 0.3 s to
past 4 s added to a look, and on lamp-52e6 and lamp-4ace a start after a
close is refused for 6 to 18 minutes now and then (#763), which would leave
someone sitting at the desk with the lamp unable to see them. One mode costs
the decode instead, on every look. Measured on a laptop (Apple M-series,
release build, `full_look::tests::the_camera_s_largest_frame_fits_the_larger_bound`),
a synthetic 3264 by 2448 frame of 3.27 MB, noisier than a page of paper:
the detector's decode 37 to 50 ms (one run 227 ms), the working frame 41 to
62 ms, and a still of 1377 by 1033 and 186,605 bytes in 222 to 509 ms (500 ms
before the quality-50-first order). That is six and a half times 1280 by
960's pixels, and a look decodes the frame twice (detector and working frame); at
320 by 240 a look takes 64 to 81 ms on lamp-52e6, so expect several hundred
ms a look on the lamp, fewer looks a second while a face is followed, and
more heat. Still to be measured on lamp-52e6: the rate the camera gives at
3264x2448 (the eyes ask for 10 a second), a look's ms and the eyes'
processor time at 1280x960 and 3264x2448, and how long the still takes. A
still that takes longer than the voice service's `STILL_WAIT` (2.6 s from
the ask, with up to 2 s of it settling) goes unsent. If the large size is
too slow, 2592x1944 or 2048x1536 are the knob.

## Which face

The detector is YuNet (`face_detection_yunet_2023mar.onnx`, 232,589 bytes,
SHA-256 `8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4`),
the model the old runtime used, run with `tract-onnx` as the endpoint worker
runs its model. The file is checked against that size and digest before it is
parsed; it is not in this repository (`hal/drivers/tracking/models/` in the
old runtime's tree has it).

A detection counts as a face from a score of 0.6. Boxes that overlap by more
than 0.3 are one face; a box that is not a number or lies off the picture is
dropped. Of the faces left, the largest whose height is 15% of the
picture's or more is the one reported: somebody at the lamp, not somebody
across the room. With several people near, that is whoever is nearest the
camera, and it can change from look to look.

### A face at a place where no person has been seen (#763)

With `--leave-still-faces on` (off unless given; `src/still.rs`), a face
at a place where only small, faint faces have ever been seen is not a
person. lamp-4ace's eyes reported a face all night in an empty office (14
to 15 October, 01:00 to 05:59): 55 of 55 detections at one spot, about 0.7
across and 0.6 down, 0.09 to 0.12 of the picture's height, at 0.60 to
0.79. Its box is not still: it wanders 0.01 to 0.02 of the picture from
look to look, as much as a person sitting at the desk does, so a stillness
rule cannot tell them apart. Replayed through the day's recording, a box
held within two pixels for two minutes left out 4 of the 55, and a centre
held within 0.02 for five minutes left out over a third of the morning's
real faces. What tells them apart is that the spot never gave a face 0.14
of the height or taller, nor one scored 0.8 or more, in a day, while the
people at the desk did within seconds and the people at the picture's edge,
small too, scored up to 0.90.

So each place a face is seen is remembered: its centre and height, a face
being at it when both coordinates of its centre and its height are within
0.03 of the place's; with how many faces came from it, since when, and
whether any of them was a person's (0.14 of the height or more, or scored
0.8 or more). A face at a place that has given faces for half an hour or
more, ten or more of them, none of them a person's, is left out before
anything else uses the look: the body is not told of it, it is not on the
bus, and it is not recognised. A place nothing has been seen at for two
hours is forgotten (the night's face came back after at most 52 minutes);
at most 32 places are remembered, the one seen longest ago going first. The
journal's ten-second line says how many faces were left out so far.

Replayed in order through the whole recording (14 Oct 18:23 to 15 Oct
13:17, `docs/verification/still-face-lamp-4ace-20261014/`, a host test):
the night's 55 faces are all left out; at the false spot the rest of the
time 448 of 540 are (the first half hour after it is forgotten passes);
none of the 390 faces of the people at the edge in the evening, nor of the
10,853 at the desk in the morning, is.

Raising the detector's least score (`LEAST_SCORE`, 0.6) instead would cost
real faces before it removed the false one: at 0.75 it drops 574 of the
595 false-spot faces but also 105 of the 390 at the edge and 1,526 of the
10,853 at the desk; at 0.8, all 595 false ones, 160 at the edge and 2,398
at the desk.

Not shown yet: the day's recording replayed through it, with the counts
before and after, and whether raising the 0.6 score would do the same.

## Who is there

Since 2026-10-15 (#759; `docs/brain-build.md` step 4). "Lamp, remember my
face. I'm Minh." Later, sitting down: "Hey Minh." Until someone asks,
nobody is recognised: `docs/memory.md` decided that everything is "said in
this room" until someone asks to be recognised.

- **The print.** OpenCV's SFace recogniser
  (`face_recognition_sface_2021dec.onnx`, Apache-2.0, 38,696,353 bytes,
  SHA-256 `0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79`,
  fetched separately and checked before it is parsed) runs on tract as
  YuNet does. The near face is cut out upright from YuNet's five marks onto
  OpenCV's 112 by 112 reference (`who::align`). It gives 128 numbers of
  length one, and two prints of one person have a cosine of 0.363 or more
  (OpenCV's own threshold for this model). One print takes 152 ms on
  lamp-4ace's processor (236 ms the first time, 320 ms to load and
  optimise the model; 68 ms on the development Mac), and the 128 numbers
  were the same to the last digit on both. At one print every two seconds
  while someone is at the lamp that is about 8 % of one core.
- **When.** Only for the largest face that is at the lamp: at least a tenth
  of the picture's height (people at lamp-4ace's desk were 0.12 to 0.27).
  At most every two seconds, or at every look while someone is being
  remembered, and at the first look after someone is remembered at the
  lamp, so the known file names them within one look, not two seconds
  later (#3108; `who::recognise_due`). A face from a photo is not: it is
  named only on its person's yes. The camera gives 320 by 240, so a face at the desk is 35 to
  65 pixels tall. Whether that is enough is to be measured with people
  (the camera can give 640 by 480 if not). With `LAMP_FACE_FULL_CROP=on`
  the face is cut from the frame at its own size instead
  ([below](#held-through-a-miss-lamp_face_full_cropon-3126)).
- **The score, in the journal.** Each check with a face in view, outside a
  remembering, says one line, at most every two seconds:
  `recognition best 0.341 height 0.152 (near at 0.1, same at 0.363)`, the
  best cosine against anyone's kept prints (`none` with nobody remembered
  or no face near enough), the largest face's height as a share of the
  picture's, the near gate and the threshold. No name, no id: only numbers,
  always on (#3126).
- **Kept.** `--people FILE` (on lamp-4ace `/var/lib/lampos-eyes/people.json`,
  in a directory of mode 0700 and a file of mode 0600). Per person: the
  name, up to five prints and when they were added; with
  `LAMP_MEMORY_STABLE_ID=on`, their memory `id` too (#2176), kept through a
  rename, `person_id` of the name for anyone remembered before it, never
  two people's (ids already held are kept first, then the rest given one;
  the voice follows the eyes' switch) ([memory.md](memory.md#each-persons-own-764)). At most 20 people. No
  picture is kept, and no print leaves the lamp. With `LAMP_ACTIVITY=on`
  the sitter's prints (up to six, for telling the sitter from others) are
  held in memory for the sitting only, never in this file
  ([below](#what-the-person-at-the-desk-is-doing-lamp_activityon-1678)). **Read one release
  forward** (#1505, harm 3): a field a later build added, on the file or on
  a person, is read past, and the next save writes this build's fields.
  What this build cannot read is **set aside, never dropped or guessed
  at**, in `set-aside/` beside the file (a folder of mode 0700, files of
  mode 0600): a person it cannot read (a field of the wrong type, a name
  or prints it does not keep, a second of one name, a twenty-first) is
  written there and the rest saved back without them; a file it cannot read
  at all (too large, not JSON, a `version` above 1, no list of people) is
  moved there whole, and the eyes start knowing nobody. One journal line says each, with a
  count and never a name. While anything is there, the eyes' health is
  `Warn`, "the eyes set aside N people's records they couldn't read"
  (counted once a minute), so a person the lamp no longer knows is told
  why. The product reset empties `set-aside/` with the file: these are face
  prints. Only a failure to set aside, or to save back the rest, stops the
  eyes. A set-aside file is named by a hash of its bytes, so the same
  record set aside again after a crash is kept once; an existing
  `set-aside` is set to 0700 and a link there is refused; a file moved there
  is linked and then unlinked, never replacing one, and set to 0600.
- **The rule for the next field** (the reviewer on #1558, as #1512's for
  tasks): an optional field that only adds to what is known may come
  without a bump. A field that narrows what the lamp may do with a face (an
  expiry, "don't greet me by name"), or any new required field, bumps the
  file's `version` in the same change, so an older build sets the file
  aside whole instead of recognising more widely and then dropping the
  field at its next save.
- **Asked.** One line of JSON on `--who-socket` (on lamp-4ace
  `/run/lampos-eyes/who.sock`) and one line back:
  - `{"remember":"Minh"}` takes five prints over the next looks while that
    one face is at the lamp and answers `{"remembered":"Minh"}`. With no face
    in view at all it answers `no_face` after 1.5 s. With a face too far
    away, or two at the lamp, it gives them four seconds and then answers
    `too_far` or `two_faces`.
  - `{"forget":"Minh"}` answers `{"forgot":true}` once the file no longer
    has it, nor any set-aside file with a list of people (#1505: each is
    rewritten 0600, or removed when nothing is left in it). A set-aside
    file that cannot be searched might hold the face: one that is no JSON,
    one with something in it but no list of people (a newer build's
    shape), or one with a person whose `name` is no plain string. JSON that
    holds nothing (`{}`, `[]`, a number, `null`) holds nobody and counts as
    searched (#2850: a 2-byte `{}` set aside on lamp-c0a4 kept every forget
    waiting). **Then the
    forget waits for it** (#1586): the answer is `{"forgot":…,
    "unsearched":N}`, never "not known" and never a fault.
  - **The memory keys, with the answer** (#2176, #2330). With
    `LAMP_MEMORY_STABLE_ID=on` the answer says the person's memory `id`.
    Every answer says `name_id_free`: whether `person_id` of the name
    (the voice's key without the stable id) is nobody else's once they are
    forgotten. It is `false` only with the switch on and another record,
    someone renamed from that name who kept its id, holding it ("Lan"
    renamed "Mai" keeps `p-` of "Lan"; a later "Lan" forgotten is then not
    free). Off, always `true` (`People::name_id_free`). The app's remove
    forgets `person_id` of the name only when it is free (cloud.md).
    - **What's kept:** a hash of the name, which a guess can undo, with when
      it was asked, in `set-aside/forgets-pending` (0600), until the forget
      finishes or a factory reset. The hash is a plain SHA-256 of its lower
      case: the lamp has to match the name later, so for anyone with the
      lamp's disk it is the name, lightly hidden, beside set-aside files
      that hold names in plain text anyway (the reviewer and Twelve on
      #1767). A file from a later build is read past what it doesn't know.
      One that can't be read at all is set aside beside it as it was
      (`forgets-pending.unreadable-<unix s>`), never counted as anyone's
      records, with a line that counts only, and forgetting goes on (#1680,
      #2052's audit): before, every `forget` failed while it couldn't be
      read. At each start a set-aside list that can be read now (the build
      that wrote it, back after a step back) has its waits merged back, the
      later word kept for a name in both, and the copy goes, so a face read
      back from a set-aside file is still forgotten (the reviewer on #2070).
    - **Finishing it:** at each start and each forget, a face of a waiting
      name **kept before the forget was asked** is dropped: read back into
      the people file, or in a set-aside file that can be searched now
      (one that says no time it was kept counts as before). Someone
      remembered after they asked is kept, since their later word stands
      (Twelve's P2 on #1767). Once nothing is left that cannot be searched,
      the forgets are done and the file goes. The times are the wall
      clock's: a lamp whose clock went back after a forget (a start before
      the time is synced) would date a later "remember me" before it and
      drop it once more. That's rare, and it errs toward forgetting (the
      reviewer on #1767).
    - It's never counted or searched as anyone's records, and the reset
      empties it with the folder. While anything waits, each forget reads
      every set-aside file again: a few files of at most 256 KiB today,
      worth a look if set-aside grows. A rewrite's temporary file a
    crash left there (`….json.new`) is neither searched nor counted, and a
    `set-aside` that is a link is never followed (`forget` refuses with
    `{"error":"storage"}`, health counts it as one).
  - `{"list":true}` answers the names (`people`), and never a face from a
    photo still waiting for its yes. `{"list":true,"waiting":true}`, which
    only lamp-cloud asks for the owner's own page (`face.list`, #2192),
    names those too (`waiting`). They are never said at the lamp, counted
    or greeted.
  - A forget also lets go a face from a photo by that name, and one let
    go is a face forgotten (`forgot: true`). A let-go its file couldn't be
    written for answers `{"error":"storage"}`, as a failed forget does.
  - **A forget is the face only, the eyes' side.** The eyes forget the
    face alone. "Forget my face" at the lamp leaves what memory keeps for
    that person, unless they also say "forget everything", or
    `LAMP_MEMORY_FORGET_ALL=on` (#2850): then, once the eyes forgot the face
    of the known person whose line it is, the voice empties their memory as
    their `forget_everything` would (their scope only, never the room's;
    memory.md). The voice asks the eyes for that person under the name the
    eyes keep, read from the known file, never the model's guess, and never
    for anyone else's face (voice-side, `known_faces::forget_name`). A
    room's line (no known person's) sends a forget only when it is that
    person's own: the eyes judged speaking through it (`LAMP_EYES_SPEAKING=on`,
    which the voice needs in its own unit too: without it every room's
    forget is refused; the product's `lamp-voice.service` pins it, #3038),
    the known file, fresh when the line was heard, named the person the
    model asked for, and their face (`known-face`, beside it) was the only
    one seen saying it. The ids are compared with the eyes' boot: the
    speaking faces' with the boot their frames came under, and
    `known-face`'s, which holds the bare id, with the newest boot the voice
    read a frame of strictly before the file was written, with no tolerance:
    after the eyes start again a still-fresh file, or one written at the new
    boot's first frame, names none of the new faces, and in doubt the forget
    is refused (#3037, Fourth's P2 on #3038). The eyes' own `known-speaker`
    file (Fourth's) is to replace this binding. The second route only: Live never records who was seen saying
    a line, so there a room's forget is always refused. A visitor or a screen saying it while they sit
    nearest, two faces speaking, no face read speaking, a line not judged,
    or a known file stale when it was heard sends nothing, and the model
    hears that only the asker's face, seen at the lamp, can be forgotten
    (the first builder on #2602, El's P2 and P3 on #3018). The words are
    judged at the line's end, so someone who sat down during a slow turn
    did not say it. Speaking is read from a face's mouth, not its
    liveness: a photo or a screen showing their face is not told from them.
    A remove from the owner's page (`face.remove`) takes their
    memory too (cloud.md, "The faces it knows"). Memory names a person by
    their name's hash, so until person ids are stable (#2176 row 3), a new
    face kept under the same name would be given those memories.
  - `{"who":true}` answers who is at the lamp now, or `null`.
  - Remembering someone already known replaces their prints.
- **Said.** With `--known-file` (on lamp-4ace `/run/lampos-eyes/known`,
  beside the socket), the name of the known person at the lamp is kept
  there and renewed every two seconds while they are there. The file is
  removed when nobody is near (with `LAMP_FACE_FULL_CROP=on`, once the hold
  is over, [below](#held-through-a-miss-lamp_face_full_cropon-3126)). With `LAMP_MEMORY_STABLE_ID=on` the
  person's memory id is on the line after the name (`NAME\nID\n`,
  `lamp_msgs::known`); off, the name alone, as before. Every reader (the
  voice's `known_faces`, the skills' `known::name_in`) reads the first
  line as the name and passes over an id on the second; any other second
  line is no name.
- **Their face.** With `LAMP_EYES_SPEAKING=on` and `LAMP_BUS=on` too,
  `known-face` beside the known file (mode 0600) holds the passing id
  their face has on `humans/faces/tracked` (`f3`), written just after the
  name and removed with it (`who::keep_known`). Only that id: no picture,
  print or name. It is what lets the voice tell whether the face seen
  speaking is the known person's ([memory.md](memory.md#how-the-voice-uses-it)).
  The voice reads it as it reads `known`, so whatever access a deploy gives
  the voice to `known` it needs to `known-face` too (the voice runs as root
  on lamp-4ace and in the product unit).
- **Named in Who was here, only with their yes** (#1997, the lead's yes on
  20 Oct; nothing asks it until the voice's question is built):
  - `{"visits_yes":NAME}` keeps a remembered person's yes to being named to
    others: `tell_visits` on their record, absent for no, an optional field
    that an older build drops (which only narrows). It's taken only from
    the known face at the lamp now (`photo::is_here`'s rule, `not_here`
    otherwise), so nothing elsewhere can say yes for someone.
  - `{"visits_no":NAME}` takes it back, at any time, since a no only narrows.
  - Answers: `{"visits":"yes"|"no"}`, or `not_known`, `tellable_off` (no
    `--known-file`) or `storage`. The journal says yes or no, never the name.
  - With `--known-file`, `tellable` beside it (mode 0600) holds the names of
    those who said yes, one a line, rewritten at start and after every
    request on the socket. So a no, a forget or a rename is there at once.
    There's no file when nobody said yes. A new remembering, even under a
    name that had a yes, starts without one: the prints are whoever is at
    the lamp then, so the voice asks again. The reset wipes their record,
    and `tellable` lives in `/run`.
  - lamp-skills reads `tellable`, read-only, for Who was here's names
    ([skills.md](skills.md)).
- **The voice** (`crates/voice/src/known_faces.rs`, when
  `LAMP_EYES_WHO_SOCKET` names the socket):
  - The model has two functions, `remember_my_face` and `forget_my_face`,
    each with the name. They are called only when the person asks.
  - With `LAMP_WHO_WAS_HERE_NAMES=on` (the worker's `--who-visits`, after
    `--names-checked`; #1997), two more: `tell_others_i_was_here` and
    `dont_tell_others_i_was_here`, each with the name, sent to the eyes as
    `visits_yes` and `visits_no`. Off, neither is offered.
    - Someone just remembered is asked once whether the lamp may tell
      anyone who asks when they've been at the desk, by name
      (`known_faces::ASK_TO_NAME`); they can also say so, or take it back,
      in their own words later.
    - **A yes only from the named person's own scope** (Third on #1997).
      It's taken only when the request's admitted scope
      (`memory_scope::ScopeLease`) is that person's, and not narrowed to the
      room by the speaking face. With the room's scope, someone else's, or
      none, it's refused with nothing sent, and the model is told not to ask
      again then (`known_faces::NOT_SURE_WHOSE_YES`). The eyes' own check,
      the face at the lamp now, stays under it.
    - **Its limit:** the scope says whose face was in view as the line
      began, not who spoke. Someone off camera saying "yes, you can tell
      people when they've been here" for the person in view is taken. Only
      a face seen speaking that isn't the known person's narrows the line
      to the room (`LAMP_SECOND_SPEAKING_FACE=on`, the second route, off by
      default). The person in view hears the lamp confirm, and can say no.
    - **Only with the memory** (`LAMP_MEMORY_SOCKET`): the scope is a known
      person's only through the memory's face cache, so without it neither
      function is offered and nobody is asked.
    - **A no from anyone at the lamp,** since it only narrows.
    - Like the face functions, neither is run for a text or a line the
      lamp wrote. The journal names nobody: "someone the lamp knows said
      yes to being named in Who was here", or why a yes wasn't taken.
  - A forget that waits for set-aside files (#1586) is told as what it is,
    never "My eye could not do that just now": forgotten, or not known by
    that name, and that a few old face records the lamp couldn't read are
    kept apart, forgotten in too if they ever come back, and cleared by a
    factory reset (`known_faces::FORGOTTEN_SOME_KEPT_APART`,
    `NOT_KNOWN_SOME_KEPT_APART`).
  - The hello for someone known is by name. With no known face, it is by
    the name typed at setup, when `LAMP_PERSON_NAME` gives one
    ([speaking-first.md](speaking-first.md), #806).
  - What a known person says is told their name once a session ("The person
    at the lamp now is Minh: you know their face.").
  - With a memory, what a known person asks it to keep and what they say
    goes to their own scope, and a recall asks theirs first, then the
    room's (#764, [memory.md](memory.md#each-persons-own-764)). Forgetting
    a face does not forget what that person said.
  - With a memory, each provider session is one person's: when the known
    person changes (someone else known after 10 s with the one before
    unseen, or at once when the one before was last seen 30 s ago; nobody
    after 30 s unseen and two minutes with nothing said), a new
    session is opened between turns and told only their thread and kept
    things ([memory.md](memory.md#a-session-for-each-person-764-follow-up)).

On lamp-4ace on 2026-10-15 at 08:22, with the office empty, the socket
answered `list` and `who` at once, `remember` with `no_face` after 4.0 s,
and `forget` of an unknown name with `false`. By voice at 08:33, "Hey Lamp,
can you remember my face? I'm Sam." (played, nobody in view) was answered
"I couldn't quite see you. Would you mind looking right at me and asking
again?", the eyes having looked for 4.2 s. Since 08:39 the eyes say no face
after 1.5 s: the same request was answered 3.5 s after the end of speech.

### Held through a miss (`LAMP_FACE_FULL_CROP=on`, #3126)

Off unless set. On lamp-4ace on 23 Oct a face just remembered was not kept
known: with `--full-look 1280x960` the picture is the frame shrunk four
times by taking every fourth pixel, so a face at the desk is 29 to 65
pixels stretched to 112, the same person scored either side of 0.363 look
to look, and each miss took their name and the known file away (Sixth's
analysis on #3126). With the switch on:

- **The face from the full frame.** The look keeps its decoded frame (the
  decode's own buffer, for that look only; nothing written) and the near
  face's five marks, found in the shrunk picture, are carried back to it;
  only the 112 by 112 face is read from it (`who::align_full`). Both
  remembering and recognising cut it this way, so prints and the faces
  matched against them come the same way: faces remembered before the
  switch are best remembered again. The sitter's prints
  (`LAMP_ACTIVITY=on`), the expression read and photos are cut as before.
- **Held through a miss.** A check that finds nobody (no match, no face
  from a photo, or no face near enough) keeps the one known, their name in
  `who` and the known file, if they matched within the last 4 s
  (`who::KNOWN_HOLD`, `who::Held`): one missed check, two seconds on, keeps
  them, and the next lets them go, so someone who leaves is let go within
  4 s of their last match. A held check does not count a visit. Someone
  else matched, or a face from a photo, replaces them at once. An unknown
  face ends the hold too: a near face that printed and matched nobody keeps
  the held name only if it is still at least 0.30 alike to that person's
  own prints (`who::HOLD_LIKENESS`, under `SAME_PERSON`'s 0.363: their
  flicker sat at 0.34 to 0.36, strangers lower; to be tuned from the score
  line) and, while speaking is judged, has the passing id their face had
  at the match (`Held::keeps_print`). Anything else lets them go at once,
  the known file with them, so a stranger sitting down is never named.
  A near face seen but not printed (turned, too small) keeps the held name
  only with the passing id their face had at the match
  (`Held::keeps_unprinted`); with no near face the hold runs to its 4 s.
  With `LAMP_EYES_SPEAKING` off there is no passing id: a printed face
  relies on the 0.30 bar alone, and an unprinted near face lets go at once.
- **The camera closed lets them go at once**: the name and the known file
  go as it closes, held or not.

Off, nothing changes: each miss lets go, as before. The lamp check: on
lamp-52e6, the lead's face from a seat with the switch on and off, the
score in the journal, the known file held and `who` naming them.

### Atomic face repair store (inactive)

`who::People::repair_target` and `replace_prints` are public Rust store APIs
with no runtime caller, RPC, or activated switch. The inert controller core
below prepares a work item; no service invokes either component yet.
Ordinary `remember` and `remember_one` behavior stays unchanged. The opaque,
non-cloneable target holds the whole existing record; taking it neither
removes that record nor frees its name. One replacement attempt consumes the
target. It must still belong to the same live `People` instance, with the same record and no
intervening committed save. This requires an exclusive process-local writer;
it is not a lock or transaction shared by separately opened stores.

Repair writes use the one exact sibling `<people>.repair.new`, opened with
`create_new` and mode 0600; an occupied path refuses the attempt without
overwriting it. The file's mode must still be exactly 0600 just before
rename. At `People::open`, the exclusive writer unlinks only `<people>.new`
and `<people>.repair.new` before reading the committed file. These are
uncommitted ordinary-save and repair leftovers, even if they contain a full
JSON record. Cleanup does not follow a temporary symlink and does not remove
a directory in its place; any error except an absent file refuses startup.
The current product, example and lamp-06c1 reset plans list both exact
siblings alongside `people.json`. No glob or wider directory wipe is added.

Before runtime repair activation, the forget path also shares this exact
cleanup: `People::forget_at` removes both unfinished siblings before changing
the canonical records or RAM. It does not wait for a later `People::open`.
An unlink failure returns an error instead of reporting a completed forget
while a temporary could still hold every person. Temporary symlink entries
are unlinked without following them; unexpected directories and neighboring
files are not swept. Existing set-aside and pending-forget handling follows
only after cleanup succeeds. This is file removal, not secure-erasure or
power-loss proof, and adds no RPC or runtime repair activation.

A successful replacement changes only the face prints and clears
`tell_visits`: a new capture cannot inherit the old face's consent to named
visits. The canonical name, existing stable ID (or missing legacy ID),
familiarity and added time remain. Other people, scoring state, memory and
rules are untouched. The store accepts one to five finite, unit-length
128-value prints and refuses any that recognize another known record,
including when the target itself matches more strongly.

The future caller must separately bind final account approval, a fresh
capture and the person's proved yes at the lamp to this target, with expiry
and cancellation. The snapshot and embedding checks provide neither that
authority nor biometric identity or liveness proof. No approval ticket,
capture, expiry controller or consent flow is implemented by these APIs.

Replacement is blocking disk work for a future controller off the capture
path. Success follows a private temporary file's sync, atomic rename and
parent-directory sync. A write failure before rename returns `NotWritten`,
leaving the old canonical file and RAM intact. After rename, RAM immediately
follows the visible replacement; a directory-sync failure returns
`Indeterminate`, never a rollback claim. The caller must consume its approval,
reload and reconcile that outcome before reporting a result, rather than
blindly retrying. Host fault fixtures cover these boundaries; they do not
establish power-loss durability or lamp timing.

### Face repair controller core (inactive)

`face_repair::Controller` adds bounded RAM staging, not an available lamp
capability. There is no RPC, environment read, model, camera/microphone
adapter or runtime activation. `LAMP_FACE_REPAIR` is only a declared name;
the constructor receives an explicit enabled value and exact Eyes boot.
One random ticket binds a store target and authenticated account generation
for at most 120 seconds before commit handoff; polls never renew it. The
first single candidate pins the exact `boot/fN`, with no retargeting. Trusted
runtime privacy or account changes and forget entry must call
`invalidate(now)` immediately, without waiting for another frame or supplying
the previous account/ticket. For forget, this comes before potentially slow
`People::forget` store I/O, even if storage later reports an error. The store's
revision check prevents a stale repair write but cannot itself drop the
controller's separate snapshot of the old person and staged prints.
Before handoff this drops the pending target, consent and prints and records
`Cancelled`; `observe` also rejects changed account or denied privacy on a
frame delivery. A fresh observation of another candidate, disappearance or
multiple faces cancels. Future ingress must also call `tick` on capture ticks
so missing fresh observations or expired deadlines need no app poll.
`invalidate` returns `Off` when disabled and `Unknown` without active work;
existing expiry and terminal receipts remain unchanged. It is a trusted
runtime API only, with no RPC or app/model route registered.

`FrameObservation::acquired_at` must be the frame's original monotonic
acquisition time, bound by the adapter to that frame's candidate, never its
delivery time. Old, future and repeated frames cannot refresh presence or
supply new prints. Capture requires five distinct latest observed frames
acquired after consent, each yielding a finite unit-length 128-value print.
Only those 128 values are copied into staging; excess caller allocation capacity
is not retained in capture state or the handed-off work.
Current source bounds are `FRESH_PROOF = 3 s`, candidate freshness 3 s,
`CONSENT_FOR = 30 s` and `CAPTURE_FOR = 4 s`. These are proposed bounds in
the code, not a timing decision from the first builder or measured lamp
limits.

`ConsentProof::from_trusted_ingress` is a constructor seam, not a verifier
or current runtime authority. Its future caller must prove an explicit
spoken yes to this ticket's already-asked question, the admitted microphone
request/session/worker and exactly the pinned face speaking during that
request's actual audio interval. Account initiation, model text or a visible
face does not establish that consent. The core checks supplied ticket,
candidate and time bindings; the authenticated account must then explicitly
attest the exact staged candidate before `begin_commit` consumes the target,
proof and prints once.

The phases are `Waiting`, `Capturing`, `Staged` and `Committing`.
`CommitWork` hands the consumed data to one exclusive store writer off the
capture path; the core performs no write. Once handed off, cancellation,
runtime invalidation or expiry cannot promise rollback: `invalidate` returns
`Committing`, and status remains `Committing` until the opaque
completion returns `Committed`, `Refused`, `NotWritten` or `Indeterminate`.
For a forget after handoff, the exclusive writer must order the store operations
and reconcile their actual outcomes; invalidation cannot retract consumed work
or claim the pending write was cancelled. The inactive writer below provides
this store ordering; no runtime caller is added here.
At most four terminal receipts remain in RAM. Eviction or restart makes a
ticket unknown; after a write handoff, the file may hold the complete old
or new record. Unknown never means unchanged: reload and reconcile the
store, retaining the store's explicit post-rename uncertainty.

The host target for bounded in-memory state/capture calls is under 1 ms,
currently unmeasured. `start` additionally requests OS entropy. Nineteen
controller fixtures pass on the development Mac. The forget-entry fixture
invalidates staged RAM
before actual `People::forget`, checks the cancelled ticket cannot hand off or
revive, and preserves the other person's complete record and stable ID. No model,
hardware, power-loss or end-to-end approval flow is qualified here.

### Face repair writer prerequisite (inactive)

`repair_writer::Writer` accepts the exact owned `People` instance and runs one
fixed thread. Prepare the controller's target and approved `CommitWork` before
transferring that instance; do not borrow another writer or reopen the file to
reserve a target. This is an inactive Rust library handoff, not a live repair
service. A future integration must route reservation, recognition, scoring,
ordinary remembering, photo confirmation and named-visit consent through the
same owner. Those commands, camera/account/Voice ingress, RPCs, environment
reads, capabilities and product activation are not added here.

The adapter holds at most one repair and one priority forget. A short mutex
protects only these in-memory records; no filesystem operation holds it.
Admission and status use `try_lock` and return `Contended` instead of waiting.
No callback or network receiver owns accepted work. The same worker selects
forget next after an in-flight repair returns, including if forget arrives
after storage returned but before its completion was polled. It also drains
accepted work after `close` or caller loss. Close rejects new admission and
does not join a blocked worker. Do not reopen the store until `stopped` confirms
the old owner exited, or the old process is known to have exited.

Trusted runtime must first call the controller's `invalidate(now)` at forget
entry and suppress stale known/tellable publication. Writer admission is not
that invalidation and proves no account authority. Internal monotonic forget
numbers make a retry of the same name refer to the first admitted operation
only while it is live or its result is untaken; its original timestamp and
deadline stay fixed. Conflicting live reuse is refused and a second distinct
pending forget returns `Busy`. After `take_forget` acknowledges the result,
the same number returns `StaleId` for mutation regardless of the new label;
use `forget_status` to read its retained result. An evicted old number also
never executes again. These numbers are not an app token or model argument.
The four acknowledged terminal receipts retain only operation IDs and fixed
status/counts, with no plaintext name or private stable ID. The live retry
name is dropped at acknowledgement and the worker's execution copy before
result publication. This is ownership removal, not zeroization or physical
RAM erasure. Only acknowledged receipts can be evicted. Live operations,
untaken repair completions and untaken forget results stay pinned separately.

Deadline status is separate from actual storage status. At the supplied
deadline, an unfinished operation reports `deadline_exceeded` with storage
still `Pending`: it failed to finish in time, not necessarily to write. The
repair checks before starting and before pre-rename I/O can refuse as actual
`NotWritten`. A forget request also latches that refusal for an older queued
repair, even if forget runs first and fails. A check racing with rename cannot
interrupt the system call. After rename, RAM follows the visible file and the
real directory sync decides `Committed` or `Indeterminate`; a late success
stays a success. The original opaque completion is taken once for
`Controller::complete`; no fabricated timeout result settles it.

Forget always uses actual `People::forget_at` under that same owner, even
after its response deadline. Its result keeps `forgot` and `unsearched`, the
stable ID captured before deletion, and `name_id_free` checked afterward.
Nothing reports forgotten at queue admission. An error can follow partial
canonical/set-aside work and does not mean unchanged; `unsearched > 0` retains
the existing pending-forget semantics. No memory/rules removal, unconfirmed
photo cleanup, success reply or identity publication is performed by this
prerequisite. Those callers must wait for and reconcile the actual result.

Blocking write/fsync/rename has no cancellation guarantee. A permanent stall
leaves one worker occupied and one forget pending, not a second writer or an
unbounded retry queue. Worker failure makes unfinished outcomes `Unknown` and
refuses new work. RAM queue/receipt loss on restart is unknown until actual
store reconciliation; neither replay nor a reboot rollback guarantee is added.
Ordinary forget's existing durability is unchanged.

The host target is under 1 ms for status/admission, including while a storage
barrier is held. All 16 real-store controls passed on the development Mac.
At the actual directory-sync checkpoint, first status took 3,713 ns, the
maximum of 100 repeated status calls 443 ns, and priority forget admission
15,688 ns. The measurement includes the bounded retry helper for an explicit
`Contended` result; it is not a cold filesystem or device measurement. The
separate held-state-lock control returns `Contended` without waiting.
Fixtures use real temporary stores, a test clock and bounded synchronization
at pre-rename/post-rename boundaries, checking success, indeterminate, refusal,
duplicate/saturated admission, completion once, caller loss, actual set-aside
results, cleanup failure and preservation of the other complete record/ID.
They do not demonstrate a bounded kernel-I/O time, device behavior or a live
account-to-lamp repair flow. Native ARM64 and full runtime acceptance remain
separate gates.

### One name for one face, and only a name they said (`LAMP_NAMES=checked`)

Off unless `LAMP_NAMES=checked` in the voice's environment, beside
`LAMP_EYES_WHO_SOCKET` (the worker's `--names-checked`, right after
`--who-socket`); built by the fourth builder on 16 October.

**Why** (the first builder, #705, 13:24): on lamp-4ace at 12:11, "How do
you spell my name?" was answered "It's just the single letter …". Then
"No, my name is …" was heard as Spanish words, and the lamp answered
that it would know them by a second name, made up from those words.
Without the switch:
- Any 1 to 40 letters are kept as a name, a lone letter among them; nothing
  checks that the person said it.
- Remembering replaces only a record with the *same* name. A second name
  for a face kept as "A" is a second person with the same face, and from then
  on recognition takes whichever is the closer match each time: the lamp
  may call them either, and their memory, kept by name
  ([memory.md](memory.md#each-persons-own-764)), is split in two.

With the switch:
- **Only a name they said.** `remember_my_face` is refused, before the
  eyes are asked, unless the name is in the person's own words, as the
  service wrote them (`Thread::names_window`, `known_faces::name_heard`):
  - in the turn in hand: what is being heard, or, once the lamp has begun
    to answer, the newest exchange;
  - or in one of the two exchanges before it, if the lamp said it back
    in its last answer and the turn in hand agrees ("Shall I call you
    Jim?" "Yes."), not a yes to something asked since. Agreeing is an
    ordinary yes ("yes", "sure", "of course", "go ahead", "uh-huh",
    "that's it", "vâng", "dạ", "có", "đúng rồi", …) with nothing anywhere
    in the turn that takes it back ("no", "not", "không", "sai", …), but in
    a yes that holds one ("no problem", "no worries", "why not", "don't
    mind"). "Sure, no problem!" agrees; "Yeah, no, it's …", "Of course
    not." and "Dạ không." don't (Ten and the reviewer on #1208: a
    correction mostly comes right after the yes word).

  Each of the name's words must be among theirs, or its letters spelled
  one by one ("J-I-M"), the run being the name's word itself. One letter
  said again and again ("It's Q. Q. Q.") spells only that letter. A word
  with an apostrophe is kept whole, so "It's cold" spells no "S". Accents
  count for nothing ("Jose" is "José", Vietnamese's marks too, and the
  comma below of "ș" and "ț"), whether written as one letter or as a
  letter and its mark, and "double N" is two N, but "double U" is W. A name whose letters they counted otherwise, in
  the turn it came from or the turn in hand ("three letters", seven
  taken), is refused. The model is told to say the name back and ask them
  to spell it (`known_faces::said_by_them`). A "yes" alone never passes,
  so a name the model made up is asked for again.

  Why the turn in hand, from lamp-4ace on 16 October at 16:46 (the first
  builder on #1057, the records described without the words): a
  three-letter name was kept after "Sure." and a lone digit, the turn
  before those a one-letter name corrected with that letter three times.
  Another was kept from "Hey, I'm …" heard wrong while the person was
  still correcting it. And seven letters were put together from four
  spelled letters, an ordinary first name said twice between them, and
  "three letters".
- **The word that called the lamp is not their name**
  (`known_faces::only_the_call`, 25 Oct).
  - **What's refused:** a name heard only as the line's opening word, past any greeting and set off by a comma or a
    stop with more said after it, when that word is the lamp's name or one the service writes for it
    (`attention::written_for_the_name`: "Man", "Helen", "Lynn", "Louvre" and the rest). The model is told to ask them
    their name.
  - **Why:** on lamp-4ace on 24 October the owner's "Lamp, remember my face, and I'm …" was heard as "Man. Remember my
    face and do", and their face was kept as "Man" and greeted by it.
  - **Still a name:** a line that is only the name ("Helen."), or the name said again as theirs ("Helen, remember my
    face. I'm Helen.").
- **One name for one face.** The voice asks the eyes
  `{"remember":"Jim","one_face":true}`. Prints already someone's under
  another name are refused with `{"error":"known_as","known_as":"A"}`
  (every name they are known by: "A and C", since `replace` forgets
  them all), and
  the model is told to say the new name back and ask whether they want to
  be known by it instead. Only once they say yes does it call again with
  `replace` true (`"replace":true`), and the eyes then forget "A" and keep
  "Jim" in its place (`People::remember_one`). `replace` is refused before
  the eyes are asked unless the lamp said the new name in its last answer
  and the turn in hand is a plain yes to it: yeses, the name itself and
  words that stand beside a yes ("please", "that's fine", "call me …",
  "go for it", "I'd like that", "gọi tôi là …"), and nothing else, not
  said as a question ("Yes?", "Dạ?", which is "sorry, what?")
  (`known_faces::plain_yes`). "Yes, but it's Kess.",
  "Sure, why not Kess?" and a spelled correction are asked about again
  (the reviewer on #1208). A misheard name is never put in place of one
  they have while they are still correcting it. A name kept from an
  earlier exchange needs the same plain yes. The journal says "did not
  remember a face: it is known by another name", without the name.
- **A near face with a near name is asked to spell it** (#2176 row 4). On
  lamp-4ace on 20 Oct a person known since 16 Oct was saved again under
  their name misheard by one letter: the new prints came to at most 0.359
  against their own, 0.004 under `SAME_PERSON` (0.363), so the check
  above let it through. Now, with no print at `SAME_PERSON` to anyone, a
  print at or over `NEAR_PERSON` (0.30) to someone known by a near name is
  refused with `{"error":"near_to","near_to":NAME}`, naming the likeliest
  such person, and nothing is kept. Near names (`who::near_name`) are two
  names of three letters or more that aren't the same name but are one
  letter apart (one put in, left out or changed), or no letters apart,
  once lower-cased with accents and tone marks taken off. A near name with
  a far face changes nothing, so a Linh is never asked about unless their
  face is near Minh's too.
  - **No known name is said back.** The model is told only that it may
    have misheard, and to ask them to spell their name. A name the person
    hasn't said or spelled never passes (`known_faces::name_heard`), so a
    "yes" to a name the lamp said can't stand in for it. Without that, a
    sibling who said yes to "are you Minh?" would have taken Minh's face,
    and with it Minh's memory (the reviewer's P1 on #2202). It also keeps
    a known person's name from someone who isn't them (P2).
  - **Once spelled,** the model calls again with the spelled name and
    `"apart":true`, whatever the first call held, so the person is asked
    once (Reviewer-lead's P3 on #2252). The known name spelled: judged
    against theirs, as below. Another name: kept beside the near one. The
    voice takes `apart` only when the turn in hand spells the name letter
    by letter (`known_faces::spelled_by_them`), a name of two words spelled
    as one run ("M A R Y A N N") included. A name said again can be
    misheard again as exactly the near name, so it goes without `apart`,
    as does a yes, and is asked about again (both reviewers' P2 on #2269).
    `apart` is sent only when true, since eyes from before it refuse a
    field they don't know.
- **A new name for a face near someone kept is asked about** (#3739). On a
  tester's lamp on 27 Oct one person was remembered under their name and,
  21 s later, under a name misheard far from it; the second capture wasn't
  recognised as them, and the lamp kept two people for one face. Now, with
  no print recognised as anyone, a capture whose **median** print is at or
  over `NEAR_PERSON` to someone kept under another name is refused with
  `{"error":"near_to","near_to":NAME}` whatever the name, and nothing is
  kept. The voice answers it as above: no known name is said, and they
  are asked to spell their name.
  - **Their own name spelled** (`apart`) is judged against theirs, as
    below: the near prints go beside theirs, still one person.
  - **Another name** goes in only with `apart`, as a new person: spelled
    in the turn in hand, as below. "I'm someone else" alone sends nothing
    (Third's 18:02 HOLD on #3792). Siblings who look alike spell it once.
  - **A yes renames nobody.** `replace` on a face not recognised is
    refused the same way, so a relative's yes never takes the kept
    person's name or memory id (the HOLDs on #3792). A plain no sends
    nothing.
  - A rename stays for a face **recognised** under another name
    (`known_as` above), and keeps only the new prints at `SAME_PERSON` to
    them, under their memory id.
  - A capture under `NEAR_PERSON` to everyone kept is someone new, as
    before.
- **A known name never takes a face that isn't theirs**, whatever is asked
  or said yes to (the reviewer's P1s on #2202 and #2211, Jarvis's on
  #2235). A capture is judged as a whole by its **median** print's
  likeness to theirs, so one near print among unrelated ones doesn't carry
  them. **A face the lamp knows under another name never takes a known
  name**, near theirs or not: always `not_like_them`. Under a name the
  lamp already keeps:
  - **At `SAME_PERSON` to theirs** (by the median): the new prints are
    kept in place of theirs, but only those themselves at `SAME_PERSON` to
    theirs, so up to two of five that are someone else's don't go in with
    them, a sibling's near print included (the reviewer's P1 after #2252,
    Reviewer-lead's P3 on #2269).
  - **Near (`NEAR_PERSON` or more):** refused as `near_to` too, since it may
    be another person's name misheard into exactly theirs, so the model asks
    for a spelling. Spelled as their name (`apart`), only the new prints that
    are near theirs are added beside theirs, keeping the newest five with at least one of their own,
    and familiarity and the who-was-here yes as they were.
  - **Under that:** always `{"error":"not_like_them"}`, `replace` and
    `apart` too. The model is told it can't keep this face under that name,
    to say neither why nor whether it knows anyone by it, and to ask for
    another name. The lamp's owner can remove an old face in the app
    (#2199).
  - Before this, any capture misheard as a known name replaced that person.
  - `apart` skips only the near-name check, never this one.
- A face confirmed from a photo keeps its path as before (`apart`). The
  journal says "did not remember a face: it is near someone known by a
  near name", or "its name is someone's whose face it isn't", naming
  nobody. It's part of `LAMP_NAMES=checked`, which the product's unit
  sets, with no switch of its own.
- **A name told is a name kept** (the first builder, #705, 13:43). The
  lamp-4ace records showed no face asked to be kept on 15 October at all:
  the model had only said it would remember. With the switch,
  `remember_my_face` is to be called when someone tells their name, not
  only when they ask. And once what they said in a request gives their
  name ("my name is Jim", "my name's Jim", "call me Jim", "I am called
  Jim", "I'm Jim", "I am Jim", "this is Jim speaking", "this is Jim
  here", "Jim's my name", "Jim is my name", or "this is Jim" and "it's
  Jim" where the request speaks of "my name", "Will you remember my name?
  It's Jim."; not "Lamp, this is Jim.", "This is Jim, my colleague." nor
  "What's his name? It's Bob.", which at a desk mostly name someone else,
  whose name the eyes would file on the one face they see, nor "He said
  Jim is my name." (Ten and the reviewer on #1224); the name capitalised
  and not a word that
  is no one's, such as "OK", a day or "American"; not after "what", and
  not in a question, except one that asks the lamp to call them so or to
  remember it, "Could you call me Jim?" or "Do you remember my name is
  Jim?"; `known_faces::gives_a_name`), the worker puts one note beside
  that request (`known_faces::NAME_TOLD_NOTE`): if they have just told
  their own name, call `remember_my_face` now, or say the name back and
  ask for its spelling, and never only say it will be remembered. It is a
  condition, so that a match that is no name ("I'm Irish.") costs
  nothing. The journal says "they gave their
  name, and the model is told to keep it".
- **On the second route, an ask with a name calls it**
  (`LAMP_SECOND_REMEMBER_ASKED=on`, off by default; the owner's try on
  lamp-4ace, 23 Oct 18:30). There "Lamp, remember my face, I'm …" was
  given every function and its first round called `take_a_look`. The
  spelled name after it called nothing, and the lamp said it knew the
  face while the eyes held no one. With the switch, names checked and the
  eyes' who socket, a line whose own words plainly ask the lamp to
  remember the speaker's face or who they are, at a sentence's start
  after a greeting or the lamp's name and an optional "please" or "can,
  could, would or will you" ("remember my face", "remember me", "remember
  who I am", "remember what I look like", "learn my face", "save my face",
  "remember my name"; `known_faces::asks_to_remember`) and give their name
  (`known_faces::gives_a_name`) has its first round sent with
  `tool_choice` naming `remember_my_face`. The name is still checked, so
  one not in their words is refused, and the model asks them to spell it.
  Not forced, the model's as before:
  - an ask with no name;
  - a name in the line after an ask, since the thread doesn't say the two
    lines were the same person's;
  - a line with a refusal anywhere in it ("I don't want you to remember my
    face", "I'd rather you didn't", "You cannot remember my face", any word
    ending in "n't", and the same written without the apostrophe);
  - the phrase further into a sentence: reported speech ("She said remember
    my face"), a question whether the lamp knows them ("Do you still
    remember me?"), "Help me remember my name tag", or an ask said another
    way ("I want you to remember my face");
  - a bare question ("Remember me?");
  - someone else's face, and a round after a call;
  - Vietnamese, which `gives_a_name` doesn't read (reviewer-three's P2s on
    #3298).

  The line that names one of the staff (`LAMP_TASKS_NAMED_ASK`) is forced
  first. A forced save, like the model's, checks the name and that one
  face is in view, but not that the face is the speaker's.
- The eyes take the plain `{"remember":"A"}` as before, so an older
  voice is unchanged. The other way round, a voice with the switch talking
  to older eyes gets `not_understood` for every remember (they take no
  field they don't know), so the switch needs both parts of this change.

What it doesn't do (the reviewer on #1057):
- **The note is the Live worker's only.** The second route gets the check
  and one name for one face, but no note.
- **The note goes only if the name's words reach the worker before the
  request ends,** and the service's text often trails the speech. On
  lamp-4ace, the "they gave their name" lines are to be counted against
  the names given.
- **A first name heard wrong, for a face with no name yet, is kept as
  heard:** the first time, "Hey, I'm …" heard wrong is a name they said.
  A wrong one is corrected by "No, it's …", which goes through the read-back.
- **The check needs the turn's own words in the thread when the call
  runs.** A call that comes before its transcript is refused and asked
  again, and the service's text often trails the speech. Not measured: the
  refusals of a name the person had just said, in lamp-4ace's journal.
- **After `replace`, the person's memory stays under the old name's id**
  (`remember.rs`, a hash of the name). It isn't recalled for the new name,
  and their forget doesn't reach it.

Tests: `who::tests::one_face_keeps_one_name_and_another_is_taken_only_in_its_place`,
`a_face_known_by_two_names_is_refused_with_both_and_replaced_for_both`
and the ask's two new lines (the eyes);
`known_faces::tests::a_name_counts_only_when_the_person_said_it_or_spelled_it`,
`a_name_given_is_told_from_talk_that_only_mentions_a_name`,
`the_three_names_of_16_46_are_refused_or_read_back`,
`a_name_said_before_is_kept_once_said_back_and_agreed_to`,
`a_yes_is_any_ordinary_one_and_any_no_takes_it_back`,
`a_yes_to_the_read_back_holds_nothing_but_the_yes_and_the_name` and
`with_names_checked_only_a_name_they_said_reaches_the_eyes_and_one_face_keeps_one`
(the voice, against a stand-in for the eyes on a real socket);
`provider_worker::tests::names::with_names_checked_a_name_given_is_told_to_be_kept_and_plain_talk_is_not`
(through the worker's own loop). Not
exercised: a played name in a busy room on a lamp, and how often the
service writes a name said once right.

### Remember only the person who spoke (`LAMP_REMEMBER_SPEAKER=on`)

Off by default in both Voice and Eyes. Eyes needs its recognizer and who
socket, `LAMP_BUS=on` and `LAMP_EYES_SPEAKING=on`; Voice also needs
`LAMP_EYES_SPEAKING=on` and the bus. With `LAMP_NAMES=checked`, the second
voice route binds `remember_my_face` to the current microphone line, its
monotonic audio interval, and exactly one face positively seen speaking.
A different face speaking even briefly refuses the request. At least three
distinct looks must positively show that speaker, at least half the looks
in the line must be positive, and all must share one Eyes boot. Unknown
looks do not count as positive evidence. The complete audio interval is
at most 30 seconds. Short or missed camera evidence therefore refuses.
The requested name must belong to a direct self-introduction in that line,
or be its entire bare-name or spelling response. A line containing a
greeting, polite preface or remember-my-face/name request needs an explicit
self-introduction too:
"Hi, Kess" and "Remember my face, Kess" may address Kess, so they refuse. Merely mentioning a name
is insufficient: "Remember her face, she's Kess" cannot save the speaker
as Kess. A self-introduction elsewhere in mixed-person narration does not
authorize another name; ambiguous mixed clauses, quoted/reported speech,
negation and tentative introductions refuse and need a fresh direct
introduction. Both the heard line and raw requested name are limited to
4 KiB. Existing spelling and letter-count checks still apply. A word name
that collides with the refusal vocabulary needs a spelling-only response.
Earlier exchanges and model-supplied face identifiers are not evidence.
With `LAMP_MEMORY_ROOM_SPEAKER` also on, its background attribution keeps
this same request's enrollment proof through the live scope check. It does
not replace the proof with visibility or a room speaker. A cancelled,
expired or mismatched request cannot publish a late proof. The off path
continues to publish room attribution without enrollment evidence.
With no eyes, missing or stale evidence,
or an unsupported route, nothing is saved. Guarded `replace=true` is refused:
the earlier confirmation is not yet bound to the same speaker. Ordinary
enrollment with the switch off keeps its existing behavior.

If Eyes says the face is already known by another name, the guarded reply
says the name has not changed. It neither reveals that stored name nor
asks for a replacement confirmation that this path cannot honor. The
ordinary switch-off name/read-back behavior remains unchanged.

The text check is not general language understanding. A bare name after a
question about a friend is indistinguishable from the speaker naming themself.
Folding punctuation can also join a name and an addressee: "I'm Kess, Mai"
can pass for the requested name "Kess Mai", although "Mai" alone refuses.
A question mark lost in transcription cannot be recovered from these words,
including in "em là…". These remain explicit physical/model trial limits.

Some ordinary forms refuse conservatively: "Remember my face, I'm Kess,
and this is Mai" refuses even Kess; "Remember me please, I'm Kess", "Im
Kess", "It's Kess", and "anh/chị là…" need a fresh supported introduction.
The refusal fixtures and the physical sheet record these expected outcomes.

These are checks on the classifier's observations, not independent proof
of acoustic identity. An unread, far or turned-away second face does not
automatically refuse the request: only a second face positively read as
speaking does. If that requester speaks while a near bystander's mouth is
misclassified as speaking in enough looks, the bystander can still be saved
under the requester's name. Unknown is not positive evidence, but it does
not prove that an unobserved person is quiet. The two-person trial must
cover these cases before activation; this change does not fix the classifier.

Voice refuses enrollment evidence above 4 KiB of transcript or a 128-byte
face identifier rather than truncating away a correction. Its existing
64-line store bounds the number of proofs; expiry, stop and privacy
invalidation discard their enrollment words and identity.

Voice sends a distinct internal operation on the existing who socket:

```json
{"remember_speaker":{"name":"Fixture Speaker","face":"boot/f3","began_us":1000000,"ended_us":3000000,"replace":false,"apart":false}}
```

The proof is built by Voice, never accepted as model tool arguments. Old
Eyes refuses this unknown operation; Voice never retries it as a plain
remember. New Eyes with the switch off also refuses it. With the switch on,
Eyes refuses legacy live `remember` operations with `speaker_required`, so
an older Voice cannot silently bypass the check. Photo enrollment retains
its separate confirmation path.

Eyes reuses its already-computed SFace prints, only when exactly one near
face is seen speaking in that same look. At most 16 prints, less than
30 seconds old, stay in RAM under one camera permission hold. No image is
kept and no background inference is added. The anchor must have the driver's
capture time inside the spoken interval; missing history after capacity
eviction is a refusal. Closing/reopening the camera clears these anchors,
and a buffered frame from before the new hold cannot refill them. A request
spanning a camera-hold reset or forget is refused even if the same tracking
ID has a later print.

Anchors are made only on the existing two-second recognition tick, and
only when that tick's near face reads as speaking. A roughly 1.8-second
enrollment line can fall wholly between ticks and return `speaker_not_seen`
despite correct speaking observations at every camera look. Such short
requests may refuse frequently; shifting the same line across a tick can
succeed. The cadence regression exercises both phases through
`recognise_due` and the actual anchor cache, without forcing extra inference.

Five distinct fresh frames captured after the request must show the same
face, each matching the immutable anchor at the existing SFace threshold
0.363. A passing tracking ID alone is insufficient: another person can
inherit it. A missing, ambiguous, unreadable or different face terminates
the capture. Forget clears both anchors and a pending guarded capture even
if the subsequent file removal fails. Caller closure, permission, expiry
and capture failure are checked again immediately before the People write.
Voice closes its connection on cancellation, privacy loss or lease loss;
only Eyes' actual success may be reported as remembered.

Latency target: add no wait or inference to the microphone path or the
background recognition cadence. The existing four-second Eyes capture
limit and six-second tool deadline remain; absent proof refuses immediately.
Software tests cover the protocol, collector, identity replacement, stale
and buffered frames, cancellation, recognition cadence and legacy behavior.
They do not launch Linux's camera/model-backed `main.rs` loop: its legacy
request refusal, tick wiring and unreadable-frame dispatch are source-read
and compiled by native CI, while the shared protocol/cache/collector paths
are executed in host tests. End-to-end coverage of that loop remains part
of the lamp qualification. Physical first-use,
repeated-use and slow-dependency latency, and speaking-classifier accuracy
in a room, still need a lamp trial. The switch remains off pending that trial.

## On the bus

With `LAMP_BUS=on` in its unit the eyes also publish every face the
detector finds, not only the one the body is told to look at, on
`humans/faces/tracked` ([bus.md](bus.md), "On the lamp";
`crates/eyes/src/bus_out.rs`). At start the journal says what this process
says on the bus, and which of `room/presence` and `room/activity` it
doesn't with the switch that would, as `said on the bus: humans/faces/tracked,
diagnostics; not said: room/presence (LAMP_PRESENCE), room/activity
(LAMP_ACTIVITY)`: a build without a part, or a switch that didn't reach the
process, shows there before any trial waits for it. For each face:

- **a passing id** (`f1`, `f2`, ...): the same from one look to the next
  for as long as its box overlaps where it was within the last 1.5 s,
  a new one after that, never given twice, and never a person's name;
- **its box**, each edge as a fraction of the picture;
- **how sure the detector was**;
- **how far it is turned**, in radians, and **whether that counts as
  turned to the lamp** ([`facing`](../crates/eyes/src/facing.rs): within
  60 degrees in the middle of the picture, more toward its edges, where
  the lens shows a straight face turned). A face narrower than 7.5% of
  the picture, or with an eye or the nose outside it, has neither: it
  counts for nothing either way.

A list lasts two seconds, and an empty one says the camera looked and saw
nobody. With the camera closed nothing is published and the last list
runs out. This is the sign a person gives, without meaning to, that they
are speaking to the lamp. Nothing uses it yet: the part that decides
whom the lamp answers ([policy.md](policy.md)) is to. The numbers for
"turned to the lamp" are V1's, tuned on V1's lamps at twice this
picture's width, and the sign of the angle assumes the picture is not
mirrored: neither has been checked with a person in front of this lamp.

About once a second the eyes also say how they are, on `diagnostics`
under the name `eyes`, for the lamp's health ([health.md](health.md)):

| Level | Message | When |
|---|---|---|
| `ok` | | The camera is open, or closed because seeing is not permitted now. Closed is the eyes working as they should |
| `warn` | `the camera does not open` | Permitted, and the camera cannot be opened; it is tried again |
| `warn` | `the camera does not open; its USB device was reset N times` | The same, once the stuck camera's USB device has been reset (#2126) |
| `warn` | `the camera stopped giving pictures` | It was open and failed, until it is open again |
| `warn` | `the eyes set aside N people's records they couldn't read` | While anything is in `set-aside/` beside the people file (#1505), counted once a minute; a camera warning is said first |

The facts beside it are `camera` (`open`, `closed`, `not opening` or
`failed`), `set_aside`, the count of people's records set aside (0 when
none), and `camera_resets`, the camera's USB device reset since it last
opened. It is said on every turn of the loop that is due, whatever the
camera is doing, and the sign that the eyes are there is now given the
same way: before 2026-10-14 it was not given while the camera was not
permitted, so the eyes read as gone from the bus whenever they were not
looking.

### A face from a photo (`--photo-dir`, faces from a photo)

Off unless `lamp-eyes --photo-dir ABSOLUTE_PATH` is given, which needs the
recogniser (`--recogniser`, `--people`, `--who-socket`). On a lamp the
folder is `/run/lampos-photos` (`lampos`'s, 0700), made at boot by the
product's tmpfiles and belonging to neither the eyes' unit nor lamp-cloud's
(cloud.md, "A face from the app's photo"); the eyes make it at 0700 if they
can, and refuse one open to others. One that isn't ready stops only photos:
the eyes start without them, say so once, and check it again at each photo
asked, answering `photos_off` while it isn't. lamp-cloud puts the app's
photo there (`faces.add_photo`), then asks on the
`who` socket, as the voice does: `{"remember":"NAME","photo":"FILE"}`.
`FILE` is a plain file name in the folder: letters, digits, `-`, `_` and
`.`, not hidden, at most 128 bytes (`photo::plain_file`).

**Only the prints of people who asked are kept, and a photo isn't their
asking** (the first builder on #1968, 22:12). So a face from a photo is
kept **unconfirmed**, until its person says yes at the lamp:

- **Read once, then gone:** the file is read (a regular file, not a link,
  checked to be the entry it opened; at most 8 MiB) and **removed whatever
  follows**: a refusal, a bad name, a picture with no face. A link is
  removed, not what it names. No picture is kept, only the print
  (`photo::take`).
- **Its one face:** decoded at the detector's size as a camera frame is,
  its faces found, and the largest taken. It must be at least `NEAR_ENOUGH`
  (0.10) of the picture's height, with no other face half its size or more
  (`photo::one_face`). Then it's cut out upright and printed as recognition
  does (`photo::print_of`).
- **Kept apart, unconfirmed:** in its own file beside the people's,
  `<people>.from-photos` (mode 0600, `photo::Unconfirmeds`), never in
  `people.json`. An older build never reads it, so never takes it for
  someone who asked. At most 20. It's **never named, counted, greeted or
  used for "who is this?"**: recognition, the known file, `humans/familiar`
  and the hello read the people's file only. The answer is
  `{"added":NAME,"unconfirmed":true}`. A face already someone's who asked is
  theirs: `{"remembered":NAME}` for the same name, `known_as` for another.
- **When it's at the lamp:** recognition finds nobody who asked, but an
  unconfirmed face matches. The eyes then note that face as here, and say
  only that when asked: `{"photo_here":true}` gives `{"photo_here":NAME,
  "asked":BOOL}` or `null`, so the voice can ask the person once
  (`{"photo_asked":NAME}`). `asked` is that face's own: a face let go and
  added again under the same name is a new face, not asked yet, and the voice
  asks it once more (Mobile on #1968, lamp-0c89).
- **As the lamp sees them** (#2854): each print of that face at the lamp, of
  nobody who asked, is kept beside its photo's, the newest four, **in memory
  only** (`photo::Unconfirmeds::saw`, `SEEN_PRINTS`), never in the file. A
  selfie is one view, the phone's: kept with it alone, the face was named
  only where it looked most like it, near the middle of the view (lamp-0c89,
  21 Oct, #1968). They go with the face on a no, no answer, a forget, a new
  photo under the name or its 30 days, and at a restart.
- **Their answer, taken only for the face at the lamp now** (seen within
  `KNOWN_FOR`, `photo::is_here`), so nothing elsewhere can say yes for
  someone (the reviewer on #1968, 22:28): `photo_asked` for a face not here
  is `not_here`, and `photo_yes` is `not_here` or, if it wasn't asked,
  `not_asked`.
  - **The face that said it** (`LAMP_PHOTO_SPEAKER=on` on lamp-eyes, off
    unless set, #2856): `photo_here` also gives `"face":"<boot>/f<id>"`, the
    passing id of the near face that matched the photo under the faces
    publisher's boot, the voice's `boot/f3` form, or `null` while speaking
    isn't judged (`LAMP_EYES_SPEAKING`) or no face is fresh here. A yes must
    carry it, `{"photo_yes":NAME,"face":"<boot>/f<id>"}`, and is kept only
    if it is that same id under that same boot (`photo::is_that_face`):
    another, an older boot's, `null`, or a bare yes is `not_that_face`, and
    the face stays unconfirmed. A passing id can't tell two people apart:
    another face in the same place within 1.5 s takes it
    (`bus_out::Ids::of`; reviewer-three's P2). So a yes that passes is not
    kept at once: it waits (`photo::PendingYes`), pinned to the name's
    unconfirmed record (`Unconfirmeds::record_of`), the camera permission's
    hold (`permit::Permit::held`) and when it reached the eyes, and the eyes
    force one look. That look counts only on a frame captured strictly after
    the yes reached the eyes, in a later microsecond, not the same one
    (the driver's monotonic time, `photo::frame_us`; a frame without one
    never counts, so the yes runs out rather than take a frame dated by its
    read): a frame
    buffered before it leaves the yes waiting. It is kept only if that
    look's near face has the yes's id and prints as the photo's face
    (`photo::yes_after_look`). No face, no print, another print or another
    id is `not_that_face`, nothing kept. Checked where it is kept, after the
    look, and at every turn of the loop: past `photo::YES_LOOK_WITHIN` (1 s)
    from its arrival, a slow look included, it is `not_that_face`, so the
    yes is answered within about a second, inside the voice's 6 s; the camera
    shut meanwhile, by a `{"camera":false}` or a permission run out, though
    it opens again, is `eyes_shut`, and the yes is gone for good; the name
    added again from another photo, let go, forgotten or answered no is
    another record or none, `not_unconfirmed` if a look comes before the
    deadline, else `not_that_face` (Fourth's three P2s on #2856). The
    voice closing its connection withdraws the yes, `peer_closed`, nothing
    kept (`photo::Caller`, Fourth's 17:01 on #2856): checked at every turn,
    at the look, and once more right before the save
    (`photo::YesEnded::fenced`). That last check is the commit point: a
    close seen before it refuses; a close after it can't undo the save.
    It is not an atomic revocation across the two processes. One yes waits
    at a time: another meanwhile is `busy`.
    And a look whose near print isn't the photo's drops the face from
    `photo_here` at once (`photo::print_is_photos`), rather than leaving it
    there for `KNOWN_FOR`. The id is held in memory only, beside the
    name, and means nothing after a restart. The voice judges who spoke the
    yes; the eyes only name the face and check it again. Off, both answer
    as before, without `face`, and a yes with `face`, even `"face":null`,
    is `not_understood`; a `face` key beside any other ask, `null` too, is
    not understood either way.
  - `{"photo_yes":NAME}` makes it an ordinary person (`People::remember_one`),
    with the photo's print and those the lamp took of them (`Taken::prints`),
    five at most, as one remembered at the lamp keeps, and the remembering's
    own answers. A yes the store refuses (`full`)
    leaves it unconfirmed as it was.
  - `{"photo_no":NAME}` deletes it (`{"let_go":true}`).
  - **No answer:** once asked, a face not seen for 20 s, the meeting over,
    is let go (`photo::unanswered`, `MEETING_GAP_US`).
  - **"Forget my face"** deletes it too.
- **Never confirmed:** deleted 30 days after it was added
  (`UNCONFIRMED_FOR_S`), looked at once a second. A `.from-photos` file that
  can't be read is removed, so no print sits out of that reach.
- **The product reset** wipes `<people>.from-photos` with `people.json`
  (`reset-plan-product.json`).
- **The answers** are the voice's: `photos_off`, `not_a_name`, `unreadable`
  (no file, a link, too large, not a JPEG), `no_face`, `too_far`,
  `two_faces`, `known_as`, `full`, `not_here`, `not_asked`, `not_that_face`,
  `not_unconfirmed`, `busy`, `eyes_shut` or `storage`; `peer_closed` is
  logged only, as no one is left to answer. The
  journal says only which, never the name or the file.

**The voice's asking** (`LAMP_PHOTO_FACES=on` in the voice, with the eyes'
socket; `crates/voice/src/photo_faces.rs`):
- A thread asks the eyes `photo_here` every 2 s, off the voice's loop, so
  the loop never waits on them (`photo_faces::Watch`).
- When an unconfirmed face is here and wasn't asked about yet, and the lamp
  is quiet (no turn under way, nothing queued, nothing said for 3 s, input
  ready, a session open), the thread tells the eyes `photo_asked`, once:
  a name the loop wants again while the eyes are still answering is not
  told twice (on lamp-0c89 it was asked twice, 16 s apart, #1968). The
  voice follows the eyes' own mark, which they save: a name the eyes say was
  not asked is a new face under it and is asked again, and a face they say
  was asked isn't asked again, even after the voice restarts. The
  voice then says, once, as its own written turn (`photo_face_ask`): "Someone
  added your face from a photo, as NAME. Shall I remember you?" It doesn't
  use the name as theirs unless they agree.
- **Their spoken answer** calls `photo_face_yes` or `photo_face_no`. These
  run in the provider's worker, a process apart from the coordinator that
  asked, so they take the face from the eyes, the unconfirmed one
  `photo_here` names now, and tell the eyes `photo_yes` or `photo_no` for it.
  The eyes take a yes only for that face once it was asked (`not_asked`
  otherwise), so no name has to cross processes (the reviewer on #1977,
  22:50). Like the other
  face functions, these are taken only from words heard at the lamp, never
  a written or typed request. A yes for a face that has gone is
  `not_here`, and the model is told nothing changed.
- No answer is the eyes' to let go, at the meeting's end.

**From the app:** lamp-cloud's `faces.add_photo` writes the app's photo into
`--photo-dir` as a new 0600 file and sends the ask, only over the lamp's own
account channel, and removes the file afterwards ([cloud.md](cloud.md), "A
face from the app's photo"). The eyes make the folder theirs at 0700 when
they start, and refuse one open to others (`photo::ready`).

Tests (`photo.rs`):
- the file read once and removed, a second ask finding nothing; one too
  large refused and removed; a link refused and removed, and what it names
  kept; a folder left; only plain file names;
- the one face, or `no_face`, `two_faces` or `too_far`, on made-up faces;
- the unconfirmed store: its own file, 0600, the people's untouched; found
  only by likeness; read back; yes takes it out; no deletes it; asked and
  unseen for 20 s, let go; 30 days, deleted;
- with the pinned models (`LAMP_EYES_MODELS`): a grey picture is `no_face`,
  bytes that are no JPEG `unreadable`, and with `LAMP_FACE_PHOTO` a print of
  length one. That was run here with the two pinned models and NASA's
  public-domain astronaut portrait (scikit-image's sample, never committed).

The asks' parsing is in `who.rs`, `--photo-dir` in `main.rs`, and the reset's
wipe in `crates/setup/tests/it/reset.rs`. Not run on a lamp.

### How familiar a remembered person is (`--visits on`, #1670)

Off unless `lamp-eyes --visits on`, which needs the recogniser (`--recogniser`, `--people`, `--who-socket`). Only
for someone who asked to be remembered. The first builder's ruling on #1670
(12:19): keep less than a day-by-day record, and the owner decides whether
it is on in the product.

- **Kept:** one number per remembered person in `people.json`
  (`familiar`), beside one time of scoring for the whole file
  (`scored_at_unix_s`). A person is counted when recognised, at most once
  in 16 hours, **in memory only**: nothing is written at a visit. The
  counts reach the file once **03:00 at the lamp** has passed (the lamp's
  zone, `/etc/localtime` through `lamp-local-time`, followed as it changes;
  UTC if it can't be read, said once), at the eyes' next once-a-minute look
  whoever is at the lamp: every number is faded to that 03:00, each count
  since is added (one a counted day), and the file is written with that
  03:00 as its time of scoring. So neither the file's fields nor the time
  it was written say when anyone came, only that a night passed (the
  reviewer on #1763; the first builder's 13:38 for the hour). The first
  scoring waits for the next 03:00 too. Numbers halve every fourteen days
  (`who.rs`, `People::seen`, `People::score`). A restart before the day
  ends loses that day's counts, and one in the same day may count someone
  twice. Both fields are optional and only add, so the file keeps version
  1. "Forget my face" removes the number and any count not yet written,
  and the product reset empties the file.
- **Levels:** `new` under 2, `regular` from 2 (about once a week, kept
  up), `close` from 8 (about three days a week). These are authored, and
  get tuned from real scores after a week on the office lamps.
- **Said:** with `LAMP_BUS=on`, at each recognition (every 2 s while a face
  is near) the level of the remembered person at the lamp, or `null` for
  nobody near, someone not remembered or not recognised, on
  `humans/familiar` (`lamp_msgs::HumanFamiliar`, good for 5 s). No name is
  in it.
- **Told:** when someone asks to be remembered, the eyes' answer carries
  `"visits": true`, and the voice tells the model to say in one short
  sentence that the lamp will notice how often they come by, so it can
  greet them more warmly (`known_faces.rs`, `VISITS_LINE`).

Tests (a lamp at UTC+7): counted once in 16 hours with nothing written;
written only once 03:00 local has passed, once, scored as of that 03:00; `regular` on the
third day, before it is written, and `close` after two weeks of days;
halved in fourteen days and back to `new`; read back from the file; only
the number and the time of scoring are written, nothing for someone never
seen; and forgetting takes the number and an unwritten count
(`who.rs`). The level is said only with the switch, from the eyes, with no
name (`bus_out.rs`). The voice's line comes only with `visits: true`. Not
run on a lamp.

### How the person at the lamp seems (`--seems`, #1671, step 1)

Off unless `lamp-eyes --seems /opt/lampOS/eyes/models/emotion-ferplus-8.onnx`
is given. It doesn't need the recogniser, and it never asks who anyone is. It's a
read for **measuring how often it is wrong**. Only Mood takes it, with
`LAMP_MOOD_SEEMS=on`, and only as the breath's pace and the idle head's
liveliness (#1671, mood.md), and
the lamp never says how anyone looks (#1671; the first builder's 14:04).

- **The model:** FER+ (`emotion-ferplus-8.onnx`, the ONNX model zoo's,
  MIT), 35,040,571 bytes, pinned by SHA-256 in `seems.rs`, run under
  tract. No build or release carries it yet. It is placed by hand on a test lamp.
- **The read:** at most every 2 s, the nearest face at or over
  `NEAR_ENOUGH` of the picture's height, with its eyes at least 10 px
  apart (`seems::READ_EYES_PX`, `seems::near_face`, for both readers), is
  cut out upright as for
  recognition (`who::align`, 112 by 112), made grey and 64 by 64
  (`seems::grey`), and scored on eight expressions. These are reduced, as V1
  did: `positive` when happiness holds 0.6 or more of the eight, `negative`
  when sadness, anger, disgust, fear and contempt together do, else
  `unsure` (`seems::judge`, `NAMED_FROM`, authored, to be set from the
  measure).
- **Said:** with `LAMP_BUS=on`, each read on `humans/seems`
  (`lamp_msgs::HumanSeems`, good for 5 s): a kind and a confidence only.
  No name, print or picture is kept or said. With no face near, nothing is
  said, and a reader treats no fresh value as not known.
- **Only with the lamp's yes, for everyone it sees** (the reviewer-lead's
  P2 on #2872), kept
  as the ears keep the yes to room sounds: `seems::due` reads
  `/var/lib/lampOS/consent/face-expressions.json` when a read is due, and
  only a JSON object whose `face_expressions` is exactly `"yes"` counts
  (`seems::face_expressions_yes`). Missing, unreadable, over 4096 bytes or
  anything else is no: no face is read, by either reader, nothing goes on
  `humans/seems` or into `--seems-log`, and the ask comes again 2 s later.
  The journal says once each time the yes comes or goes. The eyes still
  look, follow faces and say who is there. Who writes the file:
  privacy-switch.md, "Face reads stay off without a yes".
- **The journal** says only how many reads there were in the last minute,
  how long one took on average and the longest, for the cost on a lamp.
  A read runs in the look loop, so it delays the look it comes with (one in
  about eight), and that is the look the body follows a face by. If a read
  on the lamp takes over about 100 ms, it moves to its own thread before
  anything turns `--seems` on (the reviewer on #1859).

**The measure (#1671, step 0; the first builder, 20 Oct 16:16).** With
`--seems-log /var/lib/lampos-eyes/seems.log` (beside a reader; that path
only, which both reset plans wipe, so a factory reset never leaves it
behind; any other is refused), each read
is also one line in that file, made 0600: `{"t": unix_s, "kind",
"confidence", "from"}`, nothing of whose face. An existing file that's a
link or open to others is refused. It stops writing at 20,000 lines (about
11 hours of face time with one reader, 5.5 with both: a day at the desk), and says so once, and a write that fails is said once too. The ground truth is a labelled
session at a lamp, with its person's yes: they say how they feel at moments
a builder notes, one line each, `{"t": unix_s, "says":
"positive"|"negative"|"neutral"}`. Nothing of what people say at other
times is matched. `lamp-seems-measure READS LABELS --delete` counts, for
each reader and each confidence from 0.5 to 0.9, the reads that named a kind
within 10 s of a label, and the wrong ones (any named kind is wrong beside a
`neutral` label; `unsure` is never judged, and the reader's own confidence
is written and judged, unrounded). It prints whether each is under the bar,
wrong at most 1 in 10, or "too few" under 20 reads judged, and removes both
files: stop lamp-eyes, or take `--seems-log` off it, first. Only after that may
the conversation be told "they seem low".

**The session's plan (#1671, #2132).** With its person's yes and nobody
else near, both readers on (`--seems`, `--mouth-model`,
`--seems-blendshapes`) with `--seems-log`, and `LAMP_SEEMS_BROW_LOW=on`, so
the brows' long face is measured with the rest. About 3 minutes each, a
label every 5 s from a builder's notes:
1. resting, looking at the lamp: `neutral`;
2. smiling: `positive`;
3. resting again: `neutral`;
4. a long face, as when tired or low: `negative`;
5. **reading or working at the desk, concentrating:** `neutral`. Drawn-down
   brows there must not read `negative` (the first builder's P3 on #2723);
6. talking, as to someone at the desk: `neutral`, the mouth moving.
Then `lamp-seems-measure … --delete`, by reader and confidence.
`tools/seems-session.sh` runs it (the first builder on #2602, 22 Oct
03:56):
- `on`, on the lamp, adds both readers, the log and `LAMP_SEEMS_BROW_LOW`
  to the eyes' own unit, and `LAMP_MOOD_SEEMS` to Mood, through `/run`
  drop-ins, and keeps `mood/state` for the session. Without `FERPLUS`
  (FER+'s model isn't shipped, #811) it runs Blendshape alone, and `read`
  prints Blendshape's rows only.
- `labels`, at the person's keyboard, says each stretch in turn. Space
  labels it, and p, n or u say how they feel instead. No camera frame is
  taken or kept.
- `read` prints each stretch's reads by reader and kind and Mood's smiles
  and quiets, then the measure, which deletes the reads and the labels.
- `off` puts it all back, and does so by itself after an hour.
Only the counts go on GitHub. A screen
face is no stand-in for it: on lamp-75c9 a screen face sits under the 10 px
line and isn't read.

**Only for the session:** it reads whoever is near the lamp, not only the
labelled person, so at a desk it would keep the reads of anyone who stops by
and never said yes. Nothing is read at all without the lamp's kept yes
(`face-expressions.json`, above), so the session needs it too. Turn `--seems-log` on for the labelled session alone,
with nobody else near, and off right after it; don't leave it running to its
cap. Measure and delete it before a lamp is passed on (a reset wipes it too).

Tests (`seems_log.rs`): a read's line is its time, kind, confidence (as
read) and reader only, and 0.695 is not judged at 0.7; the log is 0600,
counts on from its lines when reopened, stops at its most and says so once,
says a failed write once, and refuses a link (`O_NOFOLLOW`) or a file open
to others; under 20 reads judged is "too few"; reads are counted right or wrong against the nearest label
by reader and confidence, an unsure read and one far from any label not at
all, and bad lines are refused. The flag needs a reader (`main.rs`).

The failed-write check uses a real file opened read-only, so the write fails
on macOS as well as Linux. Linux also runs the same checks against `/dev/full`:
the first failure is reported, the next is silent, and no line is counted.
Only the fixture differs; the log's file handling is unchanged.

Tests: a face made grey and 64 by 64; the eight scores made shares;
`positive`, `negative` and `unsure` at the line and just under it; a file
that isn't the pinned model refused; with the model present
(`LAMP_SEEMS_MODEL`), a read in eight shares (`seems.rs`). The read is said
only with the switch, from the eyes, as a kind and a confidence
(`bus_out.rs`); `--seems` is a short absolute path, given once (`main.rs`).
One read takes about 45 ms on an x86 laptop's release build. **The cost on a
lamp is not measured yet**, and it is the first thing to measure.

**FER+ is only for that measure, on our own lamps**, never on a customer's
lamp: it was learnt from web-scraped pictures whose rights aren't cleared
(the lead, 20 Oct 08:16, on the licence table in #1671). The product's
reader is the next one.

### The product's reader: Blendshape V2 (`--seems-blendshapes`, #1671)

Off unless `lamp-eyes --seems-blendshapes
/opt/lampOS/eyes/models/face_blendshapes.onnx` is given, with
`--mouth-model` (the mesh it reads from). Everything said, journaled and
kept is as for `--seems` above: a kind and a confidence, at most every 2 s,
for the nearest face at or over `NEAR_ENOUGH` with its eyes at least 10 px
apart; no name, picture, point or
score. Each read names its reader (`from`: `fer_plus` or `blendshapes`;
a message without it is FER+'s), so with both flags the two run side by
side on the same face for the measure (the first builder, 20 Oct 09:51),
and the journal's once-a-minute line names its reader. The measure's
logger takes the stream and splits it by `from`: `latest` holds only the
newest, whichever reader that was.

- **Off the look, on its own thread** (the reviewer-lead's P2 on #2872).
  With `--seems-blendshapes` the read runs on the `eyes-seems` thread:
  when a read is due the look offers the near face through one slot that
  holds the newest (`latest::worker`). A face not taken yet is dropped for
  a newer one, never queued, and the look never waits. The next look takes
  the read and says it, stamped with when its face was seen, so it lasts
  its 5 s from then. The journal's minute line says the longest is off the
  look.
- **One mesh a look, on the look** (#720; the regression #3158 brought,
  measured on lamp-52e6 by Fifth's #720 row on 23 Oct). The mouth's
  per-look pass runs on the look with or without `--seems-blendshapes`,
  as with `--mouth-model` alone (below), and a read due on the face it
  meshed takes the same points (`blendshapes::input`): the thread then
  runs only the blendshapes. Only a read due on another face, or on a
  look with no mouth pass (`--mouth-on-heard on` and no line heard), sends
  the face's crop (`mesh::crop`), and the thread runs its own mesh on it
  (85 ms under tract on lamp-52e6's big core, 290 ms on a small one, 33
  under `--mouth-runtime`), at most once in 2 s. #3158 had the thread
  mesh every look's crop for the mouth instead and the next look take it:
  on lamp-52e6, in 60 s of talking, the looks fell to 97 from 234 (the
  thread kept a second core meshing back to back while the look ran
  YuNet; nothing in the look waits on it, so the cost is the CPU's), and
  39 of them read the lips (speaking `yes` 39/97, against 234/234 with
  the mesh on the look).
- **One reader a look.** With `--seems` beside it, a look where one reader
  has read leaves the other for the next look.
- **FER+ as before:** a face it can't cut out upright is not read, and its
  clock doesn't start, so the next look tries again.

- **The model:** Google's Blendshape V2 (`face_blendshapes.onnx`, Apache
  2.0), 1,882,583 bytes, pinned by SHA-256 in `blendshapes.rs`, run under
  tract. It comes from the same MediaPipe download as the mouth's mesh
  (`models/blendshapes/SOURCE.md`, made by `training/mouth/make.sh`).
  Install places it at `/opt/lampOS/eyes/models/face_blendshapes.onnx`,
  with the mesh at `face_mesh_v2.onnx` beside it, from the models release
  (`deploy/product/models.json`, 22 Oct). The product's unit passes
  both, `--mouth-model` and `--seems-blendshapes` (#2799), and never
  `--seems` or `--seems-log`: FER+ isn't installed on any lamp.
- **The brows' long face (`LAMP_SEEMS_BROW_LOW=on`, #2132; off unless
  set).** With it, a face is also `negative` with the brows drawn down
  (`browDown`, left and right's mean) at or over 0.80 and the smiles under
  0.3, its confidence the brows' (`blendshapes::judge_with`,
  `BROW_LOW_FROM`, `BROW_LOW_SMILE_UNDER`). It's there because the frowns
  never rise on a long face. Offline, through this pipeline on the screen
  man's clips at 320 across (Ten on #2602, 22 Oct 02:1x), the frowns scored
  0.03 at most on 70 long-face frames, while the brows held a median of
  0.84–0.94 there against 0.63–0.72 on his smiles and resting face. This
  line named 33 of the 70 and none of 50 others. It's one generated face,
  so the line is authored, for the real person's labelled session to set.
  **Drawn-down brows also come with concentration** (the first builder's P3
  on #2723): someone reading or working at the desk the lamp lights, which
  isn't a low mood, and the smiles' brows already sat at 0.63–0.72. So the
  session has a stretch of plain reading and concentrating, labelled
  `neutral` (below), and the switch goes on only if those stay under the
  bar.
- **A far face gets no read** (Ten on #2602, 22 Oct 02:4x). One smile frame
  of a screen face, shrunk, reads smile 0.97 with the eyes 14.2 px apart,
  0.87 at 9.9 px and 0.18 at 7.0 px (`unsure`). On lamp-75c9 a screen
  face at 0.07–0.10 of the picture, 7–9 px between the eyes, read
  `unsure` 105 times in 106. An `unsure` from a face too small to read
  would break Mood's run of reads, so under 10 px there is no read at all.
- **The read:** the mesh's 478 landmarks for that face, from the thread's
  own mesh pass on the crop (`Mesh::landmarks_of`). The 146 points MediaPipe's own graph takes
  (`blendshapes::POINTS`) are turned back by the eye line's angle to the
  frame's (`mesh::eye_line`), since the crop is turned so the eyes are
  level and the model minds a turn but not where the face sits or how
  large it is. The model scores 52 movements of the face.
- **Movements, not feelings:** the reduction is ours (`blendshapes::judge`,
  `NAMED_FROM` 0.5, authored, to be set from the measure). `positive` when
  the two smiles' mean is at or over the line and over the other;
  `negative` when the frowns' or the wrinkled nose's is; else `unsure`.
  The brows drawn down aren't counted: on MediaPipe's own smiling test
  portrait they score 0.84, beside the smiles' 0.95.
- **Checked against MediaPipe** (on a laptop, not the lamp): the converted
  model gives TFLite's 52 scores to 1.4e-6 (`blendshapes_parity.py`).
  MediaPipe's own points through this pick give its own scores. On its two
  test portraits, this mesh's crop and the turn back give MediaPipe's scores
  to within 0.07 on the upright one (the smiles 0.95 and 0.93 against its
  0.96 and 0.93) and 0.35 on the one turned a quarter, where MediaPipe's
  own scores differ from its upright ones by as much. Without the turn
  back, the turned one is 0.57 off.
- **The cost:** the model takes about a sixth of a mesh pass (0.8 ms
  against 5.1 on an x86 laptop's one thread). On lamp-52e6 a mesh pass was
  85 ms under tract and 33 ms under ONNX Runtime (#720). So a read is about
  one mesh pass every 2 s, on its own thread. **Not measured on a lamp
  yet.**

Tests: a smile `positive`, a frown or a sneer `negative`, one side only or
under the line `unsure`, exactly on the line named, a smile beside a
stronger frown `negative`, MediaPipe's portrait's smile with the brows
drawn down `positive`; scores that aren't 52 numbers give nothing; the 146
points picked in order and turned by the eye line; a file that isn't the
pinned model refused; with the model present, 52 scores from 0 to 1 that
match ONNX Runtime's (`LAMP_BLENDSHAPES_GRID`, `blendshapes.rs`). The flag
is off unless given, needs `--mouth-model`,
and is a short absolute path, given once (`main.rs`); `--seems` beside
it runs both. A read says its reader, a message without `from` reads as
FER+'s, and a read is a kind, a confidence and its reader only
(`msgs/seems.rs`, `bus_out.rs`). No yes, no read; a yes, a read; before
its time the file isn't asked; the yes file is read as the ears' room sounds
yes is (`a_face_is_read_only_with_the_persons_kept_yes`, `seems.rs`). A
reader that takes 300 ms doesn't slow the offers, which stay under 50 ms,
and of the crops offered while it reads only the newest is read
(`a_busy_worker_never_holds_up_the_offer_and_reads_only_the_newest`,
`latest.rs`).

### Whether anyone is at the lamp (`LAMP_PRESENCE=on`, #1677)

With `LAMP_PRESENCE=on` beside `LAMP_BUS=on`, the eyes also say on
`room/presence` (`lamp_msgs::RoomPresence`) whether anyone is at the lamp:
`someone` while a face was seen or a voice heard within 20 s, else
`nobody_s`, how long nobody has been there. Once a second while they look
and at once when `someone` changes, each good for 3 s
(`crates/eyes/src/away.rs`, said from `bus_out.rs`'s turn of the loop, so
`main.rs` is unchanged). A face is any face of a look as published on
`humans/faces/tracked`; a voice is any line on `voice/heard`, which the eyes
subscribe to as `eyes_presence`, timed by when the voice heard it. With no
look for 5 s nothing is said, so a closed camera never reads as nobody, and
the count begins again when looks come back. While someone is there it also
says `by`, what last showed it (`face` or `voice`), and writes one journal
line at each change to someone, with a face's height as a share of the
picture. It is the one signal the light
going out and Messages read ([presence.md](presence.md)). A presence switch
whose subscription fails is said once in the journal, and the faces are
published as before. Not run on a lamp: its cost there is part of
presence.md's lamp check.

### What the person at the desk is doing (`LAMP_ACTIVITY=on`, #1678)

With `LAMP_ACTIVITY=on` beside `LAMP_BUS=on`, the eyes also say on
`room/activity` (`lamp_msgs::RoomActivity`) what the face at the desk shows,
and on `room/stood_up` (`lamp_msgs::StoodUp`, an event) when someone rose
out of the chair and left (`crates/eyes/src/activity.rs`, from
`bus_out.rs`). It says the side of the picture they left by (`side`, `left`
or `right`) when their face was last seen less than 0.35 of the picture's
width from that edge as they rose, and no side nearer the middle: for
watching them go (#1669), a turn toward it is the one the gaze would make
to a face seen there (body.md, `toward`). 0.35 is provisional, until the
arm hour sets it. The eyes see a face, not a body, so each state is read from
the sitter's face against this person's own sitting: the median of its
first two minutes of looks at the desk (a face at least 0.14 of the picture
tall, Stretch break's `near`). The sitter is the largest near face when a
sitting starts, and from then on the near face nearest their last place,
within a face's height of it or of where they sat, whoever is larger: in an
office a colleague near the desk is often the largest face. Head down is
the nose lower between the eyes and mouth by 0.12 than it sat, held 20 s
(reading, writing or a phone held low, not told apart), whatever the face's
size, since a head bowed over the desk comes closer too; leaning in is the
face 1.25 times as tall, held 10 s. No sitter for 20 s is away; a sitting
goes on through a gap under two minutes. Rising out of the chair is the sitter's face, over
its looks in the 2 s before it was lost, risen by half its height or more,
or shrunk to 0.6 of it or less, then not seen again for 2 s. Which one shows
depends on the lamp's head. On lamp-52e6, resting with its head low, the
lead stood straight up and the face rose 1.90 of its height at 1.02 as tall,
out of the picture's top (row 14 try 4, 20 Oct). With its head up, the face
fell 0.28 and shrank to 0.47 (try 5). On lamp-75c9 it rose 0.84 and grew to
1.30 (Nine, #1678). A head turned while sitting shrank to 0.84 at most, and
leaning back sits at 0.68 or more. Before, both a rise and a shrink to 0.85
were needed, and no standing up was ever said on either lamp. A face must
stay near (0.14 tall) to be followed, so on a lamp whose sitter's face is
under 0.23 tall, the shrink alone can't be seen and only the rise counts. Once a loss,
the journal says how those looks moved against that, numbers only ("sitter's
face lost: over its last N looks it rose R of its height … and was S as
tall …, last A across, H tall: rising" or "not standing up"), so a run that
saw someone leave with no `room/stood_up` shows which part was missing
(row 14 on lamp-52e6, 20 Oct).

At lamp-75c9 (19 Oct, one person, an open office, the lamp beside the
laptop at about face height) leaning in was seen (1.52 times as tall). Head
down, as first written (the centre a quarter of the face lower), was not:
the face came closer (1.21 times) and only 0.06 of its height lower, so it
is the pitch alone now, and 0.12 is the design's line until a run sets it.
Leaning back (0.99 times) and sitting low (0.15 of the face lower against
half) could not be seen from that place and were dropped; they come back
only if a lamp in front of a person shows them. While a sitting with its
baseline is followed the journal says every 2 s `lamp-eyes: sitter 1.04 as
tall as sat, 0.03 of its height lower, pitch 0.52 (sat 0.50), AtDesk`, numbers
only, for the next run. Once no look has had the sitter for 2 s, until Away
20 s on, it says `lamp-eyes: sitter not seen for 14 s, AtDesk` instead of
repeating the last look, which read as a face held still (lamp-75c9, 20 Oct,
#1678). By place alone, someone else in the sitter's own
place, at their distance, is taken for them (the run's leave step: faces
side-on at 0.40 to 0.53 across, where the sitter had sat at 0.44).

**Told by face, with the recogniser** (#1678's (d); the owner's yes, 19 Oct
15:57, recorded on #1827; switched on on a lamp only after Third's privacy
read, lamp-4ace first). Only with `LAMP_ACTIVITY=on`: `LAMP_ACTIVITY=place`,
the product's since #2834, says the same topics with the sitter followed by
place alone, and never takes a print, though the eyes have `--recogniser`
for other reasons.
When the eyes also have `--recogniser` (the pinned SFace), the sitter is
told from others by face. In the sitting's first minutes one print of the
sitter is taken every 20 s, up to six (about a second of SFace), and their
mean is the sitter's. After that a print is taken only of a face back at
the sitter's place after 2 s or more without them, at most one every 2 s:
alike enough (`who::SAME_PERSON`, 0.363) it is the sitter, the same
sitting; less, or a face that can't be printed (side-on), it is not, and
`away` and `room/stood_up` go on as if nobody were there. Followed without
such a gap the sitter is never printed again, so turning their head does
not lose them. The sitter's prints are held in the eyes' memory for that
sitting only and dropped when it ends (no sitter for two minutes, whether
or not the eyes are looking) or when the camera closes (not permitted, a
failure, or a reopen at the full size): never
written, never published, never compared with `people.json` and never
named. The journal says only what was decided, with a number: `lamp-eyes: a
face at the sitter's place, likeness 0.21: not the sitter`. Without the
recogniser it is by place alone, as above. Standing up is
the face rising by half its height or shrinking to 0.6 of it within 2 s,
then not seen for 2 s, said once. Nothing is said with no look for 5 s.
States and counts only on the bus: no box, no picture, nobody's identity.
Arithmetic on the looks the eyes already make, and with the recogniser the
few prints above, on frames the eyes already have: no model or frame is
added. Its cost on a lamp is #1678's CPU run; how well the states are told
apart is its scripted sitting.

#### A sitting's yawns (`LAMP_EYES_YAWN=on`, #3709)

With `LAMP_EYES_YAWN=on`, `--mouth-model` and `--seems-blendshapes` (as
the product runs), the eyes count the largest face's yawns into the
sitting, said as `yawns` on `room/activity` (left out at 0). This is the
largest face's count, whoever that is: a visitor's yawn goes into the
sitting's count too. It is a guess at a yawn from the mouth's shape, not a
reading of how tired anyone is.

**The yes:** they count only while the face-expressions yes that gates
`humans/seems` is kept, checked at most every 2 s. When the yes is
withdrawn, the sitting's count goes to 0 at once, and no yawn counts until
the yes is back. Because of that 2 s check, a withdrawal and a new yes
inside one 2 s window are never seen as a withdrawal, and a yawn counted in
that window still counts (Fourteen's P3 on #3898). A closed camera forgets the count too, as it forgets the
sitter's prints. Each look offered to the yawn thread carries
what its frame was captured under: the permission's own hold
(`Permit::held`, another at every close and every permission after one ran
out, counted on the permit's thread), the yes's epoch (moved on at every
look without it) and the frame's time (`yawn::Grant`). A hold begun under
another starts afresh. A count is taken only under the hold and the epoch
the look has now, from a frame at most 2 s old (`yawn::current_counts`), so
one finished after a close and a regrant is dropped (Fourth and Fourteen
on #3898). Without either model the journal says so, and no yawn is
counted.

**What counts as a yawn** (`yawn.rs`): the mouth held at `mouth_open`
0.08 or wider for 1.5 s, over at least four looks with no more than one
misread. On those looks the smiles' mean must be under 0.3 and the inner
brows raised under 0.5. Once a yawn is counted, the next can count only
after the mouth has closed (under 0.03), the face has gone, or a second has
passed with no look.

**How the blendshapes are read:** from the look's own mesh points of that
face, only while its mouth is open that wide. The read runs on a thread of
its own, `eyes-yawn`, fed through a one-item slot as the seems read is
(`latest.rs`), so the look never waits for it. A look it hasn't taken yet
is replaced by a newer one. No mesh is added, and the seems thread's 2 s
pace is untouched. Its cost on a lamp's small core is not measured yet.

**Why not the gap alone:** on the screen man's generated clips, at four
looks a second, a broad open smile held 0.18 to 0.20 for 5 s, and surprise
held 0.10 to 0.14. Of the looks open 0.08 or wider, 29 of his yawn's 30
passed both scores, and none of the smile's 61, the surprise's 30 or his
talk's 3 did. His yawns stayed open only 1 to 1.5 s, shorter than a
person's. These lines come from one generated face on a screen. Real faces
laugh, sing, eat and say "oh" with the mouth wide open, and a camera with a
different look pace changes what four looks mean. So the lines are to be
measured on a few people at a lamp before any unit turns this on.

**What is kept:** a count only. No picture, score or time is kept, and
nothing goes to the journal for each look.

### Whether a face is speaking (`LAMP_EYES_SPEAKING=on`, #720)

Off unless `LAMP_EYES_SPEAKING=on` is in the eyes' unit beside `LAMP_BUS=on`.
Off, nothing is computed and `humans/faces/tracked` is byte for byte as
before. On, each face also has `speaking`:
`{"state":"yes"|"no"|"unknown","score":0.03,"from":"corners"|"mesh"}`
([`speaking.rs`](../crates/eyes/src/speaking.rs)). It is read from the five
landmarks YuNet already gives: a few additions a face a look, no new model,
and no picture is kept or leaves the lamp. `from` says what judged it, and
the score is on that one's scale: the corners' in eye distances a look
(`yes` from 0.02), the mesh's in inner-lip gap over face height (`yes` from
0.015, below). The second route reads it with
`LAMP_SECOND_SPEAKING_FACE=on` ([memory.md](memory.md#how-the-voice-uses-it)),
with the known face's id the eyes keep beside the known file
([Who is there](#who-is-there)).

- **The shape:** the mouth's width over the distance between the eyes.
  Moving, coming closer, turning and tilting scale both alike. A nod
  shortens only what runs up and down, and both of these run across, so it
  leaves the width alone too.
- **Not used: the mouth's drop below the eye line.** A nod shortens it by
  cos(pitch) just as an opening jaw lengthens it. The nose tip, which stands
  out from the face, barely moves under a nod at the tilt of someone looking
  down at a lamp (about 25 degrees), so it can't tell the two apart.
  Measured before this was dropped: a silent face tilted 20 degrees, nodding
  12 degrees, read `yes` in 36 to 78% of looks (#1385 review).
- **The score:** the root mean square of the width's change from one look
  to the next over the last 2 s (at least 5 looks), less what the marks'
  own jitter explains. The jitter is read from how much the eye line bends
  from look to look (turning, nodding and coming closer leave its angle as
  it is, and speaking does not move the eyes), averaged over about the
  face's last ten looks. `yes` from 0.02 eye distances a look.
- **`unknown`, never `no`,** while the face has fewer than 5 looks in the
  window, its eyes are under 24 detector pixels apart on average, it is
  turned more than 30 degrees or its turn was not measured (see above), or
  the eye line bends more than 0.1 radians a look.
- **One misread look (`LAMP_EYES_SPEAKING_DROP_OUTLIER=on`, off unless
  set, #3296).** Off, one look whose landmarks misread (on lamp-52e6 a look
  at 14 px between the eyes among 28 to 33 px, its eye line bent) trips the
  jitter gate or leaves the window without the mesh, and the face reads
  `unknown` for the whole 2 s while the mouth was plainly open and shutting.
  On, a look whose eye distance is 30% (`OUTLIER_SHARE`) or more off its
  window's median is left out, and the rest is judged as above, with the same
  gates: still at least 5 looks, every one with the mesh for the mesh to
  judge. The look it leaves out is a gap: the change from look to look and
  the eye line's bend are measured only between looks that were next to each
  other, so the two looks around it are never joined as if 500 ms were one
  250 ms step. A window with more than one such look (`MOST_OUTLIERS`) is
  `unknown` (`reason` `outliers`, after `small`: a far face reads `small`). A window with none is judged exactly as
  with it off. Offline only: a talking face with one 14 px look reads `yes`,
  a still one `no`; two such looks, or one in a window of five, `unknown`.
- **The pace:** the eyes look every 250 ms while they follow a face, up to
  350 ms on the 10 fps camera (`main.rs`), so 2 s holds 5 to 8 looks. With
  `--steady-rate` a still face is looked at 500 to 600 ms apart, 4 looks a
  window, under the 5 the score needs, so in practice `unknown` (below).
- **How fast it answers** (looks every 250 to 350 ms): `yes` on the first
  look after talking starts (0.06 to 0.18 s), and held 1.3 to 1.8 s after
  it stops, the 2 s window emptying.

**What it rests on, and what it can't do.** YuNet has no lip points, so
only the corners carry the mouth: they move in as the jaw opens and in or
out as the lips round. #730 showed why pixel differences in the mouth box
fail (head motion and camera noise were as large as the lips at 1 m, five
runs on lamp-52e6). Tested offline only. The synthetic faces are five
points of a head in millimetres, seen every 250 to 350 ms at random. The
mouth is no pure tone: syllables at 3 to 6.5 Hz, louder and softer, lips
rounding near 3 Hz. At 60 px between the eyes and 0.3 px of jitter, each of
these is right in at least 90% of looks: still; nodding 12 degrees; the
same nod at a 20 degree tilt; turning 20 degrees side to side; coming
closer while drifting; talking; talking while turning 10 degrees, nodding
5 and drifting. A face too small, turned 55 degrees or jittering 5 px is
`unknown`. Talking while turning 20 and nodding 10 at once reads `yes` in
only about half its looks, since a turned head's nod tips the eye line.

The eye-distance division is what keeps a face coming closer from reading
as speaking. With a fixed divisor in the same units, that test fails (`no`
in 0% of looks), and so does turning (47%). The still and nodding faces
pass either way.

The margin is the marks' jitter against the face's size (`cargo test -p
lamp-eyes speaking::tests::margins -- --nocapture` asserts it; share of
looks right, still / talking):

| Looks every | Jitter | Eyes 24 px | 30 px | 40 px | 60 px |
|---|---|---|---|---|---|
| 250–350 ms | 0.3 px | 0.46 / 0.55 | 0.86 / 0.94 | 0.90 / 0.96 | 0.97 / 0.98 |
| 250–350 ms | 0.5 px | 0.27 / 0.45 | 0.66 / 0.80 | 0.79 / 0.92 | 0.90 / 0.95 |
| 250–350 ms | 1 px | 0.01 / 0.24 | 0.15 / 0.41 | 0.33 / 0.57 | 0.66 / 0.80 |
| 500–600 ms | 0.3 to 1 px | `unknown` | `unknown` | `unknown` | `unknown` |

A face at 1 m fills about 25 pixels of mouth in the detector's 320-wide
picture, so its eyes are about 27 pixels apart. On this model, that is too
small at any jitter. The signal holds only for a near face (60 px, about
0.45 m) with steady marks, and not under `--steady-rate`. **What a lamp
still has to show,** numbers only: YuNet's corner jitter on a still face at
0.5 and 1 m, and the score for a person silent, talking, nodding and
turning. If 1 m matters, the next step is a landmark model with inner-lip
points run on the tracked face box, as #720's comments propose. Its model
is chosen and converted (Face Mesh V2, `models/mouth/SOURCE.md`).

**Measuring the mouth with Face Mesh V2 (`--mouth-model PATH`, none unless
given, #720).** Install places the pinned model at
`/opt/lampOS/eyes/models/face_mesh_v2.onnx` (`deploy/product/models.json`),
and the product's unit passes it, for Blendshape V2 (#2799), whose mesh
runs on the look as with it alone, one mesh a look whose points a
Blendshape V2 read due on that face takes (above). The largest face in each look is cut out of the
same 320 by 240 look: its box, widened by a quarter on every side and
turned so the eyes are level, made 256 pixels square (`src/mesh.rs`).
With `LAMP_EYES_MOUTH_FULL_LOOK=on` and `--full-look` (the product's 1280
by 960) it is cut from the frame at its own size instead
(`mesh::crop_full`). The switch stays off in the product (#3764, closed):
on lamp-52e6 (27 Oct) a real person sitting silent at the desk read 31
`yes` of 372 looks, and one off-camera line was filed as theirs (a false
`yes` gives a silent known person another's line, #3679). Lips against the
room's level (`ears/level`) did not tell a face's own voice from another's
either: a look with a moving mouth to mesh takes about 0.4 to 0.5 s there,
so the eyes reach about 2 looks a second with a talking face (asked for 10,
with the full frame or the 320 crop alike), too few for the lips' rhythm.
Who a line belongs to rests on the voice's own evidence instead. The detector's box and marks are
scaled back to the frame's pixels, as `LAMP_FACE_FULL_CROP=on` does for a
print (`who::align_full`), and the frame is the one the look's picture was
shrunk from, kept for the look, never a second decode. On lamp-52e6 a face
seated at the desk, about 1 m away, has a box 0.20 to 0.25 of the height,
48 to 60 of the picture's rows, its eyes 18 to 24 pixels apart: from the
picture every look was `unknown` (`small`) and the voice never saw who
spoke; from the frame the mesh sees four times as many pixels. A Blendshape
V2 read takes the same look's points, or the same full-frame crop for
another face: its points are in the 256-square crop's pixels and turned back
by the eye line's angle, whatever the crop was cut from.
The mesh reads it under tract, and with `--mouth-diagnostics on` (never in
the product: it is a line a look, up to four a second) the journal says
`lamp-eyes: mouth open 0.0123 eyes 31 px (124 px meshed) mesh 84 ms`. That
is the inner lips' gap (points 13 and 14) over the face's height (10 to
152), the eye distance in detector pixels and in the pixels the mesh's crop
was cut from (the same without the switch), and the time. No picture is kept.

With `LAMP_EYES_SPEAKING=on` as well, that face's `speaking` reads the
mesh instead of the corners whenever every look in its 2 s window had the
mesh read: the score is how much `mouth_open` changes from look to look
(root mean square), and `yes` from 0.015 (`MESH_SPEAKING_AT`; 0.012 until
#3699, where on lamp-52e6 a still screen face cut from the 1280 by 960 look
scored at most 0.0123 and a talking one over 0.015 in 237 of 257 looks).
Every other face, and a window with one look the mesh missed, is judged by
the corners as above, with the same `unknown` gates, except that a window
the mesh read on some looks never says `yes` by the corners: their score at
or over 0.02 there is `unknown` (on 52e6 a still face's corners scored 0.08
across such a gap). On a recorded talking face cut as
the lamp sees it (NASA's public-domain interview SVS14704, 320 by 240, 4
looks a second, run through the real `Detector`, `Mesh` and `Mouths`), the
mesh said `yes` in 98% of the talking looks at 26 px between the eyes and
94% at 51 px; the corners said `yes` in 16% at either, where the synthetic
faces gave them 98% at 60 px. The clip has no silent face, so `no` is not
shown there: how far the gap moves on a still, silent face on the lamp's
camera, at 0.5 and 1 m, is the next run.

**Every face's mouth (`LAMP_EYES_SPEAKING_ALL_FACES=on`, off unless set).**
Off, only the largest face is meshed, so another face's `speaking` is the
corners' and a colleague talking beside the one at the lamp is not read
from the lips. On, up to three faces a look (`MOST_MESHED_FACES`,
`src/mesh.rs`), largest first, are meshed, each from its own box, and
each face's mouth goes to its own passing id's window, as every face's
corners already do. A face after the largest is meshed only if the look's
meshing so far plus the last mesh's time stays within 200 ms
(`EXTRA_MESH_BUDGET_MS`): on lamp-52e6 that is two faces under tract on a
big core (85 ms each), three under ONNX Runtime (33 ms each), and the
largest only on a small core (290 ms). The look's pace stays at 250 ms
while it follows a face, so a 2 s window keeps its five looks. Faces are
not meshed in turn across looks: a window reads the mesh only when every
one of its looks has it, so a face meshed every other look would be judged
on the corners all the same. A face the budget skips on a look is judged
on the corners until its window is all mesh again. The voice reads every
face's `speaking` on `humans/faces/tracked` (`crates/voice/src/speaking_faces.rs`),
so a second face's `yes` is a face seen speaking with nothing more. The
cost is one more mesh a look while two faces are in view (with
`--mouth-on-heard on`, only while a line is heard). Not yet run on a lamp.

**Only while the voice hears a line (`--mouth-on-heard on`, off unless
given, with `--mouth-model`).** lamp-eyes subscribes to `voice/heard` and
runs the mesh from a line's first words until 2 s after it is finished
(the speaking judge's window), or 15 s after its last words if its end
never comes (`mesh::Heard`). Otherwise the mesh is not run, and the corners
judge as before. A quiet room, or one with faces and nobody speaking, costs
the mesh nothing. The first look or two of a line are judged on the corners,
since the words reach the bus after they start; a line is timed by when
the voice heard it (its header's `at_us`), so lines that waited while the
camera was closed count as old as they are. It subscribes on the bus's
root (`LAMP_BUS_ROOT`, or `/run/lampos-bus`) and lamp-eyes does not start
if it cannot. Monday's silent-face measurement runs without it, so a
silent face is read.

**Explaining an unknown mouth (`--mouth-diagnostics on`, off by default,
#720).** This is an observation-only CLI option. It requires an already usable
`LAMP_BUS=on` and `LAMP_EYES_SPEAKING=on`; missing prerequisites fail before
model loading or camera setup. It does not enable a model, change a threshold,
read another frame, or change any `humans/faces/tracked` payload. No installed
unit enables it. Existing look-timing lines remain unchanged.

One `lamp-eyes: mouth diagnostic {JSON}` line explains the largest detection
per existing look (normally at most four per second). It uses the same
`Mouths::judge` state transition and gates as publication, not a second scorer.
The fixed record has monotonic `at_us`, `filled` width/height, `face_box_px`
(x, y, width, height), `face_count`, unrounded `eyes_px`,
`window_mean_eyes_px`, `window_looks`, `window_span_us`, `turn_deg`,
`jitter_rad`, `reason`, `mesh_current`, `mesh_window_looks`, `mouth_open`,
`window_reset`, and `speaking` (state, score, optional source). It contains no
track/person ID, name, landmarks, image, audio or transcript. No record is
emitted without a valid largest box or without the already enabled judge. If
that tallest box is clipped to zero area, it does not substitute a smaller face.

Coordinates and the 24 px gate are in **working-detector pixels**, not the
mesh crop's 256-square coordinates, with one exception: with the mouths cut
from the full look (`LAMP_EYES_MOUTH_FULL_LOOK=on`),
a window the mesh read on every look holds its mean eye distance to 24 px in
the frame's own pixels (`Mouths::set_mesh_scale`, four a detector pixel at
1280 by 960), since those are the pixels the mesh saw, and
`window_mean_eyes_px` says that value; `eyes_px` stays the detector's. A
window with a look the mesh missed is judged by the corners, on the
detector's marks, so it is gated in the detector's pixels as before. Without
the switch or `--full-look` nothing changes. `filled` excludes padding; the existing
camera-open line gives capture dimensions. The frame is shrunk uniformly to
fit 320 by 256, never enlarged; a 320 by 240 capture adds sixteen black rows,
without stretching. Mesh crop upsampling does not change `eyes_px`. The box
helps interpret framing but does not prove identity or physical distance.

`reason` names the first returning branch: `invalid_eyes`, `turn_unavailable`,
`turned`, `small`, `outliers` (with `LAMP_EYES_SPEAKING_DROP_OUTLIER=on`
only), `too_few_looks`, `jitter`, `mesh`, or `corners`. With that switch,
`window_looks` and `window_span_us` count the whole window, while
`window_mean_eyes_px` and `mesh_window_looks` count the looks kept. `mesh` and
`corners` identify the scoring path, not necessarily speech. Later unevaluated
metrics are null, including window count/span/mesh count after `invalid_eyes`;
zero is a measured value. `window_reset=true` means this valid look starts a
new window (first sight, a new passing track, or all earlier looks expired).
It does not mean every internal field was reset. An invalid look reports false
and does not fabricate an empty history. `mesh_current` describes this input;
`mesh_window_looks` counts available mesh values in the actual rolling window.

Logging uses the existing nonblocking log relay; records may be dropped under
backpressure. `at_us` is judgement time, not capture time, and missing records
must not be treated as silent faces. Target host overhead is below 1 ms per
look for the decision and formatting, with no added inference/frame read.
The repeatable host-only report is
`cargo test -p lamp-eyes --lib mouth_diagnostic_cost_report -- --ignored --nocapture`.
It compares one bounded synthetic stream with diagnostics off/on, including
bus-payload and diagnostic JSON formatting, without a fragile timing assertion.
It excludes inference and journal delivery. One macOS ARM64 host run with
Rust 1.96, unoptimized profile, debug info and incremental compilation off,
measured 14.35 microseconds/look off and 36.33 on over 10,000 looks (about
22 microseconds extra). This meets the host target only; real-device overhead
and capture-to-decision latency remain unmeasured.

**A closing camera waits for the mesh.** With `--mouth-model`, a
`{"camera":false}` that arrives mid-look closes the camera once that look's
mesh is done: up to 85 ms on a big core and 290 ms on a small one under
tract, 33 and 154 under ONNX Runtime.

**Under ONNX Runtime (`--mouth-runtime PATH`, with `--mouth-model`).** The
mesh runs on one thread under the same pinned `libonnxruntime.so.1.30.0`
the voice's turn-end worker loads, its size and SHA-256 checked against
that worker's pin (`lamp_turn_end::ONNX_RUNTIME`) before it is loaded: 33
ms a face on lamp-52e6's big core against tract's 85 (#720, 10:3x). The
same crop gives the same `mouth_open` under both, to 1.5e-7 on the Mac
(`onnx_runtime_reads_the_mouth_as_tract_does`, ignored without the
library). Without it the mesh runs under tract.
The file is checked by size and SHA-256 before it is parsed. It adds the
mesh's time, 85 ms on a big core under tract on lamp-52e6, to every look
with a face in it.

Once it can be trusted, the voice's "spoken to" judging is to read it
(`LAMP_SPOKEN_TO=on`, `crates/voice/src/attention.rs`, #829). A line heard
while the only face turned to the lamp reads `no` is not for it, and a face
that reads `yes` with the voice's speech is the one speaking.

### Turning to the one it answers (`LAMP_TURN_TO_ANSWERED=on`, #1810)

Off unless `LAMP_TURN_TO_ANSWERED=on` is in the eyes' unit beside
`LAMP_BUS=on` and `LAMP_EYES_SPEAKING=on`; off, the body is sent the largest
near face as before. With the bus off, the journal says it is on without the
bus and nothing changes; with `LAMP_EYES_SPEAKING` off, the journal says no
face reads speaking, so the lamp turns to nobody. On ([`answered.rs`](../crates/eyes/src/answered.rs)):

- **Who:** each look, the passing ids whose `speaking` read `yes` are kept
  for 10 s, in memory only, and none while an answer is under way; each
  pick starts the count again, so neither the last asker nor someone who
  talked over the last answer counts for the next. As the voice begins an answer (`voice/state`
  turns `speaking` on the bus, read at each look, and no older than 3 s),
  the one face that read `yes` in at least 3 of those looks is the one
  answered, and the journal says `lamp-eyes: the answer turns to f3, the
  face seen asking`. Two faces that did, or none, and nothing changes: the
  lamp turns to nobody rather than to the wrong one.
- **Once:** it is picked as the answer begins and kept until the answer
  ends. Someone else talking over the answer does not take it. While that
  face is in view and near (`sight::LEAST_SIZE`), it is the face sent on
  `--body-socket`. While its passing id is not in view (it is out of view,
  or came back under a new id after 1.5 s unseen), nothing is sent, so the
  body keeps its last aim: the largest near face is not taken. An asker
  back under a new id is not turned to again in this answer. A pause inside the turn (`thinking`, as between a filler and the
  answer) keeps it, and the second `speaking` picks nothing again. When the
  voice listens or rests, or says nothing for 3 s, the choice is let go.
- **How it moves:** nothing new in the body. Its gaze ([body.md](body.md))
  takes the face in small corrections with stillness between them, and while
  the lamp speaks it leaves a face within 0.35 across and 0.40 down alone,
  so a head already near the person barely moves. It never searches.
- **Never over a hand:** the body never moves a joint that met resistance
  for a gaze; it is left where it is for the whole ease
  (`a_base_turning_to_a_face_that_meets_a_hand_is_left_where_it_is`).

**What a lamp still has to show:** two people at the lamp, one asking. The
`speaking` it rests on holds only for a near face (above), so the asker sits
within about half a metre. Tested on the host only.

## Running it

```text
lamp-eyes --lock-directory /home/WORKER/private-camera-locks --usb 01da:5875@3-1.1.4 --model /opt/lampOS/eyes/face_detection_yunet_2023mar.onnx --permit-socket /run/lampos-eyes/permit.sock --body-socket /run/lampos-body/gaze.sock
```

Every option shown is required; `--sight-file ABSOLUTE_PATH`, `--leave-still-faces on`, `--steady-rate on`, `--look-on-change on` (with `--glance-log ABSOLUTE_PATH`) and `--full-look 1280x960 --full-look-file /run/lampos-eyes/full-look.jpg` (up to `3264x2448`) may be added, the last with `--full-look-movement on`, and `--mouth-model ABSOLUTE_PATH` (with `--mouth-on-heard on` and `--mouth-runtime ABSOLUTE_PATH`). `--mouth-diagnostics on` observes the already enabled speaking judge as described above.
`--usb` is `VENDOR:PRODUCT@TOPOLOGY`, the two numbers as four hex digits; or
`VENDOR:PRODUCT` alone (#2190). `--device` is given only with a port, and
may be left out. Without it, at each open the eyes take the one camera with
those ids, at the port named (#3504) or else on whatever port it is on, its
capture node found by following each `/sys/class/video4linux` node up to its
USB device (`lamp_camera::linux::find_camera`), and open it as before, port
checked. None, or two or more (their ports named, so one can be given), is
refused and said once in the journal ("camera 01da:5875 not opened: no
camera with those ids at port 3-1.1.4", say), and tried again every five
seconds; with a port named, a camera at another port is never taken instead.
"camera found at /dev/videoN on port P" is said when it is found, or found
on another port. **The product unit** gives `--usb 01da:5875` with no port
and no `--device` (#4074), and its helper the same ids, no port either
(below; the install test holds both): the camera's port differs by unit, `3-1.1.4` on
lamp-4ace and lamp-52e6 but `3-1.3.4` on lamp-a63e and lamp-dbda, and the
hardware can't change that. Every lamp-eyes since 20 Oct (#2190) takes
`--usb` with the ids alone, so an older binary beside this unit opens the
camera too; a second camera with the same ids is refused, never guessed. The per-lamp units (`deploy/units/`, lamp-4ace,
lamp-52e6) still give `--device` with the port, checked at open as before. The
lock directory must be private to the user it runs as. A camera that does not open, or fails, is tried again every five
seconds while permission lasts. A start is given a 2 s budget (`OPEN_BUDGET_US`)
and a close 250 ms, so a camera slow to answer after a close is opened rather
than given up as late (#911). These are budgets the caller checks after each
camera call returns, not time limits: a call blocked in the driver is not
interrupted, and one that returns past the budget is reported as expired. A
close always closes the file, but releases the buffers first only when the
stream stopped cleanly; after a failed or late stop it skips the release and
relies on the close. The eyes do not look at the stop's result. An open can
outlast the 1.5 s permission, or the permission can be taken back while it
blocks: so the permission is checked again as soon as the open returns, and a
camera opened without it is closed at once, before any frame is read, kept or
published. A blocked open is not interrupted; it is closed when it returns.
The permit socket is heard all the while by a thread of its own
(`crates/eyes/src/permit.rs`), and each `{"camera":true}` counts from when it
arrived, not from when the eyes read it: a renewal that came early in a long
open and was the last one has run out by the time the open returns, and does
not keep the camera open. Each open and each failed one is logged
with how long it took: about 50 ms on lamp-52e6.

To look at one JPEG file without a camera:

```sh
cargo run --release -p lamp-eyes --example look -- MODEL.onnx PHOTO.jpg
```

## Verification without a lamp

Host tests, `cargo test -p lamp-eyes` (14 passed, 0 failed, on macOS; the 12
before the kept frame also passed under emulated ARM64). They cover: that the
kept file is the newest frame, private, never a second file, left alone by a
frame that is cut, has no tables or is too large, and gone when forgotten or
dropped; a 320 by 240 frame keeps every pixel in
blue, green, red order with the spare rows black, and a larger one is shrunk
to fit; what is not a frame or not a JPEG is refused; a detection becomes a
box where its cell is, and weak, duplicate and senseless detections are
dropped; outputs of another shape are refused; a file that is not the pinned
model is refused; the largest near face is chosen and a small one alone is
not; the datagram's exact text; the command line and the permission message.

By hand, with the real model on a Mac (`look`): a 1920 by 1080 photo of a
person a metre from the camera gave one face, `x` 0.05, `y` -0.43, `size`
0.41, `turned` 0.04; two photos of the lamp in an empty office gave none. A
look took 29 to 38 ms there, a third of it decoding the large JPEG.

Not shown by any of this: how long a look takes on the lamp's processor; what
the lamp's camera sees from its head, how wide, and how well in office light;
whether a face at conversation distance is 15% of that picture, the line
V1 measured between colleagues' faces (8.3 to 13.6%) and its user's (19.2
to 46%) and V2 took on 2026-10-14 without anyone in front of this lamp; how the
detector fares with a face lit from behind, in profile or with glasses, where
the old runtime's notes say it struggles; anything about the camera's lock and
identity check on the lamp itself.
