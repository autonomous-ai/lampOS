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

It is behind `LAMP_SEEN_ACTIVITY` ([switches.md](switches.md)), off unless
set. Its product unit is installed on every lamp and sets it on, with
posture, the held sitting, the stand-up break and the more doings
(#6543's switch-on; [The product unit](#the-product-unit)); nothing is
sent before the owner's yes. Phase 1 and 1b make no server call. Phase 2
(activity, below) sends frames only when the launcher also gives it the
account (`--account`, `--key`); without them, **no frame leaves the
lamp**.
Phase 3, posture, sends a frame every 30 s only with its own sub-switch
`LAMP_SEEN_POSTURE` on too, also off unless set and on in the product
units ([Phase 3: posture](#phase-3-posture)).

## Phase 0: the rules

The rules are as phase 0 has them on #6543, with the first builder's
seventh line. The lead approved all seven, adding to the seventh that the
lamp also says once when the opt-in is turned off.

1. **The opt-in is off by default.** It is the owner's yes on that lamp,
   given in the app or spoken. `LAMP_NOTICING_BY_DEFAULT=on` (#7767,
   switches.md; off unless set, in no unit) turns a missing
   `seen-activity.json` into a yes nobody gave (`"by":"default"`, no
   time), by `lamp_msgs::consent::allowed`'s one rule, shared with face
   expressions, room sounds and health reports (these with their own
   default, on, since 25 Nov 2026): exactly `"yes"` is on,
   exactly `"no"` stays off with the switch on, and a file that can't be
   read is off. With it off, a missing file is no, as before. With it on,
   the voice's stop and the app's desk off keep a no (`consent::no_bytes`)
   where today they remove the file (#7767, slice 2): the voice's
   `stop_looking_at_my_desk` writes `"no"` (`"by":"voice"`) for seen
   activity and face expressions both, under the mark's lock with the mark
   raised, and the app's `desk_noticing` off writes it (`"by":"app"`) for
   seen activity alone, with the mark raised to its `request_id`, so a late
   app yes still loses to it. Face reads have their own switch in the app
   (`consent.set`'s `face_expressions`, a later slice), so the app's desk
   off and on leave them as they are. The stop's answer counts face reads
   too: with only faces on it says it stopped, in its own line
   (`seen_yes::STOPPED_FACES`, since lamp-seen says back only seen
   activity's change). `look_at_my_desk` ("start looking again") keeps the
   owner's own spoken yes with no offer and no "turn it on in the app",
   turning a no in either file back to a yes, and says it is on already
   only when both were (`LOOKING_FACES` when only faces come back); the
   model judges the phrase from the two descriptions, and no function is
   added. A forget removes only a kept yes and keeps a no, so it never
   turns noticing on, and lamp-seen's forget latch holds only an explicit
   yes, never the default's: the default is only a missing file
   (`consent::Kept::by_default`), never a file's text, `"by":"default"`
   or not. The voice's start and setup's walk leave a kept no in place too
   ([onboarding.md](onboarding.md), "Room sounds"). The reset still removes
   the file, the no too, so the next setup starts at the default.
2. **The ring shows its mark whenever a frame is going out.**
3. **No frames** while the privacy slide is on, nobody is at the desk, or
   the lamp is asleep. Phase 0 also closed on a stranger in view; the
   owner removed that check on #6543 (17 Nov: "just remove it completely,
   no stranger check needed"), so anyone at the desk is seen once the
   owner opted in ([No stranger check](#no-stranger-check)).
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
  | Awake | `expression/state` (`says_asleep()`, valid and enabled) | asleep, falling asleep, disabled or stale |

  Presence is `room/presence`, the one signal every reader uses, rather than
  `room/activity`'s `at_desk`. Presence can be shown by a voice alone, but a
  voice puts nobody at the desk to see. So a face in view on
  `humans/faces/tracked` is also needed.

  Whose face it is, and how many, is not asked: there is no stranger
  check ([No stranger check](#no-stranger-check)), and `lamp-seen` does not
  read `humans/familiar`.

- **The frames** (`frames.rs`). These follow the frame link's rules
  (`crates/camera/src/frames.rs`, the `perception` role):
  - one request at a time, only while the gate is open in that same step;
  - the frame is copied into one buffer of this part's own before anything
    else;
  - the first frame's `CameraGrant` is kept, and a frame under another grant
    is dropped;
  - when the gate closes for any reason, or the link closes or fails, which
    includes the camera's privacy close, everything goes: the connection,
    its partial frame, the request in flight, the grant and the copy (wiped);
  - the link is read by a thread of its own (`Drained`), every 2 ms while a
    frame is asked for, never by the serve loop: the camera closes a reader
    that leaves a frame unread for 0.5 s (`ConsumerStalled`), its kernel
    buffer holds about 16 KiB, and a full-look frame is several times that,
    so a loop reading once a 100 ms tick, and longer while a frame is
    scaled for sending, lost the link mid-burst (13 of 16 bursts on one
    lamp). The thread copies the whole frame into one slot of its own; the
    next step moves it into the copy above and wipes the slot, and a loss,
    the gate closing or a stop wipes it too.

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

  > On, with `LAMP_SEEN_MORE_DOINGS` on: I'll look now and then to see if
  > you're eating, drinking, taking a break or slouching, if you yawn or
  > stretch, what you're working on at the desk, and if you're
  > celebrating. I keep none of the pictures.
  >
  > On, with both: the same, then "…and now and then how you're sitting,
  > so I can remind you to sit back. I keep none of the pictures."

  The posture line is **awaiting the PM's wording**: the proposed clause
  is "…and now and then how you're sitting, so I can remind you to sit
  back." It is one constant, `said_back::POSTURE_ON_LINE`. The more
  doings' lines (`said_back::MORE_ON_LINE`, `POSTURE_MORE_ON_LINE`) name
  everything the lamp then reads, the first builder's condition on #6543,
  and await the PM's wording too.

  - Once the voice has begun a line (`{"event":"begun"}`) it counts as
    said, and what it said is kept in `lamp-seen`'s own state folder
    (`--state-folder`, `/var/lib/lampos-seen` by default), in
    `said-back.json`: `{"said":"on","for_at_unix_s":N}` for the yes it
    was for, with `"posture":true` when the line said named posture and
    `"more":true` when it named tiredness, desk work and celebrating, or
    `{"said":"off"}`. A record with no `posture` (one kept before posture)
    is the plain line, and the same for `more`: a yes whose line did not
    name them is owed the line that does, once. Never under `consent/`. A restart says
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
  (`activity::FRAMES`, the server's clip). With `LAMP_SEEN_BURST_15S=on`
  (exactly; dark, no unit sets it), the cloud may ask for one as often as
  every 15 s (`activity::FAST_EVERY_US`, the owner's call on #7401, 19 Nov,
  for the seen reactions) through the `/ping` reply's `seen_clip`
  (`clip.rs`): lamp-cloud keeps it as
  `/var/lib/lampos-cloud/seen-clip.json`, `{"every_s": N, "until_unix_s":
  T}` in whole seconds (cloud.md), and lamp-seen reads it again every 2 s
  (`opt_in::READ_EVERY`). It is a minute whenever lamp-seen hasn't heard:
  no file, one that can't be read or is over 4096 bytes, not exactly those
  two whole numbers, `every_s` outside 15–60 s, or `until_unix_s` reached.
  With the switch off it is a minute whatever the cloud says; the heat
  pace multiplies the period in force ("The board's heat"). The seen
  reactions' switch-on (#7668) leaves it dark: it waits for a heat read
  and the perception service's capacity confirmation, and no product unit
  sets it (`no_product_unit_sets_the_fifteen_second_switch`), so every
  lamp stays at a burst a minute until a unit sets it and Backend sends
  `seen_clip`. Nor do the reactions send anything: they read
  `humans/doing_more` on the lamp and make only a turn and a line
  (skills.md, the seen reactions' row), so nothing seen leaves the lamp
  but what lamp-seen already sends and counts here. At 15 s,
  against a minute, a seated lamp then sends the
  perception server four times the bursts and spends about four times the
  encode and TLS work. At either rate a burst under way holds the next back
  until it ends, so two never overlap and none is queued. The sink wants a frame
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
  raw label. A false "drank" silences sip-of-water for 30 minutes, so a
  kind counts only when one answer is at least 0.5 sure of it, or at least
  2 of the burst's last 4 answers have it at all (the server sends only
  what clears 0.3).
- **The sitting and its timers.** A sitting begins when someone is at the
  desk with a face in view (the gate's presence line, whatever the
  opt-in, its said-back line, the privacy or sleep say), from `room/activity`'s `sitting_s`
  when it said one lately, else from now, but never from before the
  chair last emptied (`keep::Seat`, `keep::chair_emptied`): nobody at the
  desk for 120 s (`Why::Expired`), or a held sitting ended on another face
  (`hold::End::OtherFace`, below). `sitting_s` is the room's, forgives a
  gap under two minutes and knows no face, so the next person's timers
  counted the first one's time (lamp-06c1, run 8: `break_s_ago` 2191 a
  minute after the hold carried over; the first builder's rule on #6543).
  A stand-up, a forget, and the gate opening on a new passing id without
  the hold (most often the same person looking back up) begin from
  `sitting_s` as before. It ends at once on
  `room/stood_up` or a forget; a stranger, a second face or a misread ends
  nothing (no stranger check). When the gate's presence line says nobody,
  it is kept, unsaid, for up to 120 s and goes on if the face is back
  within them on the same passing id, or within 4 s on a new id in the
  same place with one face in view (another id starts afresh); past
  them it ends (below, "A short closing keeps the sitting"). Its timers count from the desk arrival
  until something is seen, and from the last sighting after.
- **Published**, on the topics `lamp_msgs` declares (`doing.rs`):
  - `humans/doing` (`HumanDoing`): `drank_s_ago`, `ate_s_ago`,
    `break_s_ago` (always present), `last` and `confidence`, every 30 s
    while a sitting lasts (not while it is kept through a short
    closing), and at once, with `humans/doing_more`, in the tick of a
    burst's answer that saw a kind of it or raised any count
    (`drank_seen`, `ate_seen`, `tired_seen`, `yawn_seen`, `stretch_seen`,
    `celebrate_seen`; #7401: a
    count up is a seen reaction right away, not up to 30 s later); the
    30 s beat goes on from that saying.
    It goes out whether or not frames do (the opt-in off, the server busy,
    the slide shut, no account given), so a skill's `[[while]] above` on a
    timer runs on the clock as it would without this part;
  - `humans/doing_seen` (`DoingSeen`): once for each kind newly seen, not
    seen in the burst before that answered;
  - with `LAMP_SEEN_MORE_DOINGS=on` only, `humans/doing_more`
    (`HumanDoingMore`), with `humans/doing` and as often: `tired_s_ago`
    and `tired_seen` (V1's `tired` group, a yawn, and a stretch of an arm
    or a leg, which still counts as a break on `humans/doing` too),
    `yawn_s_ago` and `yawn_seen`, and `stretch_s_ago` and `stretch_seen`
    (#8123: the two apart, each by the same sureness rule and counted as
    `tired_seen` is, so Seen tired names only what was seen; a burst that
    newly saw both counts only the surer, a yawn when as sure, and the
    other goes on as already seen, so a yawn while stretching is one
    count; tiredness counted only from the two together, an unsure yawn
    and an unsure stretch in a run, raises `tired_seen` and neither;
    `tired_s_ago` and `tired_seen` stay the two together; each read as 0
    when absent, from an older `lamp-seen`),
    `desk_work_s_ago` with a coarse `desk_work` (V1's `sedentary` group:
    "using computer" and "playing controller" are `computer`, "texting"
    `phone`, "reading", "reading book" and "reading newspaper" `reading`,
    "writing" and "drawing" `writing`; the kind of the surest such label)
    and `desk_s`, this sitting's desk-work time in seconds: the time
    between two bursts in a row that both saw desk work, each gap counted
    up to a minute (the default burst period, at either rate), so a missed burst (busy, failed,
    paused) never counts the long gap it leaves, and a burst without desk
    work ends the run (0 before two such bursts; a number only, no
    picture or label kept),
    and `celebrate_seen` (V1's `celebrate` group), how often celebrating
    was newly seen this sitting, counted as `tired_seen` is (#7401: each
    one seen is one count, for the seen reactions), and `drank_seen` and
    `ate_seen`, how many drinks and how much eating were newly seen this
    sitting: exactly the sightings `humans/doing_seen` says, so a count is
    only of what was seen (the first builder's fourth change on #6543),
    for A comfort break's "after 2 drinks". Each count is of a move newly
    seen: one seen in two bursts in a row that answered is one, and a burst
    that did not answer (busy, failed) is no burst between them, so one
    sip, bite, yawn or cheer caught either side of it counts once
    (`Sitting::took`, the owner's call on #7401). Every field always
    there, each timer counting from the desk arrival; the same sureness
    rule, the same bursts, no more pictures, wiped with the sitting. They
    never set `last` or say `humans/doing_seen`: desk work shows in almost
    every burst and would push a drink out of the second voice's note.
    V1's `emotional` group (laughing, crying, singing) is never said. Its
    own topic, so a skill given drinking, eating and breaks never receives
    these; off, it is not even declared. Nothing of it is timed or said
    until a line naming it (`said_back::MORE_ON_LINE`, above) has begun for
    the kept yes, nor while the opt-in is off: then what was seen is
    forgotten at once, each timer counting from the arrival again, so the
    opt-in off stops all of it (the first builder's condition on #6543).
    The four seen reactions hear it under the same yes: the owner's
    desk-noticing yes covers them (the owner's call, 23 Nov on #7668,
    "make the desk-noticing yes toggle covers these 4 too"), so
    lamp-skills gives `seen-drink`, `seen-eat`, `seen-tired` and
    `seen-celebrate` the topic while `seen-activity.json` holds the yes,
    read by `lamp_msgs::consent::allowed`'s rule as here, with no
    `seen_more` line of their own (`lamp_skills::SEEN_PRODUCT`,
    skills.md, "The owner's seen_more yes"). Any other skill still needs
    its own `seen_more` yes. The yes taken back or a forget takes it away.
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

### A break is a stand-up (`LAMP_SEEN_BREAK_IS_STANDING`)

A stretch says someone is tired and needs a break, not that they took one
(the owner, #6543 step 2). With `LAMP_SEEN_BREAK_IS_STANDING=on` exactly
(off unless set, on in the product unit; `sitting::BREAK_IS_STANDING`,
read once at the start), a
break is a stand-up:

- **A stretch is not a break.** A burst that sees stretching (V1's `break`
  group) no longer restarts `break_s_ago`, becomes `last` (and its
  `confidence`) or is said on `humans/doing_seen`; a burst that saw only
  that does not say `humans/doing` at once (`Sitting::takes`). Drinking and
  eating are unchanged.
- **The break counts from their return.** `room/stood_up` already ends the
  sitting, so its timers begin again when someone is back at the desk. That new sitting counts from `room/activity`'s `sitting_s`, and
  the eyes keep a sitting through a gap under two minutes, so after a short
  stand-up the arrival can still be the first one. With the switch on, a
  sitting begun after a `room/stood_up` that came at or after its arrival
  counts `break_s_ago` from the step it began, their return
  (`Sitting::break_is_standing`); the other timers keep the arrival. A
  `room/stood_up` is what the eyes say (eyes.md): the sitter's face rose or
  shrank as it was lost, and was not seen again for 2 s. No shorter
  stand-up is left out: standing up and walking off is the break Stretch
  break and Stand up invite. Away longer than two minutes, or gone with no
  `room/stood_up` said, the sitting is new and every timer counts from it,
  as before.
- **What the readers see.** Stretch break (`break_s_ago` above 1799) and
  Stand up (above 5999) hold for that long after a stand-up, not after a
  stretch: a stretch at the desk no longer silences them (case
  `quiet-after-a-stand-up`). The second voice's note (`doing_told.rs`) is
  never "seen stretching", since `last` is never `break`.

Off, `humans/doing` and `humans/doing_seen` are byte for byte as before.
Tests: `sitting.rs`, `switch_off_a_stretch_is_a_break`,
`switch_on_a_stretch_is_not_a_break` and
`switch_on_a_break_counts_from_the_return_after_a_stand_up`.

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
- `lamp-seen` no longer reads `humans/familiar` (no stranger check). The
  product unit still gives the eyes `--visits` with the switch on, as it
  did ([The product unit](#the-product-unit)).

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
     `{"type":"config","task":"pose","frame_interval":1.0,"confidence_threshold_2d":0.3,"min_valid_keypoints":5,"ergo_mode":"desk"}`.
     V1 sent none, so these are the server's defaults. The server's
     sessions share one config object, so any other value would change
     every V1 lamp's results. It adds `"ergo_mode":"desk"`, which the
     server keeps for this session alone: the 2D neck signed by the facing
     side and, with the hips hidden, the upper body scored alone
     (`ergo.partial`, below). A server without it ignores the field (its
     config model forbids no extra field), so no switch holds it. The
     answer must be `config_updated`;
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
  - `window_s` is how long this sitting has been watched, at most 600 s;
  - a partial answer (`ergo.partial: true`: the hips hidden, the upper body
    scored alone) is bad only when the neck subscore on either side is 4
    or more, and its region can only be neck. The server fixes trunk and
    legs at 1 and caps `risk_level` low, and it measures the upper arm
    against a trunk made from the shoulder line, so neither `risk_level`
    nor trunk, the arms or wrist count. An answer with no `partial` field
    (an older server) is full and judged as above, so no switch holds this.

  An answer with no body found counts no sample. A sample is its time, one
  flag and four bits: nothing of the frame or the answer is kept. The
  window is forgotten at once on `room/stood_up` and when posture's gate
  closes for the privacy, sleep, the opt-in off or a forget; a stranger,
  a second face or a misread closes nothing. Any other closing (nobody in
  view, the face lost, a reading gone stale) holds it unsaid for at most
  120 s (below, "A short closing keeps the sitting"); it goes on
  if the gate opens again within them, and is forgotten after.
- **Said on `humans/posture`** (`lamp_msgs/HumanPosture` v1:
  `{window_s, bad_share, region}`) every 30 s while posture's gate is
  open and the window holds at least 3 samples: as soon as the sitting's
  third sample lands, then every 30 s. Closed, the topic says nothing and
  its last value runs out, so no old sample is said while the privacy is
  on. **Under 3 samples, nothing is said either** (#6839's and #6660's
  reviews): frames not going out (the opt-in off, the slide shut), the
  server busy or failing, no body found, the sitting just begun, or the
  window just wiped (the privacy, sleep, `room/stood_up`, or
  a closing longer than 120 s).
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

## No stranger check

The owner's word on #6543 (17 Nov, after lamp-06c1's run 6): "no stranger
interrupt this activity and posture flow", then "just remove it
completely, no stranger check needed". In run 6, with the lamp angled to
the side, the eyes read the owner as a stranger three times (11:38:32,
11:46:43, 11:48:07); each wiped posture's window and the sitting's timers
at once (`gate stranger`), one ended the held sitting as another person
alone in the seat, and a drink seen was lost.

So `lamp-seen` has no stranger rule anywhere, with or without a switch:

- **The gate** (`gate.rs`) asks only the opt-in, its said-back line, the
  privacy, someone at the desk with a face in view, and that the lamp is
  awake. A guest, a face nobody remembers, a misread of the owner and a
  second face beside them all leave it open. There is no `stranger` word
  on `diagnostics` or in the journal any more.
- **The keep** (`keep.rs`) wipes at once only on `room/stood_up`, a forget,
  the opt-in off, the privacy and sleep.
- **The pause on a misread** (once `pause.rs`) is gone: with no stranger
  reading there is nothing to pause on, and `diagnostics` no longer says
  `paused` or `misread_on_track`.
- **The held sitting** (`hold.rs`) no longer ends on another person alone
  in the seat as a stranger. It ends on another passing id only once the
  chair may have emptied (a pose answer with no close person since the
  face was lost): two ids from the eyes and the shoulders compared, never
  who anyone is.
- **`humans/familiar`** is not read by `lamp-seen`.

What this agrees to: once the owner has opted in and been told, pictures
go whenever someone is at the desk with a face in view, whoever they are,
under the same mark, rates, privacy slide and sleep rules. The keep still
starts afresh when a face comes back under another passing id after a
short closing, so a window and timers kept for one face are not taken on
by the next, unless that id is the one face in view, within 4 s, in the
same place (below, "A short closing keeps the sitting"): a timer only,
never anything kept about a person.

**Whose sitting it is (the bound).** With no stranger check, the window
and timers are the seat's, not a person's, and they can be carried from
the owner onto someone who takes the seat:

- With two faces in view no id is matched (`watch::Readings::on_track`
  is none), so the keep goes on holding the id it had before the second
  face came (`keep::Away`). While the gate stays open, nothing compares
  ids, so the owner leaving with a guest still in view carries the
  owner's posture window, timers and drink seen on under the guest.
- The eyes' tracker may also hand the owner's passing id to whoever sits
  down within about 1.5 s of them leaving, and that is the same id to the
  keep.
- After a closing, a new passing id alone in view within 4 s, its box's
  centre within its width of the kept one's, carries the window and
  timers (`Step::Carried`), so someone else sitting down in the same spot
  that fast is carried too. The first builder took that for a timer on
  #6543; the keep holds timers only (`keep::Away::timer`), nothing about
  a person.
- **The bounds:** `room/stood_up` or a forget wipes both at once; nobody
  at the desk for 120 s wipes both (`keep::KEEP_US`); posture's samples
  age out of its ten-minute window, so after 10 minutes it holds only the
  one sitting now. Held without a face (`LAMP_SEEN_HOLDS_SITTING`), the
  sitting ends when, after the face was lost and an answer with no close
  person, a face comes into view under another passing id than the one it
  was open on (`hold::End::OtherFace`, which wipes both and starts the
  next sitting from that wipe), and at the 10-minute re-check since the gate was last
  open on a face (`hold::MOST_US`) at the latest. A guest who never shows
  a face, or who sits down with a close person in every answer, is
  carried until then. The keep then holds the id it
  had, so the guest's own face opening the gate wipes both as another face
  (`Why::OtherFace`), as the test
  `the_owner_leaving_and_a_guest_sitting_within_1_5_s_ends_at_the_10_min_re_check`
  shows.
- **The effect** is a wrong count or timing, never a name: nothing is said
  or kept with a name, and the voice's `doing_told` still speaks only to
  a known person alone.

## A short closing keeps the sitting

On lamp-06c1's run 3 (16 Nov) the gate closed four times in eight minutes
on brief misreads and the face lost for a moment, and each closing forgot
the posture window. With 3 samples wanted and one each 30 s, posture was
almost never said. So a closing that is not one of those that wipe at
once keeps the posture window and the doing timers, in memory only, for
up to 120 s (`keep.rs`, `KEEP_US`):

- **Wiped at once:** `room/stood_up`; a forget (`memory/forgot`); the
  opt-in off; the privacy closed (the slide, or capture muted); the lamp
  asleep. The doing timers keep their rule for the privacy, sleep and the
  opt-in: a sitting never ends on those alone. A stranger, a second face
  or a misread wipes nothing ([No stranger check](#no-stranger-check)).
- **Kept, for up to 120 s:** any other closing. Nobody in view, the face
  lost, and a reading gone stale (presence, the privacy lease, sleep not
  known).
- **Nothing is sent or said while kept.** The gate is unchanged: no frame
  is asked of the camera, a burst or pose request under way is stopped and
  its answer dropped, and nothing kept is said (no `humans/posture`,
  `humans/doing` or `humans/doing_seen`).
- **Goes on** when the gate opens again within the 120 s, and only on the
  face it was kept for. The keep holds, in memory only and never said, the
  passing id and box of the one face in view the last step it was open
  (`keep::Away`). The same id goes on. A new id goes on too
  (`Step::Carried`) only when all of these hold; otherwise the gate
  opening on it, or with no id known when it was kept, wipes both
  (`Why::OtherFace`), so a second person within the 120 s never takes on
  the first one's window or timers (the Reviewer-lead's P2 on #7251).
  Past 120 s both are wiped, as any closing wiped them before. The
  posture window's samples older than its ten minutes still age out.

  | Carry rule | Value | Source |
  |---|---|---|
  | Faces in view | exactly one | `watch::Readings::on_track`, the gate's `faces` |
  | Longest gap since the gate closed | 4 s (`keep::CARRY_GAP_US`) | the eyes' `KNOWN_HOLD` (`crates/eyes/src/who.rs`) |
  | Same place | centres apart at most the new box's width | the eyes' follow rule (`crates/eyes/src/bus_out.rs`, #4885) |

  Why: the eyes keep a passing id only 1.5 s (`SAME_WITHIN_US`), and the
  gate reads nobody only after a 2 s stale face list
  (`watch::FACES_LAST_US`), so after any closing the same face always
  comes back under a new id. On lamp-06c1 (23 Nov) a slump gave `gate
  nobody` at 15:51:06 and then `posture's window wiped: the gate opened
  again on another face than the one it was kept for` at 15:51:08. It is
  not face recognition: no name or print is compared.

  **Timers only** (the first builder's condition on #6543): the keep
  holds posture's window and the sitting's timers (break, drink, eating,
  desk time and the counts), never a name, an identity, a memory or a
  per-person record. Only a timer's keep (`keep::Away::timer`) carries;
  `keep::Away::default`, the rule for anything kept about a person, goes
  on only on the same id
  (`a_keep_about_a_person_never_carries_onto_a_new_id`).
- **The journal** says, once each: `posture's window kept unsaid for up to
  120 s: the gate closed (nobody)`, `posture's window goes on: the gate
  opened again within 120 s`, `posture's window goes on: the gate opened
  again within 4 s on a new passing id in the same place (f1 to f3 after
  2000 ms: centres 0.010 apart, within the new box's width 0.200)`,
  `posture's window wiped: the gate stayed closed 120 s (nobody)`,
  `posture's window wiped: the gate opened again on another face than the
  one it was kept for (f1 to f3 after 5000 ms, over 4000 ms)` (or the
  centres over the width, `2 faces in view`, or `no face known when it was
  kept`) or `posture's window wiped at once (privacy_closed)`, and the
  same for `the sitting's timers`. The ids are the eyes' passing ids, gone
  with the face, never a name; the boxes are not written.

No new switch: it is in memory only, sends nothing, and runs only with
`LAMP_SEEN_ACTIVITY=on`.

## The held sitting

The owner's rule (#6543, c6908160252), behind its own switch,
`LAMP_SEEN_HOLDS_SITTING` (`on` exactly, off unless set, on in the product
unit; [switches.md](switches.md)). In run 3 the face left
view while the owner slumped, so posture was said once in seven and a half
minutes. Held, the sitting goes on without the face while the pose
model sees a close person (`hold.rs`):

- **Begins** only when the gate opens as it always does: the opt-in on and
  said, the privacy open, the lamp awake, and someone at the desk with a
  face in view (no stranger check, above).
- **Goes on without the face:** while held, nobody in view or the face
  lost do not close the gate, as long as the newest pose answer
  shows a close person: both shoulders (COCO-17's 5 and 6 in the answer's
  `pose_2d`, pixels of the frame as sent) at or above the model's own
  confidence threshold, 0.3 (`hold::CONFIDENCE`, as the lamp configures
  it), and at least 15% of the frame's width apart (`hold::CLOSE_SHARE`, to
  be tuned on a lamp). Posture keeps its frame every 30 s and activity its
  bursts, each behind the ring's mark; the short-closing keep sees the gate
  open meanwhile. The opt-in, its line, the privacy and sleep
  still hold every step: a privacy lease or a state not known closes that
  step, and the hold stays.
- **Ends**, the gate closed as it would be, then the short-closing keep
  above applies: two answers in a row with no close person (one is forgiven,
  so at most about a minute of frames with none); the face lost and no close
  person's answer within 45 s of the hold's beginning, below;
  `room/stood_up`; the privacy slide or capture muted; sleep; the opt-in off
  or its line withdrawn; a forget; or 10 minutes since the gate was last
  open on a face (`hold::MOST_US`). **Another face once the chair
  emptied** ends it too: the hold keeps, in memory only and never said,
  the passing id of the one face in view the last step the gate was open
  on one, as the keep does. Once the face was lost and at least one pose
  answer since showed no close person, the one face in view coming back
  under another id (or with none known, as when it began on two faces)
  ends it and wipes the posture window and the sitting's timers as the
  keep's `Why::OtherFace` does, and the next sitting counts from that
  wipe. On lamp-06c1's run 8 (18 Nov, #7506) the owner hid at 07:20:04,
  the hold went on on a stale close answer, one answer at 07:20:33 had no
  close person, and another face came into view at 07:20:45 under its own
  id and took on the owner's sitting. With a close person in every answer
  it is the same body in the chair, and a new id is them looking back up
  (the eyes give a face turned away and back a new passing id as a rule,
  [eyes.md](eyes.md)): the hold goes on, on the new id (Heron's P1 on
  #7635). With two faces in view no id is matched and nothing ends. Only
  two ids from the eyes are compared: who anyone is is never asked ([No
  stranger check](#no-stranger-check); #7256's stranger stop stays
  removed).
- **The first answer** (lamp-06c1, run 4: the face lost seconds after
  every beginning, while the first answer was ~30 s away): a hold begins
  with the newest pose answer when its frame went under 30 s ago
  (`Pose::recent_answer`, forgotten with the window), since the 30 s rate
  lets no new frame go yet; otherwise posture's next frame is already due
  and goes at the ring's next mark. Until a close person's answer comes, a
  lost face keeps the hold for at most 45 s since it began
  (`hold::FIRST_ANSWER_US`), with no frames beyond posture's and activity's
  own rates; with none by then (no answer, or only a far one), it ends.
- **Nothing kept:** an answer's two shoulders are read as a share of the
  width and dropped; no frame or point is kept, the one face's passing id
  is held in memory only, and the rates are unchanged.
- **What it agrees to:** for up to 10 minutes after the gate was last open
  on a face, pictures may go while no face is visible, as long as someone
  is close to the lamp. That includes someone else taking the seat without
  showing a face, and a second person nearby (the owner's call); it ends
  at the 10-minute re-check at the latest, and at once when another face
  shows after an answer with no close person.
- **Without posture** (`LAMP_SEEN_POSTURE` off) no pose answer ever comes,
  so a hold ends as soon as the face is lost, with no 45 s wait: nothing
  changes.
- **The journal:** `held sitting began: the gate open on a face`;
  `held sitting goes on (close person, shoulders 23% of width)` when it
  first holds the gate open and with each close answer then; `held sitting
  goes on (no close person in 1 answer of 2, shoulders 8% of width)`; `held
  sitting ended (no close person in 2 answers in a row)`, or `(stood_up)`,
  `(privacy_closed)`, `(asleep)`, `(no_opt_in)`, `(not_said_yet)`,
  `(forgot)`, `(the face lost and no close posture answer within 45 s)`,
  `(10 min since the gate was open on a face)`, `(another face in view
  than the one it was held for)`.

Off, `serve` is as without it: the shoulders are read from each answer and
dropped, and nothing else changes.

## The board's heat (`LAMP_HEAT_BACKOFF`)

Off unless `LAMP_HEAT_BACKOFF=on` exactly, read once at start, in no unit
yet (#7879, slice 3; lamp-health says the level, health.md "The board's
heat"). On, `lamp-seen` reads the kept `board/heat` at most once a second
(`heat.rs`, `HeatIn`), as lamp-body reads `room/presence`: a non-blocking
read of the kept value, with no subscriber name. A value past its 15 s, from
the future or not valid is ok: full speed.

| Board | Bursts, start to start | Posture frames |
|---|---|---|
| ok | 60 s (`activity::EVERY_US`) | every 30 s (`pose::EVERY_US`) |
| warm | 120 s | every 30 s |
| hot | 240 s | one a minute (never paused) |

- **A held sitting keeps posture at 30 s** (`Pace::holding`, Heron's HOLD
  on #7927): while a hold is open (`LAMP_SEEN_HOLDS_SITTING`, "Held
  sitting"), posture frames come every 30 s at any level, since a hold ends
  with no close answer after 45 s (`hold::FIRST_ANSWER_US`) and ends on
  someone else after two far answers in a row (`hold::FAR_IN_ROW`). Bursts
  keep the level's pace. The window and the swap are as at ok.

- **Only the next one moves:** a change counts from the last burst's start
  or the last posture frame sent, so a hot board that cools brings the next
  burst back to a minute after the last.
- **Unchanged at every level:** the frames 1.2 s apart within a burst
  (`SPACING_US`, the server's clip), a burst under way (it finishes), the
  opt-in read every 2 s, and the gate closing on a stop, a forget, the
  privacy or sleep (`Sink::stopped`). So no yes or stop is ever missed.
- **The journal:** one line per pace change, for example `lamp-seen: board
  hot: bursts every 240 s, posture one a minute` and `lamp-seen: board ok:
  bursts every 60 s`. lamp-health's hysteresis keeps these to one change in
  10 to 60 s at most (hot after 10 s, or at once when the board is held back).
- **Tests:** `heat::tests` (each level's pace, the switch off, the line, a
  stale, future or invalid value read as ok, a held sitting's posture at
  hot), `activity::tests` (the next
  burst at 240 s on a hot board and back; a stop still aborts a burst at
  hot), `pose::tests` (one frame a minute at hot, not paused; a closed gate
  still stops the request).

## The journal

With activity on, `lamp-seen`'s journal says each burst and each posture
sample, and what the server answered: diagnostics only, never a picture, an
id or a name. A burst a minute and a sample each 30 s keep it to a few
lines a minute (about fourteen at the cloud's fastest `seen_clip`, 15 s, with `LAMP_SEEN_BURST_15S=on`).

By default the lines say only counts and outcomes: no label, confidence,
score, risk, bad or good flag, region or bad share. The journal is kept on
the lamp across reboots, and a forget or the opt-in going off does not
clear it, so a line naming what was seen would be a timed record of what
the person did (the Reviewer-lead's P2 on #7251). The diagnostics switch
`LAMP_SEEN_JOURNAL_LABELS=on` (exactly `on`; in no unit) adds them, as the
"with labels" lines below show, and `serve` says once at start that it is
on.

`lamp-skills` keeps the same rule for the skills that read
`humans/doing_more` (the privacy read on #8375): a seen reaction's name
says what was seen, so with the switch off in lamp-skills' environment
(read once at its start) its journal lines say `a seen reaction` in place
of the skill, and `a reading` in place of a kind's reading, for example
`lamp-skills: a seen reaction Triggered: humans/doing_more has held for 0
s` rather than `seen-drink Triggered: …` ([skills.md](skills.md), "Journal
names"). On, they name it.

| When | Line |
|---|---|
| A burst's first frame opens its socket | `burst started` |
| All 8 frames handed to it | `burst sent: 8 frames, 61234 bytes` |
| Answered | `burst answered: 1 kind counted; server 412 ms a frame, at most 530; 8 of 8 frames, 61234 bytes, 9874 ms in all` |
| Answered, with labels | `burst answered: top drinking 0.81; counted drinking 0.81; server 412 ms a frame, at most 530; 8 of 8 frames, 61234 bytes, 9874 ms in all` |
| Busy | `burst busy: the server is busy, the burst dropped; 3 of 8 frames, 22410 bytes, 2810 ms in all; bursts held back 60 s` (when the retreat holds) |
| Failed | `burst failed (refused, HTTP 503); 1 of 8 frames, 7650 bytes, 80 ms in all; bursts held back 60 s` (or `no_answer`, `connect`, …) |
| Stopped | `burst aborted (gate nobody) after 2 frames, 15300 bytes: its answers dropped` |
| A posture frame handed to its socket | `posture sample sent: 7650 bytes` |
| Its answer | `posture sample answered: scored, 640 ms`, or `no body found`, `busy`, `failed (handshake_5xx)`, `cancelled: the gate closed` |
| Its answer, with labels | `posture sample answered: score 5, risk 3, bad, region neck, 640 ms` (`score 3, risk 2, partial, bad, …` on a partial answer) |
| `humans/posture` said | `posture said: window 93 s, 4 samples` |
| `humans/posture` said, with labels | `posture said: window 93 s, 4 samples, bad_share 0.75, region neck` |

With labels, `top` is the surest label any frame's answer gave; `counted` is what the
burst counts as drinking, eating or a break (`sitting.rs`), or `none`. The
server's time is from a frame sent to its answer read. The retreat's hold is
the bursts' own (`backoff::Retreat`): the server's `Retry-After` is not
read on these sockets.

## The product unit

`deploy/product/lamp-seen.service`, installed and enabled on every lamp by
`lamp-update install` and carried by each build (`install::PRODUCT`; the
binary is in `optional` in `deploy/package-binaries.json`, beside the
other parts with a unit). It sets `LAMP_SEEN_ACTIVITY`,
`LAMP_SEEN_POSTURE`, `LAMP_SEEN_HOLDS_SITTING`,
`LAMP_SEEN_BREAK_IS_STANDING` and `LAMP_SEEN_MORE_DOINGS` on (#6543's
switch-on; the voice's unit sets the first two, lamp-cloud's the first,
for the owner's yes); `LAMP_SEEN_JOURNAL_LABELS` is in no unit. With
`LAMP_SEEN_ACTIVITY` unset (a drop-in, or another unit), it idles and
touches nothing, and the eyes serve no link.

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
- **The eyes' visits.** The same file's second line,
  `LAMP_EYES_VISITS=--visits on`, kept alone beside it as
  `/run/lampos-seen/eyes-visits.env` (the one file the eyes' unit loads,
  `EnvironmentFile=-`, and removed with the switch off), starts the eyes
  with `--visits` (they
  already run with `LAMP_BUS=on`), so `humans/familiar` says the level of
  the one at the lamp, with no name, face or picture. It was for the gate's
  stranger check (the first builder's yes on #6543), which is gone; the
  line is kept as it was, and `lamp-seen` no longer reads the level. Off,
  the eyes run without it, as before.
- **Not in `lamp/health`'s expected parts**: off, it says nothing on
  `diagnostics`, so expecting it would mark every lamp stale. It goes in
  with the switch.

## Owed

| Item | What | When |
|---|---|---|
| Giving the yes | Built in #6912 (onboarding.md "Seen activity"): the voice's offer naming the server (only with `LAMP_SEEN_VOICE_OFFER=on`, off in the product, where the yes comes from the app), counted only when played whole and uncut, then only the owner's spoken yes in the very next turn writes `seen-activity.json`; the key in `lamp_msgs::consent::FILES` and `lamp-cloud`'s `consent.set`; removal on every forget and "stop looking" (Fourteen's design on #6543; the PM's wording). Owed: the backend's written retention confirmation behind "deleted once they're checked" | before the switch goes on, with #6912's reads and that confirmation |
| Posture's on line wording | The posture line (`said_back::POSTURE_ON_LINE`, above) is gated in code but awaits the PM's wording | before `LAMP_SEEN_POSTURE` is on anywhere |
| The more doings' on line wording | `said_back::MORE_ON_LINE` and `POSTURE_MORE_ON_LINE` (above) are gated in code but await the PM's wording | before `LAMP_SEEN_MORE_DOINGS` is on anywhere |
| Posture at 320×240 | A slump read on `humans/posture` from frames sent at 320×240 (every frame is, above) against V1's 640×480, to see whether the 3D lifter's scale changes RULA (above) | with the lamp rows |
| The frame size | Every frame is sent at 320×240, scaled down from the eyes' link at any larger size (`--full-look` included). Measure the labels at 320×240 on a lamp against V1's 640×480 before the switch goes on | before the switch goes on |
| `humans/doing` when lamp-seen stops | A topic that has run out holds a skill's `[[while]]` closed until lamp-skills restarts (`unknown = "holds"` covers only a topic never said). lamp-seen now runs under the keeper (above); the skills change (`unknown = "holds_stale"`) is still owed before a skill reads it | before a skill reads it |
| Skills | They read the labels with the existing `[[while]]` on a topic's field (for example `topic = "humans/doing"`, `field = "drank_s_ago"`); no new skill key | after the `humans/doing` row above |
| `lamp/health`'s expected parts | `--expect seen` in `lamp-health.service` once the switch is on in the product (off, it says nothing) | with the switch |

V1 faults not ported: activity's per-face mode sent double events (one
burst here, for the one face), its heartbeat waited with no
timeout (`motion.py:208`; no heartbeat here, and every answer is bounded at
V1's 15 s, `wire::ANSWER_WITHIN_S`), and it reconnected every 2 s with no
backoff (here, the retreat).
