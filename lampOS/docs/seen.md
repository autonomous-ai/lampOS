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

It is off behind `LAMP_SEEN_ACTIVITY` ([switches.md](switches.md)). Phase 1,
described here, makes no server call, so **no frame leaves the lamp**.

## Phase 0: the rules

The rules are as phase 0 has them on #6543, with the first builder's
seventh line. Phase 2 waits for the owner's or the lead's ruling on them.

1. **The opt-in is off by default.** It is the owner's yes on that lamp,
   given in the app or spoken.
2. **The ring shows its mark whenever a frame is going out.**
3. **No frames** while the privacy slide is on, a stranger is in view,
   nobody is at the desk, or the lamp is asleep.
4. **Nothing is kept.** A frame is sent, labelled and dropped, on both
   ends.
5. **Forget everything turns the opt-in off.**
6. **Voice never leaves the lamp for this.**
7. **The opt-in is said back once when it's turned on:** "I'll send a
   picture now and then to see if you're eating or drinking, and keep none
   of them."

## What phase 1 does

`crates/seen`, the binary `lamp-seen serve`:

- **The "no frames" gate** (`gate.rs`). This pure function asks for no
  frame unless every rule holds. Anything not known counts as closed. It
  reads only the bus, never the eyes' private files:

  | Rule | Read from | Closed when |
  |---|---|---|
  | The opt-in | the opt-in file (below) | no kept yes, or a forget came after it |
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
  In phase 1 that is `Discard`, and the copy is wiped right after.

- **The opt-in** (`opt_in.rs`). It is read as the app's two yeses are
  ([cloud.md](cloud.md), #5884): a file in `/var/lib/lampOS/consent`,
  present only while the yes stands. Its key must be exactly `"yes"`, and
  anything missing or unreadable is off. **The key and the file are not
  chosen yet.** Fourteen, who owns the app, is choosing them; until then
  they are the placeholders `seen_activity` and `seen-activity.json`, read
  behind a `Reader` trait. Nothing writes them, and `lamp-cloud`'s
  `consent.set` has no such key yet.
- **Forget everything** (`memory/forgot`). Any forget turns the opt-in off.
  This part can't remove the kept yes itself, since only root's helper
  writes the consent folder. So from the forget on it takes a yes only if
  the yes was given after it (a later `at_unix_s`). This holds only while
  the process runs; removing the kept yes is owed (below).
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
  `lamp/health`) whether the gate is open or why not, and its frame counts.

### What phase 1 does not do

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
| The opt-in's key | Fourteen's choice. Then: `lamp_msgs::consent`, `lamp-cloud`'s `consent.set`, and the voice's spoken yes | before phase 2 |
| The ring mark | `light/mark {asker: "lamp_seen", camera, blink}` for each frame sent ([bus.md](bus.md)), and a frame goes only after `light/mark_drawn` for it, as `lamp-control` does | 1b, with the first send |
| The said-back line | Phase 0's seventh line, once, in the voice, when the opt-in turns on | 1b |
| Forget everything, kept | Removing the kept yes through root's helper on `memory/forgot`, so it holds across a restart | 1b, with the key |
| The frame source | A `perception` consumer of the product's camera: joining the eyes' camera and the frame link, or a link from the eyes | phase 2 |
| A known face on the bus | `lamp-eyes --visits` in the product, or another marker the gate can read | phase 2 |
| Labels | `humans/doing` (latest, `lamp_msgs/HumanDoing` v1, every 30 s, lasting 90 s: `{drank_s_ago?, ate_s_ago?, break_s_ago?, last, confidence}`, reset on `room/stood_up`), `humans/doing_seen` (an event for each result), `humans/posture` (latest, `lamp_msgs/HumanPosture` v1: `{window_s, bad_share, region}`). Types and declarations in msgs.md and bus.md | phase 2 (activity), phase 3 (posture) |
| Skills | They read the labels with the existing `[[while]]` on a topic's field (for example `topic = "humans/doing"`, `field = "drank_s_ago"`); no new skill key | phase 2 |
| A product unit and a package | Its unit, `lamp/health`'s expected parts, the units list | phase 2 |

V1 faults not to port: activity's per-face mode sent double events, and its
heartbeat waited with no timeout (`motion.py:208`). Phase 2 bounds every
answer, the heartbeat's too, at V1's 15 s (`wire::ANSWER_WITHIN_S`).
