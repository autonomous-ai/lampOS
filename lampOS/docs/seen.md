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

It is off behind `LAMP_SEEN_ACTIVITY` ([switches.md](switches.md)). Phase 1
and 1b, described here, make no server call, so **no frame leaves the
lamp**.

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
   the pictures." (the first builder's wording). **And once when it's
   turned off** (the lead's addition).

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
  In phase 1 that is `Discard`, and the copy is wiped right after. The sink
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

  - Once the voice has begun a line (`{"event":"begun"}`) it counts as
    said, and what it said is kept in `lamp-seen`'s own state folder
    (`--state-folder`, `/var/lib/lampos-seen` by default), in
    `said-back.json`: `{"said":"on","for_at_unix_s":N}` for the yes it
    was for, or `{"said":"off"}`. Never under `consent/`. A restart says
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
    design, and `lamp-seen`'s unit needs `LAMP_SAY_GROUP` (Owed, below).
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
  jitter. While paused nothing is published, so the skills keep their own
  clock and presence rules.
- **V1's wire** (`wire.rs`), as types only. The activity session
  (`/ws/hal/api/dl/action-analysis/ws`, task `action`) sends a `config`,
  then `heartbeat`s and `frame`s, and gets back `detected_classes`. The pose
  session (`/ws/hal/api/dl/pose-estimation/ws`, task `pose`) gets back
  `pose_2d`, `pose_3d` and `ergo`. It also has V1's label groups (drink,
  eat, break and the rest).
- **Health.** Once a second it says on `diagnostics` (part `seen`, so on
  `lamp/health`) whether the gate is open or why not, its frame counts, and
  its marks: asked, not drawn in time, blinks that ended with no frame
  inside them, and frames dropped with no mark drawn for them or captured
  outside the blink.

### What phase 1 and 1b do not do

- No server call and no socket to it: no frame, label or key leaves the
  lamp.
- No label topic is published.
- No product unit, and no package. The binary is listed under `development`
  in `deploy/package-binaries.json`. The product's frame link has no
  `perception` consumer yet: `lamp-eyes` owns the camera, and the voice's
  camera worker, which serves `frames.rs`, isn't the product's camera
  ([eyes.md](eyes.md#when-the-camera-is-open)). With no `--frames-socket`,
  `lamp-seen` runs the gate and says it on `diagnostics`, and asks for no
  frame.
- `humans/familiar` is said only with `lamp-eyes --visits`, which the
  product doesn't set. Until it does, or until another "this face is known"
  is on the bus, the gate reads every face as a stranger and stays closed.

## Owed

| Item | What | When |
|---|---|---|
| Giving the yes | The voice's spoken yes writing `seen-activity.json`, the key in `lamp_msgs::consent::FILES` and `lamp-cloud`'s `consent.set`, removal on every forget, and the file in lamp-setup's reset list beside the other two (Fourteen's design on #6543) | before phase 2 |
| The state folder | `lamp-seen`'s unit with `StateDirectory=lampos-seen`, in the `lampos` group so it may read the consent folder | phase 2, with the unit |
| The say service for `lamp-seen` | Its unit in `LAMP_SAY_GROUP`, so it may reach the say socket. Without it the on line is never said and the gate never opens | phase 2, with the unit |
| The frame source | A `perception` consumer of the product's camera: joining the eyes' camera and the frame link, or a link from the eyes | phase 2 |
| A known face on the bus | `lamp-eyes --visits` in the product, or another marker the gate can read | phase 2 |
| Labels | `humans/doing` (latest, `lamp_msgs/HumanDoing` v1, every 30 s, lasting 90 s: `{drank_s_ago?, ate_s_ago?, break_s_ago?, last, confidence}`, reset on `room/stood_up`), `humans/doing_seen` (an event for each result), `humans/posture` (latest, `lamp_msgs/HumanPosture` v1: `{window_s, bad_share, region}`). Types and declarations in msgs.md and bus.md | phase 2 (activity), phase 3 (posture) |
| Skills | They read the labels with the existing `[[while]]` on a topic's field (for example `topic = "humans/doing"`, `field = "drank_s_ago"`); no new skill key | phase 2 |
| A product unit and a package | Its unit, `lamp/health`'s expected parts, the units list | phase 2 |

V1 faults not to port: activity's per-face mode sent double events, and its
heartbeat waited with no timeout (`motion.py:208`). Phase 2 bounds every
answer, the heartbeat's too, at V1's 15 s (`wire::ANSWER_WITHIN_S`).
