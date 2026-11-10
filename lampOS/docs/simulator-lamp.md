# The virtual lamp: a lamp that runs on a desktop

Design for the first builder's yes, by sim-1 and sim-2. Nothing in it is built yet.

Goal: a lamp that runs on a desktop, looks and moves like the real one, and is
indistinguishable to the SDK (Python, JS) and lamp-mcp.

## Decided by the first builder (PM), 10 Nov 17:20, on #5385
- The body is the real `lamp-body` on a simulated servo bus (a small crate, `crates/sim`), not a
  copy of the body core in lamp-sim: one body, no second copy to drift. Making `lamp-body`'s runtime
  portable (so macOS runs it) is a later change to ten's crate, planned with ten.
- First step: Linux only, the `lamp-sim` skeleton, the sockets and the twin. macOS after.
- The test is the conformance script (the SDK's tour steps against the fake, the sim and a real lamp).
- The design is yes; open the first PR. Questions 3 to 10 were not answered one by one: the
  answers assumed in the code are marked in section 8.

## 1. Principle
The SDK, lamp-mcp and the SDK examples/tests run UNCHANGED, no "sim mode": the virtual
lamp answers the same protocol (control.schema.json), events, refusals, limits and
timings. Everything the developer can observe on the wire must come from the same code
as on a real lamp, so: run the real lampOS parts on the desktop wherever they build
(lamp-control, msgs, bus, ring logic, body logic), and replace ONLY the hardware edge.

## 2. What runs where 
The seam is already there: `lamp-control` (the real protocol server, WebSocket+TLS :7731,
pairing, grants, leases, rates, refusals) touches the rest of the lamp ONLY through Unix
datagram sockets and the bus directory, never hardware. Its flags: `--mood-socket` (body
gesture/move), `--lease-socket` (body lease, look), `--say-socket` (voice say/sound/stop/
turn/listen), `--actions-socket` (volume), `--bus-root` (events in, `light/request` out),
`--frames-socket` + `--sight-file` (camera), `--manage-socket` (skills).
- Real, unchanged, on the host: `lamp-control`, `lamp-bus`, `lamp-msgs`, `lamp-ipc`
  (Linux+macOS; the bus is Unix datagrams + hard links, so NOT native Windows: WSL/container
  or a later bus port).
- Real body logic: `lamp-body` (limits, caps, gestures, lease, `body/joint_states`, refusals).
  Its core `MotorControl<P: BusPort, C: Clock>` is generic over a 3-method `BusPort`; its
  supervised runtime (runtime/native, linux/motion.rs, worker) is Linux-only. Two ways:
  (b) Linux v1: a simulated STS3215 servo (packets in docs/hardware-contract.md) on a pty,
  the real `lamp-body` binary pointed at it with `--device`: zero body changes;
  (a) portable: our own `BusPort` + clock behind the core, which needs the supervisor/
  channels made non-Linux. Recommend (b) first, (a) if macOS needs it (open question 1).
- One new process, `lamp-sim` (ours), is the hardware edge and owns the rest:
  servo simulator (above); light: reads `light/request`, applies the ring's real ceiling and
  quiet-hours policy (reuse `lamp-ring` encoding/limits), keeps 32 pixels; voice stand-in on
  the say socket (answers say/sound/stop with the same `done` outcomes, speaks via host TTS
  or plays the clip, publishes `voice/state`, `speaker/volume`); `camera/image` + frames
  from the host webcam; `mic1/audio` from the host mic; fakes for presence/hand topics
  (`humans/faces/tracked`, `humans/hands/tracked`) from the twin's buttons in v1.
  The 3D view needs no feed of its own: it is the JS SDK's twin, a program paired like any other (section 3). Real eyes/ears (face, hand, sound-class models
  in `models/`) can run on the host feeds later; v1 does not need them for the protocol.
- Real `lamp-voice` (Gemini, turn logic) is NOT in v1: 280k lines, needs keys and the audio
  stack; the virtual lamp acts as a lamp with a developer's program driving it, `say` goes
  to a TTS stand-in. Later: run the real voice against host audio.

## 3. The 3D view
Spike result (10 Nov 16:10, throwaway): lamp.glb loads in three.js (GLTFLoader) and the five
bones move from a joint list with the rule below; headless Chrome renders it (the whole lamp,
base to head, fits in a 0.45 m box). Nothing new needs modelling.
- Model: `robots/lamp/hardware/cad/glb/lamp.glb` (22 MB; one skinned armature, bones base_yaw,
  base_pitch, elbow_pitch, wrist_pitch, wrist_roll; static 0_base). The viewer loads it from
  the repo file, it is not copied or committed twice.
- Kinematics: `lampOS/docs/lamp.model.json` (bone, axis, sign, zero_deg per joint, already in
  the repo with copies in both SDKs). Rule: bone.quaternion = rest_quat * axisAngle(axis,
  sign*(deg - zero_deg)). The viewer does NO limiting, easing or clamping: the numbers come
  from the body, so ranges, speed, acceleration and refusals are the body's.
- Reuse, don't rebuild: `lampOS/sdk/js/examples/twin/` (eighth's #4564) already is
  this viewer: a three.js page that loads lamp.glb, applies the rule above from the SDK's
  `body.state` event (12.5/s, eased in the page), shows the light's colour as a glow, has joint
  sliders that send `joints` under a body lease, a "Calibrate" panel that sets each joint's zero
  and sign against a real lamp (for lamp.model.json), and a token + origin guard on localhost.
  It talks to the lamp through the SDK, so the virtual lamp needs NO view feed of its own:
  `node examples/twin/server.js` against lamp-sim is the 3D view, and it doubles as an SDK
  conformance test. Our view work is additions, upstream into the twin (eighth owns it, ask first):
  (1) the real 32 pixels (no state or event carries them today: `get light.shows` and `body.state`'s
  light are only {on, colour, level}; v1 shows one colour like the twin does; for 32 pixels we propose
  to the first builder an additive `shows.pixels` (hex x32, from `light/drawn`) in `get light` and
  `body.state`, question 10, not a back door), (2) private/speaking/
  listening indicators from events the SDK already has, (3) touch/face/hand inputs back to the
  lamp (later), (4) a desktop window around it, (5) three.js vendored (it imports from unpkg
  today) so the app works offline.
  Today lamp-control takes `joints` for yaw and pitch only (roll/shoulder/elbow refused until the
  body's goal stream): the sim inherits whatever the real lamp-control does, which is the point.
- Joint degrees: whatever `body.state` carries (the body's own, through its calibration);
  per-lamp ranges come from the lamp config (question 7), never from the view.
- Light ring: the twin's glow becomes 32 emissive points on the head's opening (the glb has no
  ring mesh and generic mesh names, so the circle is found by eye once and stored: centre,
  normal, radius in the head bone's frame), coloured by the ring's 32 pixels.
- Window: v1 a browser tab (`lamp-sim` starts the twin server and opens the browser). Later a small
  desktop shell (Tauri, or a Rust webview) with the same page; native window = packaging only.
- Tech: the twin's own (plain JS ES modules + three.js); JS is already in the repo.
- Risks found: (1) lamp.model.json says zero and sign are CAD guesses "until measured on a lamp":
  the sign of shoulder/elbow/pitch must be checked against a real lamp's pose (a [hand-off] with
  eyes.sh on lamp-75c9: command a pose, compare its photo with the render); until then the
  view could fold the wrong way for a joint. (2) How far the model moves depends on the body
  config copied for the virtual lamp (question 7): the dev lamps now give apps about 90% of their
  range, a smaller range moves the model little, and the view offers an exaggeration slider
  (display only) so a small move can be seen.

## 4. Host camera / mic / speaker 
All three plug in at the bus, so the real consumers (lamp-control's `camera.snapshot/
stream`, `microphone.stream`, privacy rules) run on them unchanged.
- Camera: `lamp-sim` captures the host webcam (v1: Linux V4L2 via the existing `lamp-camera`
  Linux backend, macOS AVFoundation, through one small capture helper; or ffmpeg as a child
  process, which is one code path for all OSes) and does what eyes does at the edge: writes
  the newest JPEG to the file `camera/image` points at, publishes `camera/image` (2-4/s) and
  feeds `--frames-socket`. lamp-control already refuses it while private. Pictures with
  people: local only, never committed. The camera is off unless `--camera=always` or `--camera`
  is given. `--camera=always` keeps it open for the lamp's whole life, as the eyes' is (the host's
  camera light stays on). `--camera` opens it only while a computer's camera mark is on the ring
  (closed about 10 to 11.5 s after the last: a mark counts for 1.5 s after it was drawn, then the
  camera's own 10 s hold), so the host's camera light means someone is looking. That needed one
  change in lamp-control's own `camera.rs`: it read the sight file before it asked the ring for the
  mark, so a closed camera could never be asked to open. The change is
  `--camera-opens-on-demand` (#6095), off by default so real lamps behave as before; lamp-sim
  passes it with `--camera`. Each picture still goes to a computer only while the ring shows the
  camera's mark.
- Mic: capture host mic, resample to 16 kHz mono S16, 20 ms blocks (320 samples), publish
  `mic1/audio` as the ears do (`json+pcm`, 50/s). `microphone.stream` and `hear` then work.
  Needs the OS mic permission on macOS (one prompt); say so in the README.
- Speaker: `say` text -> host TTS (`say`/`espeak`/SAPI, or a TTS service later) and `sound`
  clips -> host audio out (cpal, already a workspace dep), with the voice's state and done
  events and the lamp's volume (`speaker/volume`) applied. Echo of the lamp's own voice
  into the host mic is the user's to manage in v1 (headphones), an AEC later.
- Privacy/mute: the viewer has a switch (private / camera off / mic off) that publishes the
  same privacy lease the voice does, so the grants and refusals match the real lamp.
- Packaging: capture via ffmpeg needs it installed; native helpers avoid that. Decide with
  the first builder (question 5).

## 5. Packaging
v1: Linux only: `lamp-sim run DIR` starts everything from the lampOS binaries built for the host
(`crates/sim/README.md`). macOS after `lamp-body`'s runtime is portable; Windows and a desktop
shell (single installer) later.

## 6. v1 vs later
v1: pair + all body/light/gesture ops with real limits, viewer with 5 joints and ring,
host camera, microphone and speaker, SDK tests green. The microphone is just `mic1/audio` blocks
on the bus (section 4); on macOS the first use asks the OS for microphone (and camera) permission,
which the README says. Later: touch via click, sleep/mute states, Windows, installer.

## 7. Test plan
The rule: the virtual lamp is a lamp to everything outside it. No test gets a sim flag, no SDK
or lamp-mcp code is changed to suit it, and a test that fails on the sim but not on a real lamp
is a sim bug (or a recorded, listed difference).
1. SDK suites against it: the Python and JS SDK test suites and lamp-mcp's, run with the lamp
   address pointed at `lamp-sim` (pair with its code, then the ordinary tests). Where a suite
   today uses the SDK's own fake lamp (`lampos.testing`), it stays; a second run uses the sim.
   Tests that need what v1 lacks (real voice turns, real faces) are skipped by an explicit,
   listed marker, not by a sim switch.
2. Every SDK example (examples/ in both SDKs and the docs' snippets) runs unchanged against
   it, exit code 0 and the expected output, in a script `lamp-sim-examples`.
3. Conformance against a real lamp: one op script (`get` state/body, gesture, move, joints,
   look, light, say, sound, camera.snapshot, refusals: bad_request, busy, not_granted, private,
   rate limits, lease taken) run on the virtual lamp and on lamp-75c9 (through a [hand-off],
   under the lock); compared: reply fields, refusal codes, event order, and timings (a joint
   move of N degrees takes within 10% of the real time; a `light` fade, a `say` done). The
   expected values come from the real lamp's run, stored as fixtures.
4. Kinematics: render the lamp at 6 poses and compare the viewer's bone angles to `get body`
   degrees (exact), and its picture to a lamp-75c9 photo of the same pose (by eye, once, for
   the sign check in section 3).
5. Viewer smoke (headless Chrome): loads the glb, five bones move on a feed, ring shows 32
   distinct pixels, no console errors; screenshot kept as a CI artifact (never a camera frame).
6. CI: a job on Linux and macOS: build lamp-sim and lamp-control for the host, start it, run 1,
   2 and 5; conformance (3) is run by hand on the lamp for each release of the sim.
7. Host devices: a webcam/mic/speaker loop test on a developer machine (a manual checklist) and,
   in CI, fake devices (a test pattern as the camera, a tone as the mic, a null speaker).

## 8. Open questions
1. (Answered: make `lamp-body`'s runtime portable, later, with ten; Linux first.) Body on macOS/Windows: the real `lamp-body` runtime is Linux-only (its core is generic over
   a `BusPort` trait, so a seam exists, but the supervisor and channels are cfg(linux)). v1:
   Linux runs the real binary on a simulated servo bus (a pty); macOS needs either that runtime
   made portable (a change to lamp-body, which is not ours) or the body core in `lamp-sim` on
   our own `BusPort`. Which do you want?
2. (Answered: yes.) May we add a simulated servo bus as a small crate (no change to lamp-body for the Linux path)?
3. The view is the existing JS twin (sdk/js/examples/twin, eighth's #4564); our additions go to it, upstream. OK, and is a new JS tool folder for the launcher/vendored three.js acceptable?
4. Where does it live: lampOS/crates/sim + lampOS/tools/sim-viewer?
5. Camera/mic capture: ffmpeg child process (one path for Linux/macOS/Windows, needs ffmpeg
   installed) or native (V4L2 / AVFoundation / MediaFoundation, more code)? v1 suggestion: ffmpeg.
6. Is the real `lamp-voice` out of v1 (say via host TTS only)? Our reading: yes.
7. Lamp identity: the virtual lamp needs its own name/unit id, certificate and pairing code
   (lamp-control makes the certificate on first start), plus a body config copied from one real
   lamp (`get body`: ranges, home, folded, speeds). Which lamp's? 75c9's measured one?
8. Discovery (mDNS `lamp-xxxx.local`) on the host: advertise it, so `Lamp.discover` finds it?
9. Does the 'sim' name/unit conflict with `lamp-voice-eval`'s fake runtime or `selftest`?
   (no: different purpose, but say so in the docs.)
10. Pixels back: may `get light` and `body.state` carry `shows.pixels` (32 x `#rrggbb`, from
    `light/drawn`), additive, so an SDK program such as the twin can draw the ring as the lamp does?
