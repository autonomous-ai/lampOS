# What the lamp sees for its skills (`lamp-seen`, #6543)

V1's proactive skills could see what was happening: a drink, a meal, a
break, a slump. The current lamp only counts time at the desk, so
`sip-of-water` can nudge just after a drink and `meal-check` just after a
meal. `lamp-seen` brings back two of V1's cloud senses, **activity** (eat,
drink, a break) and **posture** (pose, then RULA), from the same perception
service and wire, so the server needs no change.

It is one process with one owner. It asks the camera for a frame only when
the rules below allow, sends it encrypted, and puts **labels** on the bus.
No frame and no voice ever reaches a skill, the bus or the model.

It is off behind `LAMP_SEEN_ACTIVITY` ([switches.md](switches.md)). Its
product unit is installed on every lamp, off
([The product unit](#the-product-unit)). Phase 1 and 1b make no server call. Phase 2
(activity, below) sends frames only when the launcher also gives it the
account (`--account`, `--key`); without them, **no frame leaves the
lamp**.
Phase 3, posture, sends a frame every 30 s only with its own sub-switch
`LAMP_SEEN_POSTURE` on too, also off ([Phase 3: posture](#phase-3-posture)).

## Phase 0: the rules

The rules are as phase 0 has them on #6543, with the first builder's
seventh line. The lead approved all seven, adding to the seventh that the
lamp also says once when the opt-in is turned off.

1. **The opt-in is off by default.** It is the owner's yes on that lamp,
   given in the app or spoken.
2. **The ring shows its mark whenever a frame is going out.**
3. **No frames** while the privacy slide is on, a stranger is in view,
   nobody is at the desk, or the lamp is asleep.
4. **Nothing is kept.** A frame is sent, labelled and dropped, on both
   ends.
5. **Forget everything turns the opt-in off.**
6. **Voice never leaves the lamp for this.**
7. **The opt-in is said back once when it's turned on:** "I'll look now
   and then to see if you're eating, drinking or slouching. I keep none of
   the pictures." (the first builder's wording). With posture on, a line
   that names posture too (below, awaiting the PM's wording). **And once
   when it's turned off** (the lead's addition).

## What phase 1 and 1b do

`crates/seen`, the binary `lamp-seen serve`:

- **The "no frames" gate** (`gate.rs`). This pure function asks for no
  frame unless every rule holds. Anything not known counts as closed. It
  reads only the bus, never the eyes' private files:

  | Rule | Read from | Closed when |
  |---|---|---|
  | The opt-in | the opt-in file (below) | no kept yes, or a forget came after it |
  | The opt-in said back | the said-back record (below) | the on line for the kept yes has not begun (`not_said_yet`), so the ring's first camera mark never comes before the lamp said it would look |
  | The privacy is open | `voice/privacy` (`MicrophonePrivacy`) | the lease is invalid, is not permitted, or is older than 50 ms. The camera looks exactly while the microphone may listen ([eyes.md](eyes.md#when-the-camera-is-open)) |
  | Someone is at the desk | `room/presence` (`someone`) and `humans/faces/tracked` | nobody, no face in view, or either value stale |
  | No stranger | `humans/faces/tracked` (count, passing `id`) and `humans/familiar` (`level`) | more than one face, the one face has no familiarity level, or the level was said before that face came into view |
  | Awake | `expression/state` (`says_asleep()`, valid and enabled) | asleep, falling asleep, disabled or stale |

  Presence is `room/presence`, the one signal every reader uses, rather than
  `room/activity`'s `at_desk`. Presence can be shown by a voice alone, but a
  voice puts nobody at the desk to see. So a face in view on
  `humans/faces/tracked` is also needed.

  `humans/familiar` names no face, and a level stays fresh for 5 s. So a
  level counts only if it was said at or after the moment the one face now
  in view, by its passing `id`, came into view as the only face. A new id,
  a second face, no face, or an invalid list since then closes the gate
  until the eyes say a level again. When the owner leaves and a guest sits
  down a second later, the owner's level is not the guest's. A new id that
  `follows` an old one counts as a new face.

- **The frames** (`frames.rs`). These follow the frame link's rules
  (`crates/camera/src/frames.rs`, the `perception` role):
  - one request at a time, only while the gate is open in that same step;
  - the frame is copied into one buffer of this part's own before anything
    else;
  - the first frame's `CameraGrant` is kept, and a frame under another grant
    is dropped;
  - when the gate closes for any reason, or the link closes or fails, which
    includes the camera's privacy close, everything goes: the connection,
    its partial frame, the request in flight, the grant and the copy (wiped).

  Each copy goes to a `Sink`, the one place a frame would go to the server.
  Without the account it is `Discard`, and the copy is wiped right after;
  with it, activity's sink ([phase 2](#phase-2-activity)). The sink
  is asked before each request (`Sink::ready`) and told when sending stops
  (`Sink::stopped`), which is where the ring's mark holds (below).

- **The ring's mark before any frame** (`mark.rs`, phase 0's second line,
  1b). `Marked` wraps the sink, so `Discard` is gated by it too, and it
  fails closed:
  - Before each request it asks the ring for its mark on `light/mark`
    (`lamp_msgs::RingMark`): claim source `lamp_seen`, `camera`, a `blink`,
    priority `skill`, held 1 s ahead (`mark::HOLD_US`) on the boot's clock
    (`/proc/sys/kernel/random/boot_id`). No frame is asked of the camera
    until the ring says on `light/mark_drawn` (`lamp_msgs::RingMarkDrawn`,
    only from the ring's own publisher `voice_ring_mark`, for `lamp_seen`,
    on this boot) that it drew the mark at or after that ask. Not drawn
    within 1 s (`mark::WAIT_US`): still no frame, and the mark is asked
    again. No ring, no bus, no boot id or a ring at ceiling 0 means no
    frame.
  - A frame passes to the sink only if it was captured while the blink was
    surely lit: its capture time (`FrameMeta.dequeue_completed_at_us`) is
    at or after the ring's first word that it drew the mark since the ask,
    and no later than the ask plus 0.6 s (`mark::BLINK_US`, the ring's
    `ring_mark::BLINK_US`). The ring starts a blink when it takes the ask,
    which is after the ask and before its first draw, and holds it 0.6 s,
    so the dot is lit through that whole window, however late the first
    draw. The ring's word must also be within 1 s of now
    (`mark::SHOWN_WITHIN_US`, as lamp-control's
    `camera::MARK_SHOWN_WITHIN`). Any other frame is dropped and wiped.
    The capture time, the ask and the ring's word are all on the boot's
    `CLOCK_MONOTONIC`, and the ring's word counts only for this boot's
    clock id.
  - So that the camera's newest frame isn't one from before the dot lit,
    no frame is asked for until 40 ms after the first draw
    (`mark::FRESH_US`, about one frame at 25 fps), and the request takes
    no frame older than the time since that draw (`Sink::max_age_us`,
    under `frames::MAX_AGE_US`). No request goes after the ask plus 0.6 s:
    a blink that ends with no frame inside it is asked again
    (`blinks_missed` on `diagnostics`). Each frame passed makes the next
    one ask its own mark: one blink for each frame.
  - When sending stops (the gate closes, the link is lost, the grant
    changes, the process stops) the mark is let go: it is asked for no
    more and its ask is forgotten, so a word from the ring from before the
    next ask counts for nothing. The ring takes no earlier end for a claim,
    so letting go is not renewing it: a blink shows for 0.6 s, and the
    claim lapses within 1 s.

- **The opt-in** (`opt_in.rs`). It is kept as room sounds and face
  expressions are ([cloud.md](cloud.md), #5500, #5884), in
  `/var/lib/lampOS/consent/seen-activity.json` (the folder is
  `--opt-in-folder`), present only while the yes stands:
  `{"version":1,"seen_activity":"yes","at_unix_s":N,"by":"voice"|"app"}`,
  as `lamp_msgs::consent::bytes` writes it.
  - **The read rule.** It is a yes only when `seen_activity` is exactly
    `"yes"`. Missing, unreadable, over 4096 bytes, not JSON or anything
    else is no. It is read with `lamp_msgs::consent::read`, the rule
    lamp-ears keeps for `room-sounds.json`.
  - **Read again every 2 s** (`opt_in::READ_EVERY`), as lamp-ears asks its
    yes again. Once the file is gone, the gate closes at the next read.
  - **`lamp-seen` never writes the consent folder.** The voice writes the
    file (0640 `root:lampos`, whole, through a temp file and a rename), as
    onboarding's storage worker writes `room-sounds.json`; the app's yes
    takes root's helper, as room sounds' does. Any forget and a redo of
    setup remove it. `lamp-seen` only reads it, in the `lampos` group.
- **The opt-in said back, once each way** (`said_back.rs`, phase 0's
  seventh line with the lead's addition, 1b). When the opt-in turns on, and
  again when it turns off, the lamp says one line, word for word, through
  the voice's say service
  ([speaking-first.md](speaking-first.md#asked-by-another-part-the-say-service-939)),
  the way other parts have the lamp say something: `{"skill":"seen",
  "text":…, "priority":6, "expires_in_ms":120000}` on `--say-socket`
  (`LAMP_SAY_SOCKET`, `/run/lampos-say/say.sock` if unset), on a thread of
  its own so the gate and the frames never wait. The lines:

  > On: I'll look now and then to see if you're eating, drinking or
  > slouching. I keep none of the pictures.
  >
  > Off: Okay, I've stopped looking at the desk.
  >
  > On, with posture (`LAMP_SEEN_POSTURE` on): I'll look now and then to
  > see if you're eating, drinking or slouching, and now and then how
  > you're sitting, so I can remind you to sit back. I keep none of the
  > pictures.

  The posture line is **awaiting the PM's wording**: the proposed clause
  is "…and now and then how you're sitting, so I can remind you to sit
  back." It is one constant, `said_back::POSTURE_ON_LINE`.

  - Once the voice has begun a line (`{"event":"begun"}`) it counts as
    said, and what it said is kept in `lamp-seen`'s own state folder
    (`--state-folder`, `/var/lib/lampos-seen` by default), in
    `said-back.json`: `{"said":"on","for_at_unix_s":N}` for the yes it
    was for, with `"posture":true` when the line said was the posture
    line, or `{"said":"off"}`. A record with no `posture` (one kept before
    posture) is the plain line. Never under `consent/`. A restart says
    neither again. If the record can't be written, the line is still not
    said again while the process runs. A record left over after the yes
    was removed means nothing, because the gate reads the yes.
  - The off line is owed only after the on line was said: the record says
    on and the opt-in is now off. A lamp whose on line never began says no
    off line.
  - **A forget says the off line too.** Forget everything turns the opt-in
    off, so the person hears that the lamp stopped looking, as when the
    yes is taken back in the app.
  - Not begun (no say service, refused, expired): asked again 2 min later
    (`said_back::RETRY_US`), while that line is still the one owed.
  - **A line still queued is withdrawn when the line owed changes**, for
    example a forget before the on line has begun, or another yes. The
    say service's cancel (`{"cancel":true}`, `say::CANCEL_LINE`) is
    written on that ask's own connection, since a connection closed
    cancels nothing ([speaking-first.md](speaking-first.md)). Once its
    answer comes, the line now owed is asked for at once. An on line
    withdrawn before it began was never said, so no off line follows it:
    a forget then says nothing. One that had begun before the cancel
    reached it counts as said, and the off line follows.
  - **No frame before the on line.** The gate stays closed
    (`not_said_yet`) until the on line for the kept yes has begun. A lamp
    with no say service never opens the gate: this fails closed by
    design. The product unit runs as `lampos`, the say socket's group
    (the voice's `LAMP_SAY_GROUP=lampos`), so it may reach it.
  - **No posture frame before the posture line.** With posture sending,
    the on line owed is the posture line. A yes whose plain line was said
    before posture was switched on is owed the posture line once, and
    posture's gate stays closed (`not_said_yet`) until it has begun for
    the kept yes; activity's gate opens on either line. A forget then a
    new yes needs the posture line again, as any new yes needs its on
    line.
- **Forget everything** (`memory/forgot`). Any forget turns the opt-in off
  in the running process at once. The voice removes the kept yes;
  `lamp-seen` doesn't. Until the file is read gone, or replaced by a yes
  given after the forget (a later `at_unix_s`), a yes is refused, however
  slow the removal is. A yes with no time can't be shown to be later, so
  it stays off until then.
- **Encryption** (`crypto.rs`), ported byte for byte from V1's
  `hal/drivers/sensing/crypto.py`:
  - a random 32-byte AES-256-GCM session key, sent RSA-OAEP-encrypted under
    the server's public key (SHA-256 for the hash and for MGF1, no label);
  - each message under a fresh random 12-byte nonce, with no associated
    data, and `cipher_data` as the ciphertext followed by the 16-byte tag;
  - standard Base64;
  - V1's envelopes: an HTTP request `{nonce, cipher_data, encrypted_key}`,
    an HTTP response `{nonce, cipher_data}`, the WebSocket key exchange
    `{type: key_exchange, encrypted_key}` answered by
    `{status: key_exchange_ok}`, then each message
    `{nonce, cipher_data, type: encrypted}`.

  There is no plaintext path. Its tests check vectors made with Python's
  `cryptography`, as V1 uses it. The crates are `rsa` 0.9.8 and `aes-gcm`
  0.10.3 (RustCrypto, MIT or Apache-2.0); see
  [reuse-decisions.md](reuse-decisions.md#v1s-perception-encryption-in-rust-6543).
- **The server is full** (`backoff.rs`). On a 503, sending pauses for
  `Retry-After`, as V1's `retry_after_s` sets it: the header's seconds,
  held to 2–30 s, 5 s when the header is missing or a date, and ±20 %
  jitter. While paused no frame is sent, but `humans/doing` keeps going
  out on the clock (phase 2), so the skills run on their own clock and
  presence rules. Activity's socket never gets a 503: its bursts have
  their own retreat (phase 2).
- **V1's wire** (`wire.rs`), as types only. The activity session
  (`/ws/hal/api/dl/action-analysis/ws`, task `action`) sends a `config`,
  then `heartbeat`s and `frame`s, and gets back `detected_classes`. The pose
  session (`/ws/hal/api/dl/pose-estimation/ws`, task `pose`) gets back
  `pose_2d`, `pose_3d` and `ergo`. It also has V1's label groups (drink,
  eat, break and the rest).
- **Health.** Once a second it says on `diagnostics` (part `seen`, so on
  `lamp/health`) whether the gate is open or why not, its frame counts
  (`requested`, `handed`, `grant_changed`, `lost`), and its marks: asked,
  not drawn in time, blinks that ended with no frame inside them, and
  frames dropped with no mark drawn for them or captured outside the
  blink. With activity on, also its bursts (below), and `warn` while they
  retreat.

## Phase 2: activity

With a frame link (`--frames-socket`, or `--frame-link-file` in the
product) and the account (`--account`, setup's `backend.json`; `--key`,
the account key; both as the product unit's `LoadCredential=` gives them,
read by path inside the process), the frames' sink is activity's
(`activity.rs`) instead of `Discard`. It still sits inside the ring's mark
and behind the gate, so a frame reaches it only while the gate is clear
(the on line for the kept yes begun included: `not_said_yet` closes it)
**and** the ring said it drew the mark for that frame. Any gate close,
that one too, aborts a burst under way.

- **One burst a minute** (`activity::EVERY_US`), of **8 frames**
  (`activity::FRAMES`, the server's clip). The sink wants a frame
  (`Sink::ready`) only while a burst is due or under way with frames left,
  so between bursts no frame is asked for and the ring is not asked for its
  mark (`Marked` asks the sink inside first). The taker asks for the
  frames 1.2 s apart (`activity::SPACING_US`), and the burst sends them at
  least 1.1 s apart (`burst::Pace::gap`): the server buffers a frame into
  its clip only a second or more after the last.
- **Each burst is its own socket** (`burst.rs`), on a thread of its own so
  the gate, the frames and the bus never wait on the network:
  1. the server's public key, fetched once with `X-API-Key` from
     `<base>/crypto/public-key` (V1's `fetch_public_key`) and kept, unless
     `--public-key-file` gives it; fetched again after a refused key
     exchange;
  2. a new WebSocket to `<base>/ws/hal/api/dl/action-analysis/ws`
     (`wss` for `https`), with `X-API-Key`;
  3. the key exchange, then V1's config: the 30 labels of V1's
     `white_list.txt` in its order plus one class beyond V1's list,
     "eating chips" after "eating carrots" (`wire::WHITELIST`, 31
     labels), threshold 0.3, person detection on, its least area 0.25.
     "eating chips" is the nearest Kinetics class to a snack, added after
     the 13 Nov lamp run gave no eating label for one; it is in the `eat`
     group. The whitelist reaches only this session: the server builds each
     session's class mask from the list that session sent
     (`core/perception/action/session.py`, `_post_config_update`), and V1
     lamps send their own 30 at each session's start, so they are
     unaffected. The server keeps **one** config object for every session
     (`perception.py`, `create_session`) and reads its threshold live, so
     the threshold and the person fields stay V1's: another value there
     would change V1 lamps' results. The config's answer is read and not
     checked, as V1 does;
  4. each frame, `{"type":"frame","task":"action","frame_b64":…}`,
     encrypted, and its answer read before the next;
  5. the socket closed. The server's session, with the frames it buffered
     for the clip, ends with it, so **nothing is kept between bursts**.

  No heartbeat is sent: a burst lasts about ten seconds, and V1's heartbeat
  came once a minute. Every wait is bounded: 15 s for the connection, the
  public key and each answer (V1's `DL_WS_RECV_TIMEOUT_S`), and 10 s for
  the camera's next frame, after which the burst ends with what it has.
- **The address** is V1's (`endpoint.rs`, `v1_base`): the account's
  `llm_base_url` with trailing `/` trimmed, ending in exactly one `/v1`,
  added when missing and never doubled. V1 lamps hold
  `…/api/v1/ai/v1`; setup's `backend.json` holds `…/api/v1/ai/`, and
  without the `/v1` the relay answers 404 "route not found". `<base>`
  above is that `/v1` base, and `Endpoint::socket` gives any other V1
  path on it. A relay probe on 12 Nov got 101 on both WebSocket upgrades
  (action-analysis and pose-estimation) on the `/v1` base, and 521 on the
  public key (the origin down). The key goes only to
  `https://campaign-api.autonomous.ai`, or to this machine (`127.0.0.1`,
  `localhost`, `[::1]`, for the simulator and the tests); any other base
  is refused and nothing is sent.
- **The frames** are sent at 320×240 (`scale.rs`), whatever size the
  camera serves: only the camera's own JPEG (`MJPG`, not a derived copy)
  of at least 320×240 (`activity::LEAST_SIZE`) is taken. A larger one is
  decoded (`zune-jpeg`), scaled down by area until one side meets
  320×240's and the other is no smaller (4:3 becomes 320×240 exactly,
  1280×720 becomes 427×240), and encoded again at quality 80
  (`jpeg-encoder`); 320×240 goes as it came. Anything smaller, not MJPG,
  derived, or a JPEG that does not decode at its meta's size is dropped
  and counted (`wrong_frame`). Before this, a lamp whose eyes ran
  `--full-look 1280x960` had every frame dropped (lamp-06c1, 13 Nov: 106
  `wrong_frame`, no burst sent). The server takes any size: its person
  detector letterboxes the frame to its own input, and the action model
  scales the person's crop to 224×224 (`preprocess_single_frame`). V1
  sent 640×480 from its own camera; whether 320×240 reads drinks and
  meals as well is not measured; the lamp rows before the switch goes on
  should say. The scaling runs in the frame's call, before a burst opens
  for it: on a development Mac, in release, a 1280×960 frame (274 KB, more detail
  than the lamp's ~88 KB) takes 26 ms at the median and a 640×480 one 9 ms
  (`scale::tests::scale_time`); a lamp's A55 cores, three to five times
  slower, would take about 80–130 ms for 1280×960, eight times a minute.
  Not measured on a lamp yet.
- **Copies are wiped.** Each frame's copy for a burst is wiped when its
  message is made, or with the burst if it was never sent; its Base64 and
  the message's plaintext are wiped too, and only ciphertext reaches the
  socket. A scaled copy is made inside the copy itself: the decoded and
  the scaled pixels are wiped before it is handed on, and the encoder
  writes into room reserved for the whole result, so no outgrown buffer
  is left behind. The decoder's and encoder's own working rows are inside
  those crates and are freed unwiped. The frames' own buffer is wiped as in phase 1.
- **Busy.** The server answers a busy frame `{"error":"busy"}` and never a
  503 on this socket. The burst is then dropped at once, with no retry
  inside it, and the next comes in its turn.
- **The retreat** (`backoff::Retreat`). After 3 busy bursts in a row, or
  any burst that failed (no public key, no connection, the upgrade refused,
  such as a 5xx, a refused key exchange, a server error, no answer in
  time), the next burst waits 60 s, doubling each time to 10 min, with
  ±20 % jitter. Meanwhile `diagnostics` says `warn` (so `lamp/health`
  does), with the last failure, until a burst is answered.
- **The gate closes mid-burst** (or the link is lost, the grant changes,
  the process stops: `Sink::stopped`): the burst is aborted at once,
  whatever it waits on, its socket closed, its frames wiped and its answers
  dropped.
- **What was seen** (`sitting.rs`). A burst's answers are read as drinking,
  eating or a break (V1's `drink`, `eat` and `break` groups), never as the
  raw label. A false "drank" silences sip-of-water for 90 minutes, so a
  kind counts only when one answer is at least 0.5 sure of it, or at least
  2 of the burst's last 4 answers have it at all (the server sends only
  what clears 0.3).
- **The sitting and its timers.** A sitting begins when one remembered face
  is at the desk (the gate's presence and stranger lines, whatever the
  opt-in, its said-back line, the privacy or sleep say), from `room/activity`'s `sitting_s`
  when it said one lately, else from now. It ends on `room/stood_up`, and
  when the gate's lines say nobody or a stranger, except through a pause
  on a misread (below, "The pause on a misread"). Its timers count from the
  desk arrival until something is seen, and from the last sighting after.
- **Published**, on the topics `lamp_msgs` declares (`doing.rs`):
  - `humans/doing` (`HumanDoing`): `drank_s_ago`, `ate_s_ago`,
    `break_s_ago` (always present), `last` and `confidence`, every 30 s
    while a sitting lasts, and at once after a burst that saw something.
    It goes out whether or not frames do (the opt-in off, the server busy,
    the slide shut, no account given), so a skill's `[[while]] above` on a
    timer runs on the clock as it would without this part;
  - `humans/doing_seen` (`DoingSeen`): once for each kind newly seen, not
    seen in the burst before.
- **Health**: with activity on, `diagnostics` also says `sends: activity`,
  its bursts (`bursts`, `answered`, `busy`, `failed`, `aborted`),
  `frames_sent`, `frames_wiped`, `wrong_frame` and `last_failure`.

The crates are the ones the other lampOS clients use, pinned as
`lamp-gemini` and `lamp-voice` pin them: `tokio-tungstenite` 0.30.0,
`rustls` 0.23 with the ring provider and the webpki roots, `reqwest` 0.13
with no provider of its own, so no OpenSSL; the lock gains no package
([reuse-decisions.md](reuse-decisions.md)).

Its tests run a local WebSocket server that speaks V1's side, the key
exchange with the test key's private half included, and answers each
frame with labels, `busy`, an error, or nothing: a full burst on V1's
wire and its socket closed after, a gate close mid-burst, an abort that
doesn't wait for a silent server, busy bursts and the retreat, a refused
upgrade, a server error and no server, the copies wiped, and the frame
sizes taken.

### What phases 1 to 2 do not do

- Without `--account` and `--key`, no server call and no socket to it: no
  frame, label or key leaves the lamp. `humans/doing` still goes out, from
  the clock.
- Posture is phase 3's, below: without `LAMP_SEEN_POSTURE=on` it sends
  nothing and `humans/posture` is not published.
- With no frame link (no `--frames-socket`, and no link kept in
  `--frame-link-file`), `lamp-seen` runs the gate and says it on
  `diagnostics`, and asks for no frame. In the product the eyes serve the
  link ([The product unit](#the-product-unit)).
- `humans/familiar` is said only with `lamp-eyes --visits`. Without it the
  gate reads every face as a stranger and stays closed. The product unit
  gives it with the switch on, for the stranger check only
  ([The product unit](#the-product-unit)).

## Phase 3: posture

`src/pose.rs` and `src/pose/`. Off unless `LAMP_SEEN_POSTURE=on` beside
`LAMP_SEEN_ACTIVITY=on`, with a frames socket and activity's account
(`--account`, `--key`, `--public-key-file`, read and checked by
`endpoint.rs`): posture uses the same base, key and host rule, on the pose
path. Without the switch or the account it sends nothing and says nothing.

Activity and posture share the frames: the sink inside the ring's mark is
both, and each takes a frame only when it wants one (`Sink::ready` is
either's). One frame and one blink can serve both.

- **Behind the same rules.** Posture is the frames' sink inside the ring's
  mark, so a frame reaches it only while the gate is open and the ring drew
  the mark for it.
- **Only once posture was said.** Posture's gate (`pose::gate`) is the
  frames' gate, closed `not_said_yet` too until an on line naming posture
  (`said_back::POSTURE_ON_LINE`) has begun for the kept yes (the
  said-back record's `posture`, above). Only the plain line said, posture
  asks no mark and takes no frame, while activity's bursts go on.
- **Only the camera's own JPEG**, as activity takes: MJPG, at least
  320×240, not derived, and sent at 320×240 as activity's are (`scale.rs`,
  above: a larger frame is scaled down inside its copy). Another frame, or
  one that does not decode at its size, is skipped and counted
  (`pose_wrong_frame`), with no request and no backoff. It is ready for a frame only every 30 s (V1's
  `POSE_SAMPLE_INTERVAL_S`), with no request running and the backoff not
  holding back. While neither sense wants a frame, no mark is asked for,
  so the ring blinks only for a frame on its way.
- **One socket for each frame** (`pose/client.rs`), on a thread of its own
  so the gate and the ring never wait:
  1. connect to `<base>/ws/hal/api/dl/pose-estimation/ws` with
     `X-API-Key`, `<base>` activity's: the account's `llm_base_url` ending
     in exactly one `/v1` (`endpoint::v1_base`, above). The key goes only to `campaign-api.autonomous.ai` over TLS
     (rustls, webpki roots), or in the clear to this computer for the
     simulator and the tests;
  2. the key exchange (`crypto.rs`). The server's key is fetched once from
     `<base>/crypto/public-key` with `X-API-Key`, as V1 fetches it,
     and kept; it is read again after a refused key exchange;
  3. V1's pose config, its values exactly:
     `{"type":"config","task":"pose","frame_interval":1.0,"confidence_threshold_2d":0.3,"min_valid_keypoints":5}`.
     V1 sent none, so these are the server's defaults. The server's
     sessions share one config object, so any other value would change
     every V1 lamp's results. The answer must be `config_updated`;
  4. the frame, and its answer: `ergo` (RULA), or `{"error":"busy"}`;
  5. the socket closed, so the server drops its session and the keypoints
     it kept.

  Every wait is bounded at V1's 15 s. The frame's Base64 and its plaintext
  message are wiped when the request ends (the encrypted copy handed to the
  socket library is not).
- **The gate closing stops it.** The frames tell the sink when the gate
  closes, and the request is cancelled: a wait on a read sees the cancel
  within 0.1 s and closes the socket, no connect begins after it (the
  socket's open looks at the cancel before each connect), a connect or
  write already begun runs to its own 15 s bound, a frame not yet sent is
  never sent, and an answer that still comes is dropped. A cancel is not a
  failure. The gate is judged once per 100 ms tick, and the request's last
  look at the cancel is just before it sends, so a frame captured just
  before the slide closes can still go out within about one tick.
- **Busy and failures**, as activity's bursts take them (`backoff::Retreat`,
  one of its own).
  WebSocket streams never get a 503, only `{"error":"busy"}`. Busy drops
  that frame, with no retry. Three busy answers in a row, or any failure,
  pause sending for 60 s, then 120, 240, 480 and at most 600 s, each with
  ±20 % jitter. A failure is no connection, a refused or 5xx handshake or
  key fetch, a refused key exchange, no answer within 15 s, or the server's
  own error. While a pause has come and no frame has been answered since,
  `diagnostics` says `warn` for part `seen`, so `lamp/health` shows it. An
  answer clears it all.
- **The window** (`pose/window.rs`), V1's rules (`pose.py` `_aggregate`):
  - a sample is bad when `risk_level` is 2 or more, or any region's
    subscore on either side is 4 or more;
  - `bad_share` is the share of bad samples over the last ten minutes,
    said only over at least 3 samples (V1's minimum; V1 said 0 under it);
  - `region` is the region most often high among the bad samples, with
    ties going to neck, trunk, upper arm, then lower arm. Wrist counts
    toward a bad sample, as in V1, but is never the region: the server
    fixes it at 1;
  - `window_s` is how long this sitting has been watched, at most 600 s.

  An answer with no body found counts no sample. A sample is its time, one
  flag and four bits: nothing of the frame or the answer is kept. The
  window is forgotten whenever posture's gate closes, for any reason (the
  privacy, sleep and anything not known included), and on
  `room/stood_up`, except through a pause on a misread (below), which
  holds it unsaid for at most 5 s.
- **Said on `humans/posture`** (`lamp_msgs/HumanPosture` v1:
  `{window_s, bad_share, region}`) every 30 s while posture's gate is
  open and the window holds at least 3 samples: as soon as the sitting's
  third sample lands, then every 30 s. Closed, the topic says nothing and
  its last value runs out, so no old sample is said while the privacy is
  on. **Under 3 samples, nothing is said either** (#6839's and #6660's
  reviews): frames not going out (the opt-in off, the slide shut), the
  server busy or failing, no body found, the sitting just begun, or the
  window just wiped (any closing, the privacy, sleep, `room/stood_up`).
  The topic runs out, so a skill holding on it with
  `unknown = "holds_stale"` (Sit back) runs as it did, rather than reading
  a fresh `bad_share` of 0 that would hold it quiet: a `bad_share` of 0 is
  said only over at least 3 samples.
- **Health.** `diagnostics` says `sends: activity+pose` (in place of
  `activity`), and adds `pose` (`idle`, `sending`
  or `backing_off`), the counts sent, answered, with no body, busy, failed
  and cancelled, the window's samples, the frames skipped as not the
  camera's own JPEG (`pose_wrong_frame`), and the last failure's kind.

**Frame size.** Every frame is sent at 320×240, whatever size `lamp-eyes`'
frame link (#6645) serves (`--full-look` included, scaled down as
activity's are); V1 sent 640×480. The pose model needs no more: the server
resizes the whole frame to RTMPose's own input
(`predictors/pose2d/base.py` `preprocess`, `cv2.resize`) and returns the
keypoints in the frame's pixels. One step depends on the size: the 3D
lifter normalises the keypoints by a fixed 1920×1080
(`predictors/pose3d/base.py`), so a 320×240 frame's pose is half the scale
of V1's 640×480 (which was itself a third of what the lifter expects).
Whether RULA reads the same at that scale is not measured; the lamp rows
below should compare a slump at 320×240 against V1's 640×480. One frame
every 30 s costs about 26 ms to scale from 1280×960 on a development Mac,
an estimated 80–130 ms on a lamp.

**Not done in phase 3:** the posture line's wording awaits the PM.

## The pause on a misread

On a live person, `humans/familiar` flips between known and stranger
whenever the owner looks down: the recogniser's threshold on a tilted face
(lamp-06c1, run 2: slouching gave 64 open samples to 54 stranger, upright
31 to 88). Each flip closed the gate, which forgot the posture window and
ended the sitting, so `bad_share` stayed 0.0 and a drink seen at 0.91
never reached `humans/doing`. So a stranger reading on the same tracked
face **pauses** the gate instead (`pause.rs`, under the Reviewer-lead's
conditions on #6543):

- **What pauses.** The gate was open on one face at the last step and now
  closes only because a fresh `humans/familiar` says nobody remembered,
  said after that face came into view alone. That same passing id is the
  only face in view, its box has not moved by more than the face's own
  width, and every other rule of the gate holds.
- **A pause sends nothing.** The gate stays closed while paused, so no
  frame is asked of the camera, no mark is asked of the ring, and a burst
  or pose request under way is aborted, its answer dropped. Nothing kept
  is published: no `humans/posture`, `humans/doing` or
  `humans/doing_seen`.
- **What ends it.** Only a new known reading on that same passing id,
  said after the stranger one, opens the gate again: time alone never
  does. The kept window and timers carry on only when the box is then
  near where it paused (the centre within the face's own width).
  Otherwise they are wiped and the sitting starts fresh.
- **What a pause keeps.** The posture window and the doing timers, in
  memory only. They are wiped after 5 s paused, and at once on a box jump
  between samples larger than the face's own width, a new id, a second
  face, no face, the privacy, sleep, the opt-in withdrawn,
  `room/stood_up`, or anything the gate no longer knows.
- **The familiar yes is never carried to a new id** (#6602's P2): a new id
  wipes, and the gate still wants a level said after that face came.
- **The journal** says `gate paused on a misread (paused N,
  misread_on_track M)` and `the pause ended: kept` or `wiped`.
  `diagnostics` adds the counts `paused` and `misread_on_track`. No id and
  no box is written, and `humans/familiar` is unchanged.

**Why the passing id alone is not enough, and why the pause stands
without more.** The eyes follow a face by its box alone (`lamp-eyes`
`bus_out::Ids`: the same id for a box overlapping one seen within 1.5 s by
at least 0.2 of their union), with no face print. So an id can pass to
another person who takes the same place within 1.5 s, or swap when two
faces cross. `humans/familiar` says only a level, not whose. The pause
does not rely on the id meaning one person. It sends nothing, and only
the recogniser's own known reading ends it, which is what opens the gate
anyway. Two faces, or none, wipe at once, before a swap through a crossing
or an occlusion can matter. What is left is the owner's window and timers,
unpublished, for at most 5 s, and dropped if the box moved. A face print
check from the eyes may come later, as its own read, if the 5 s ever
needs to grow.

## The product unit

`deploy/product/lamp-seen.service`, installed and enabled on every lamp by
`lamp-update install` and carried by each build (`install::PRODUCT`; the
binary is in `optional` in `deploy/package-binaries.json`, beside the
other parts with a unit). With `LAMP_SEEN_ACTIVITY` unset, as in the
product, it idles and touches nothing, and the eyes serve no link: the
product is unchanged.

- **Under the keeper.** In `lampos.target` (`PartOf=`, `WantedBy=`),
  `Restart=on-failure` after 5 s, at most 5 starts in 10 min, as every
  part ([keeper.md](keeper.md)). An update round restarts it with the
  other running parts it changes; the round that first adds it starts it
  only after the watch, once the switch is kept (`start_added` in
  `round.rs`), not with the restart. The eyes may restart before it in
  that round, with no `/run/lampos-seen` yet: no folder reads as no file,
  so they start without a link. So `humans/doing` does not run out for
  long when it stops.
- **As `lampos`**, in `lampos-parts.slice`, `Nice=10`. That one user covers
  the three it reaches: the bus (group `lampos`), the voice's say socket
  (`LAMP_SAY_GROUP=lampos`) and the eyes' frame link, whose folder must be
  the eyes' own user's with no group access. Least privilege:
  `ProtectSystem=strict` with only the bus writable besides its own two
  folders, no capabilities, `AF_INET`, `AF_INET6` and `AF_UNIX` only, and
  the eyes' files kept as the same user (`/run/lampos-eyes`,
  `/var/lib/lampos-eyes`, `/run/lampos-eyes-kept`, `/run/lampos-photos`)
  and lamp-control's state made inaccessible.
- **Its own state**: `/var/lib/lampos-seen` (0700, its
  `StateDirectory=`, wiped by the reset plan), for the "said once" record,
  `said-back.json`, passed as `--state-folder` (also its default), and
  `/run/lampos-seen` (0700, kept across its own stop until the lamp
  restarts). **The consent folder is only read**, as a member of `lampos`
  (`root:lampos` 0750, the file 0640): `ProtectSystem=strict` leaves it
  read-only, so `lamp-seen` never writes it (Fourteen's ruling). The reset
  plan removes the yes, `seen-activity.json`, beside the other two.
- **The account**: setup's `/etc/lampOS/setup/backend.json` and the
  voice's `/etc/lampOS/voice/credential`, as `lamp-cloud` reads them,
  handed in by `LoadCredential=` and passed as paths (`--account
  %d/account --key %d/key`): the key is read inside the process, never on
  a command line. A lamp without them starts all the same, and sends
  nothing.
- **The frame link.** Its `ExecStartPre=`, `lamp-seen frame-link`, keeps
  the link's two boots (`link.rs`): with the switch on, two new boots from
  `/dev/urandom`, as the voice makes its camera worker's
  (`lamp_voice::process::new_boot`), written once, 0600, to
  `/run/lampos-seen/frame-link.env` as
  `LAMP_EYES_FRAME_LINK=--frame-link /run/lampos-eyes-frames/camera.frames
  --frame-link-controller HEX32 --frame-link-reader HEX32`, and left as
  they are while that file reads; with it off, the file removed.
  `lamp-eyes.service` passes `--frame-link
  /run/lampos-eyes-frames/camera.frames --frame-link-boots
  /run/lampos-seen/frame-link.env`, and the eyes read the boots from the
  file themselves (lampos's own, 0600, in a 0700 folder), never from their
  line or environment ([eyes.md](eyes.md#the-cameras-frame-link-for-lamp-seen---frame-link-6543));
  `lamp-seen serve --frame-link-file` reads the same line for its socket,
  its own boot and the controller's. The unit is ordered before the eyes,
  so the file is there when they start; the socket's folder,
  `/run/lampos-eyes-frames`, is made 0700 `lampos` by `lampos.conf`.
  Turning the switch on, or off, takes effect for the eyes only at their
  next start (the lamp's restart, or the target's). Until then, after
  turning it off, they keep their `--visits` counts and serve the link,
  but no frame leaves: only lamp-seen may connect, and off it idles.
- **The stranger check.** The same file's second line,
  `LAMP_EYES_VISITS=--visits on`, kept alone beside it as
  `/run/lampos-seen/eyes-visits.env` (the one file the eyes' unit loads,
  `EnvironmentFile=-`, and removed with the switch off), starts the eyes
  with `--visits` (they
  already run with `LAMP_BUS=on`), so `humans/familiar` says the level of
  the one at the lamp, with no name, face or picture. It is for the gate's
  stranger check only (the first builder's yes on #6543); off, the eyes
  run without it, as before.
- **Not in `lamp/health`'s expected parts**: off, it says nothing on
  `diagnostics`, so expecting it would mark every lamp stale. It goes in
  with the switch.

## Owed

| Item | What | When |
|---|---|---|
| Giving the yes | The voice's spoken yes writing `seen-activity.json`, the key in `lamp_msgs::consent::FILES` and `lamp-cloud`'s `consent.set`, removal on every forget (Fourteen's design on #6543) | before the switch goes on |
| Posture's on line wording | The posture line (`said_back::POSTURE_ON_LINE`, above) is gated in code but awaits the PM's wording | before `LAMP_SEEN_POSTURE` is on anywhere |
| Posture at 320×240 | A slump read on `humans/posture` from frames sent at 320×240 (every frame is, above) against V1's 640×480, to see whether the 3D lifter's scale changes RULA (above) | with the lamp rows |
| The frame size | Every frame is sent at 320×240, scaled down from the eyes' link at any larger size (`--full-look` included). Measure the labels at 320×240 on a lamp against V1's 640×480 before the switch goes on | before the switch goes on |
| `humans/doing` when lamp-seen stops | A topic that has run out holds a skill's `[[while]]` closed until lamp-skills restarts (`unknown = "holds"` covers only a topic never said). lamp-seen now runs under the keeper (above); the skills change (`unknown = "holds_stale"`) is still owed before a skill reads it | before a skill reads it |
| Skills | They read the labels with the existing `[[while]]` on a topic's field (for example `topic = "humans/doing"`, `field = "drank_s_ago"`); no new skill key | after the `humans/doing` row above |
| `lamp/health`'s expected parts | `--expect seen` in `lamp-health.service` once the switch is on in the product (off, it says nothing) | with the switch |

V1 faults not ported: activity's per-face mode sent double events (one
burst here, for the one remembered face), its heartbeat waited with no
timeout (`motion.py:208`; no heartbeat here, and every answer is bounded at
V1's 15 s, `wire::ANSWER_WITHIN_S`), and it reconnected every 2 s with no
backoff (here, the retreat).
