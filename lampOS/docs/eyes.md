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
  looks half as often (only with `--steady-rate on`).
- It never moves anything and decides nothing about the conversation. It
  recognises a face only for someone who asked the lamp to remember them
  ("Who is there", below), and nobody else.

## When the camera is open

Only while the voice service says so. `lamp-voice serve`, when
`LAMP_EYES_PERMIT_SOCKET` is set, sends `{"camera":true}` to that socket twice
a second while the microphone is permitted, and `{"camera":false}` once, at
once, when it is not (`crates/voice/src/body_link.rs`, `EyesLink`). The eyes
open the camera on the first yes and close it on a no or when no yes has come
for 1.5 s, so a voice service that is muted, stopped or gone closes the
lamp's eyes too. The rule is the same as for its ears: the camera may look
exactly while the microphone may listen.

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
Resetting the camera's USB port to end a refusal is the fifth builder's
idea, not built here.

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
  remembered. The camera gives 320 by 240, so a face at the desk is 35 to
  65 pixels tall. Whether that is enough is to be measured with people
  (the camera can give 640 by 480 if not).
- **Kept.** `--people FILE` (on lamp-4ace `/var/lib/lampos-eyes/people.json`,
  in a directory of mode 0700 and a file of mode 0600). Per person: the
  name, up to five prints and when they were added. At most 20 people. No
  picture is kept, and no print leaves the lamp. A file that is too large
  or not of this shape stops the eyes rather than being guessed at.
- **Asked.** One line of JSON on `--who-socket` (on lamp-4ace
  `/run/lampos-eyes/who.sock`) and one line back:
  - `{"remember":"Minh"}` takes five prints over the next looks while that
    one face is at the lamp and answers `{"remembered":"Minh"}`. With no face
    in view at all it answers `no_face` after 1.5 s. With a face too far
    away, or two at the lamp, it gives them four seconds and then answers
    `too_far` or `two_faces`.
  - `{"forget":"Minh"}` answers `{"forgot":true}` once the file no longer
    has it.
  - `{"list":true}` answers the names.
  - `{"who":true}` answers who is at the lamp now, or `null`.
  - Remembering someone already known replaces their prints.
- **Said.** With `--known-file` (on lamp-4ace `/run/lampos-eyes/known`,
  beside the socket), the name of the known person at the lamp is kept
  there and renewed every two seconds while they are there. The file is
  removed when nobody is near.
- **The voice** (`crates/voice/src/known_faces.rs`, when
  `LAMP_EYES_WHO_SOCKET` names the socket):
  - The model has two functions, `remember_my_face` and `forget_my_face`,
    each with the name. They are called only when the person asks.
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

## On the bus

With `LAMP_BUS=on` in its unit the eyes also publish every face the
detector finds, not only the one the body is told to look at, on
`humans/faces/tracked` ([bus.md](bus.md), "On the lamp";
`crates/eyes/src/bus_out.rs`). For each face:

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
| `warn` | `the camera stopped giving pictures` | It was open and failed, until it is open again |

The one fact beside it is `camera`: `open`, `closed`, `not opening` or
`failed`. It is said on every turn of the loop that is due, whatever the
camera is doing, and the sign that the eyes are there is now given the
same way: before 2026-10-14 it was not given while the camera was not
permitted, so the eyes read as gone from the bus whenever they were not
looking.

### Whether a face is speaking (`LAMP_EYES_SPEAKING=on`, #720)

Off unless `LAMP_EYES_SPEAKING=on` is in the eyes' unit beside `LAMP_BUS=on`.
Off, nothing is computed and `humans/faces/tracked` is byte for byte as
before. On, each face also has `speaking`:
`{"state":"yes"|"no"|"unknown","score":0.03}`
([`speaking.rs`](../crates/eyes/src/speaking.rs)). It is read from the five
landmarks YuNet already gives: a few additions a face a look, no new model,
and no picture is kept or leaves the lamp. Nothing reads it yet.

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
- **The pace:** the eyes look every 250 ms while they follow a face, up to
  350 ms on the 10 fps camera (`main.rs`), so 2 s holds 5 to 8 looks. With
  `--steady-rate` a still face is looked at every 500 ms, 4 or 5 looks a
  window, and it is mostly `unknown` or wrong (below).

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
| 450–550 ms | 0.3 px | 0.11 / 0.25 | 0.27 / 0.48 | 0.34 / 0.48 | 0.46 / 0.48 |
| 450–550 ms | 0.5 px | 0.07 / 0.20 | 0.13 / 0.45 | 0.20 / 0.48 | 0.31 / 0.48 |
| 450–550 ms | 1 px | 0.00 / 0.07 | 0.03 / 0.29 | 0.08 / 0.39 | 0.13 / 0.45 |

A face at 1 m fills about 25 pixels of mouth in the detector's 320-wide
picture, so its eyes are about 27 pixels apart. On this model, that is too
small at any jitter. The signal holds only for a near face (60 px, about
0.45 m) with steady marks, and not under `--steady-rate`. **What a lamp
still has to show,** numbers only: YuNet's corner jitter on a still face at
0.5 and 1 m, and the score for a person silent, talking, nodding and
turning. If 1 m matters, the next step is a landmark model with inner-lip
points run on the tracked face box, as #720's comments propose.

Once it can be trusted, the voice's "spoken to" judging is to read it
(`LAMP_SPOKEN_TO=on`, `crates/voice/src/attention.rs`, #829). A line heard
while the only face turned to the lamp reads `no` is not for it, and a face
that reads `yes` with the voice's speech is the one speaking.

## Running it

```text
lamp-eyes --device /dev/video0 --lock-directory /home/WORKER/private-camera-locks --usb 01da:5875@3-1.1.4 --model /opt/lampOS/eyes/face_detection_yunet_2023mar.onnx --permit-socket /run/lampos-eyes/permit.sock --body-socket /run/lampos-body/gaze.sock
```

Every option shown is required; `--sight-file ABSOLUTE_PATH`, `--leave-still-faces on` and `--full-look 1280x960 --full-look-file /run/lampos-eyes/full-look.jpg` (up to `3264x2448`) may be added, the last with `--full-look-movement on`.
`--usb` is `VENDOR:PRODUCT@TOPOLOGY`, the two numbers as four hex digits; the
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
