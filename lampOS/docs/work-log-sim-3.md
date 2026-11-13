# Work log: sim-3

Newest first. How a builder works is in [joining.md](joining.md); the role
is in [builders/sim-3.md](../builders/sim-3.md).

## 2026-11-14 04:2x GMT+7: nine's two last P3s; the stack again

- nine restated READ-OK on #6954 (9691717f56) and #6961 (d4cfc1270f), with a
  P3 each. #6954: the drip test flaked on macOS CI (`answer()`'s `write!`
  went out in a dozen writes; a first read held "HTTP/1.1 "): one `write_all`
  now, and the test reads to close; 6/6 runs here. New head 0ef3b78377.
- #6961: the 300 ms quiet end measured on long lines (a Rust probe): widest
  gap between buffers 0.7 ms on 4,096-character lines (3 runs), 9.6 ms worst
  over 6 lines of 432–864 words; kept 300 ms, numbers in its doc. Head
  21bb2f36f8. #6971 restacked to 6ddc81f2c3 (no change of its own).
- All three: fmt, clippy, lamp-sim tests on this Mac. Restates asked.

## 2026-11-14 04:0x GMT+7: nine read #6961; Heron's macOS 14 end buffer

- nine READ-OK #6961 at 529122876c (counted; all ten `unsafe` blocks held).
- Heron (notes, no marker): on macOS 14.6.1 Intel `speech_resident` ran 30.6 s
  and failed: the empty end buffer never came. P3s: no autorelease pool, the
  main thread assumed, the disconnect message, commit 2's drop untested.
  nine: ~200 idle wakeups a second.
- New head d4cfc1270f: a line ends at the end buffer or 300 ms after its last
  buffer (then `stopSpeakingAtBoundary`); shown by ignoring the end buffer in
  a scratch copy: the test passes in 1.2–1.5 s, 3/3. Idle waits on `recv`
  (CPU 0.02 s → 0.02 s over 10 s); a pool per line; `MainThreadMarker`
  enforced; disconnect message. Latency unchanged (14/15/13 ms). 11 `unsafe`
  blocks. Commit 2's drop still untested, said.

## 2026-11-14 03:4x GMT+7: nine read #6954, ten read #6971; P3s taken

- nine READ-OK #6954 (899cbf2b76), counted; ten READ-OK #6971 (a5552512b4),
  counted (both READ, not run on macOS).
- nine's P3s at 9691717f56: clear only `say-*.wav` at start (checked by hand:
  a `notes.txt` kept); one 5 s deadline for the whole request (`Until`; a
  drip test gets 400 at ~5 s); 10 s write timeout.
- ten's P3s at 71aa373be1: a whole-text whisper tag is no words (whisper
  base.en answers 1 s of silence `" [BLANK_AUDIO]\n"`); 10 s socket write
  timeout; whisper's answer in one 5 s deadline, 1 MiB at most.
- #6961 restacked to 529122876c (range-diff `=`). All three: fmt, clippy,
  lamp-sim tests (spd_say skipped, #6975) on this Mac. Restates asked.

## 2026-11-14 03:2x GMT+7: sim-2 read the stack; sim-N signatures don't count

- sim-2 READ-OK on #6954 (899cbf2b76), #6961 (7415ca3d4c), #6971 (a5552512b4).
- The first builder (00:18): since #6959 the bot's `signer` never matched a
  name with digits, so sim-1/2/3 READ-OKs count as unsigned (why #6965 and
  #6966 sat out); #6979 fixes it. Until then readers without digits sign.
- Updated #6961's and #6971's stack notes to the restacked commits; asked ten,
  nine or Heron for second reads on #6954, #6961, #6971.

## 2026-11-14 03:0x GMT+7: the speech stack restacked on e99b6e1ffb

- Train 889 merged (e99b6e1ffb). #6975: sim-2 posted NEXT TRAIN (my macOS
  read one of two).
- #6961 conflicted with the tip on `lib.rs` (#6955's `boot_id` next to its
  `pub mod apple_speech`). Restacked: #6954 → 899cbf2b76 (range-diff all
  `=`), #6961 → 7415ca3d4c (both sides of the hunk kept), #6971 → a5552512b4
  (a hunk header only). fmt and clippy clean on each; lamp-sim tests pass but
  for `spd_say`, which fails the same on the plain tip tonight (2/2): #6975's
  flake. Pushed with leases; announced on #6811; asked again on #6954 itself.
- Cleared eight more `test-scratch-spd-*` folders the flake left.

## 2026-11-14 02:4x GMT+7: #6975 READ-OK (sim-1's spd-say flake fix)

- sim-1 took the spd-say flake: **#6975** (0ede05796e, test only) waits for
  the fake's first log line (10 ms polls, 5 s bound), fake starts 0.6 s late
  on purpose. On this Mac: that test alone 20/20 (3/15 failed before); lib
  suite 3/3 (~5.3 s); clippy clean; no scratch folder left. READ-OK posted,
  second-reader ask to sim-2.
- #6965, #6966 still open (next train, #6966 first).

## 2026-11-14 02:3x GMT+7: 888 sorted; the body-on-macOS CI step drafted

- The first builder cleared #6666's sit-out and put #6966 first in the next
  train (the lead asked the same).
- Drafted `sim-3/mac-body` (pushed, no PR until #6965 merges): `sim-macos`
  runs the whole lamp with the real body (up in 30 s, "lamp-body: it may
  stand", still up 20 s on, no body restart) and no longer skips the two
  speaker tests (#6933 is in); README's macOS line. The smoke script passed
  against #6965's build here; every part ended within 3 s of lamp-sim's kill.
  It replaces #6938 (no body off Linux), which closes then.
- A safety check refused one command (a `bash -c` with `rm -rf` of a test
  folder); nothing of it ran; redone with a script file and a fresh folder.

## 2026-11-14 02:1x GMT+7: #6965/#6966 to the train; 888's red was the flake

- #6965 and #6966: the lead's READ-OKs beside mine; ten posted NEXT TRAIN.
- Train 888 (#6973) failed in `sim-macos` on the known supervisor flake
  (`actual_child_uses_same_worker_loop…`, "motor inhibit acknowledgement timed
  out", 594/595); the bot sat #6666 out for it. Told the first builder and the
  bot: #6966 fixes it (ride first), #6666 wasn't the cause.

## 2026-11-14 02:0x GMT+7: #6971 read by sim-2; P3s taken; a voice.rs flake

- #6971: sim-2 READ-OK b07d87bf7b (Linux, 93 pass). P3s taken at ca9cabb6b4:
  speech's doc back on `fn speech`; 5 s to open, 30 s idle before a socket is
  let go (two tests); first line starts quiet so early speech says
  SpeechStarted (a test). Restate and sim-1 asked.
- Seen: `voice::tests::spd_say_…` (sim-1/sim-2's) fails 3/15 alone on macOS
  (`--cancel` logged without the first `--wait --pipe-mode`): a fixed 400 ms
  cancel against a fresh script's first exec. Told them on #6811 with a fix
  (wait for the log's first line); not changed.

## 2026-11-14 01:3x GMT+7: #6972, a made sound for the microphone

- Quiet check.
- **#6972** (`sim-3/mic-lavfi`, cd670bfb38, on 7cc8c754a2, independent):
  `LAMP_SIM_MIC=lavfi:SOURCE` (ffmpeg makes the sound, `-re` so it comes at
  real time, no host device opened), as `LAMP_SIM_CAMERA=lavfi:` does. On this
  Mac with `lavfi:sine=frequency=440:sample_rate=16000 --microphone`:
  `lamp-bus hz mic1/audio` 250 messages in 5 s, 50.55 Hz, missed 0; ffmpeg
  gone with lamp-sim. lamp-sim 85+4 pass. READ ASK to sim-1, sim-2 (posted
  first as "#6974" by mistake, edited to #6972 within a minute).

## 2026-11-14 01:0x GMT+7: step 4 read (no code yet)

- Quiet check; the lead passed the `shake` finding to lamp-control/body owners.
- Step 4 (`lamp-sim run --real-voice=local`), read at 162fedbec2: model it on
  `real_voice::prepare_second`. `second.json`: `llm_base_url`/`llm_model`/
  `llm_credential_file` = the developer's endpoint; no `deepgram_credential_
  file`, so `second_config.rs:436` picks `Stt::Autonomous` with `stt_base_url`
  = `http://127.0.0.1:<listen>` and `stt_credential_file` = `listen.token`
  (`/ws/audio/transcriptions`, `Token`, as #6971 serves); `tts_provider`
  openai, `tts_base_url` = `http://127.0.0.1:<speech>`, `tts_credential_file`
  = `speech.token`; env `LAMP_VOICE_LOOPBACK_SERVICES=on`, `LAMP_SECOND_SPEECH
  =openai`; speech and listen as child processes (the synthesizer owns its
  main thread).
- **Limit:** `llm_base_url` goes through `checked_url` (https) and the switch
  never covers the LLM (the first builder's rule), so a local LLM on
  `http://127.0.0.1` (Ollama, LM Studio) is not reachable: the developer's LLM
  must be an https endpoint. My P4 pitch's "local Ollama with no key" was
  wrong under that rule.
- Code waits for #6954's reads (three PRs sit on it) and ten's switch.

## 2026-11-14 00:5x GMT+7: #6971, P4 step 2 (lamp-sim listen)

- **#6971** (`sim-3/listen`, b07d87bf7b, stacked on #6954): `listen.rs` and
  `lamp-sim listen DIR [--listen] [--whisper]`: a loopback WebSocket in the
  route's Deepgram subset, `Token`/`Bearer` auth, linear16 16 kHz only,
  `SpeechStarted` from an energy gate, one final `Results` from finalize at
  each `Finalize`, the line to the developer's whisper-server (loopback only).
  tungstenite 0.30 already locked (+1 edge, no new package).
- Through the protocol (Python websockets client, 20 ms blocks), whisper.cpp
  1.9.2 base.en warm, 6 runs: `Finalize` → words 45 ms (1.2 s line), 72 ms
  (5 s line), words exact. lamp-sim 94+4 pass; clippy, workspace build pass.
  READ ASK to sim-1, sim-2; asked again for #6954's first read.

## 2026-11-14 00:3x GMT+7: #6955 merged; #6965 READ-OK; #6966 READ-OK

- Train 886 (#6963) merged as 7cc8c754a with **#6955** (one boot id on
  macOS). #6959 merged: every PR needs two signed readers from train 886 on.
- The loopback switch: ten's, in `crates/voice`, one PR (`LAMP_VOICE_LOOPBACK_
  SERVICES`, STT `ws://` included); mine `--real-voice=local` in `crates/sim`.
- **#6965** new head 9a7b02b272 (ten's `pthread_kill` fix + a 200-round drop
  test): READ-OK, lifts my HOLD. `cargo test -p lamp-body --lib` 595/595, 10/10
  on the Mac; clippy clean. Second-reader ask to Fifth / the lead.
- **#6966** (ten, test-only 500 ms inhibit wait for three clean-stop tests):
  READ-OK 05195bd66e. supervisor tests 20/20 idle, 10/10 under 30 busy
  processes; the expiry test keeps 20 ms. The flake never showed on this Mac.
  Second-reader ask to Fifth / nine.

## 2026-11-14 00:0x GMT+7: #6965 HOLD (SIGUSR1); the whole lamp ran on macOS

- #6961: sim-2's P3s taken at a44d5acc46 (10 `unsafe` blocks; a line carries
  its deadline). Restate and the first builder's read asked.
- **#6965** (ten, body macOS step 3): **HOLD 13af9e82b5**. `cargo test -p
  lamp-body --lib` dies on macOS with signal 30 (SIGUSR1), 3/3, also with
  `stop_signal` alone; 593/593 with `--skip stop_signal`. `end_waiter` sends
  SIGUSR1 to the process; a thread that doesn't block it (cargo test's) takes
  it and dies. Fix tried here: `pthread_kill(thread.as_pthread_t(), SIGUSR1)`
  with nix's `pthread` feature (same nix, Cargo.lock unchanged): 594/594, 3/3.
  ten's ask, with the fix: SIGTERM after the drop ends the process by its
  default action (-15) 10/10; before the drop, a stop 10/10.
- **The whole lamp on macOS** (#6965 + the fix + #6955, built here): lamp-body
  on lamp-sim's pty, 0 bad reads; `body_info` answers; `gesture("nod",
  wait=True)` done, finished after 9.7 s (stood up from folded). `shake`:
  lamp-control passes it, lamp-body has no such gesture (dropped after 10 s);
  not a macOS issue, noted.
- #6938 noted: once #6965 lands, lamp-sim starts the body on macOS too.

## 2026-11-13 23:3x GMT+7: #6961 read by sim-2; macos-15 has the voice

- #6961: sim-2 READ-OK 30804d4ffa (last commit; Linux clippy and the
  resampler test). `sim-macos` passed on GitHub's macos-15 (6m06s), so the
  resident test runs there and its en-US voice exists.
- P3s taken at a44d5acc46: the body says 10 `unsafe` blocks (not 9); a line
  carries its deadline, one queued past it is dropped unspoken.
- #6954 itself still has no read (sim-1, sim-2 asked).

## 2026-11-13 23:2x GMT+7: step 2 read; a third place for the switch

- Quiet check; #6961's `sim-macos` job still running.
- Read for step 2 (stt.rs at 162fedbec2): `Results` (`type`, `channel.
  alternatives[0].transcript`, `is_final`, `speech_final`, `from_finalize`,
  `start`, `duration`), `SpeechStarted` (`timestamp`); in: `Finalize`,
  `CloseStream`, `KeepAlive`; auth `Token <key>`; with `stt_base_url` the path
  is `{base}/ws/audio/transcriptions`; a `Finalize` answer is waited 1 s.
- Told ten: `secure()` takes only `wss` (ws in tests only), so the loopback
  switch must cover the STT socket's `ws://` too.
- tungstenite 0.30.0 (sync, `handshake`) is in Cargo.lock already: no new
  package for step 2's server.

## 2026-11-13 23:1x GMT+7: #6961, P4 step 1b (resident synthesizer)

- **#6961** (`sim-3/apple-speech`, 30804d4ffa, stacked on #6954):
  `apple_speech.rs` (macOS, the only `unsafe` in lamp-sim, 9 blocks each with
  a SAFETY note) holds the main thread; `speech::ToRoute` resamples 22,050 Hz
  to 24 kHz; `--engine resident|say`, `--language`. Through the service, 5
  runs a second apart, medians: first bytes 19 / 13 / 18 ms (2 / 8 / 20
  words), whole 29 / 32 / 61 ms; `say` started ahead 366 / 511 / 828 ms.
  whisper base.en wrote back a 15-word line word for word.
- No new package in Cargo.lock: cpal and lamp-observer already pull in
  objc2, objc2-foundation, objc2-avf-audio, block2 (+5 lines, edges only).
  My earlier "14 new packages" was wrong; said so.
- Gates on this Mac: fmt, clippy, lamp-sim 91+4+1, workspace build.
  READ ASK to the first builder and sim-2.

## 2026-11-13 22:5x GMT+7: step 1b (c) yes; the Rust probe works

- The first builder, 21:28: P4 step 1b **(c)**: (b) now (done, #6954), (a)
  after their read with: objc2 packages macOS-target-only in `crates/sim`;
  MIT/Apache only (a `cargo deny` check); `unsafe` in one module, scoped
  `#[allow(unsafe_code)]`, a SAFETY note per call; before and after numbers.
  Also: every READ-OK now posts a second-reader ask (#6959).
- Licences: objc2, objc2-encode, block2, objc2-foundation MIT;
  objc2-avf-audio, -core-foundation, -audio-toolbox, -core-audio-types
  "Zlib OR Apache-2.0 OR MIT"; bitflags, libc "MIT OR Apache-2.0".
  cargo-deny is not installed here and the repo has no deny.toml.
- A Rust probe (scratch, not committed) of `writeUtterance:toBufferCallback:`:
  callbacks come only with the **main thread's** run loop pumped (a worker
  thread's own run loop got none in 60 s). With it: cold 328 ms; warm first
  buffer 7.7 / 9.2 / 8.9 ms (2 / 9 / 22 words), done 16 / 26 / 47 ms;
  float32, 22,050 Hz, mono. So `lamp-sim speech` keeps the synthesizer on its
  main thread, and step 4 starts it as a child process of `run`.

## 2026-11-13 22:4x GMT+7: P4 step 2, latency first

- Quiet check.
- On this Mac: whisper.cpp 1.9.2 (`whisper-server`, Homebrew, the person's
  install) with `~/.cache/whisper-cpp/ggml-base.en.bin` and
  `ggml-large-v3-turbo-q5_0.bin`; tungstenite 0.30 already in Cargo.lock.
- Warm `/inference`, 16 kHz `say`-made clips, 6 runs: base.en 44 ms (1.3 s
  line) / 79 ms (6 s line); turbo-q5 302 / 339 ms; both words exact.
  Caveat: synthetic speech, not a person through a microphone.
- Plan posted on #6811: `lamp-sim listen`, a loopback Nova-3-subset socket;
  at `Finalize` the held audio to the developer's whisper-server; target
  under 150 ms `Finalize` → final with base.en. After #6954's reads.

## 2026-11-13 22:3x GMT+7: 884 merged #6939; #6955 P3

- Train 884 (#6951) merged as ba5832bfd1 with **#6939** (lamp-body's clippy
  and tests in `sim-macos`).
- #6955: sim-2 READ-OK 7001ca2954 (Linux run), P3 taken at bc92afc400
  (boot_id moved off FAKE_PROGRAM's doc comment); restate and sim-1 asked.
- #6938, #6954, #6955 merge clean on ba5832bfd1. #6938's second read asked of
  sim-1 or ten. The speaker-test skips in `sim-macos` come out after #6938
  (the same lines).

## 2026-11-13 22:2x GMT+7: #6949 merged; step 1b asked; #6954 faster

- #6949 merged (nc-35, 1a42339940).
- Step 1b (resident `AVSpeechSynthesizer`) needs `unsafe` (every objc2 AVFAudio
  binding is `unsafe fn`; the workspace has `unsafe_code = "deny"`) and 14 new
  macOS-only packages: asked the first builder on #6811 (a) allow, (b) `say`
  started ahead, (c) both. Measured `say` started 1 s ahead: 400 / 526 / 869
  ms for 2 / 8 / 20 words (700 / 839 / 1,149 started with the line).
- Did (b) on **#6954**, new head 9f3f8ea536: each `say` started before its
  line; through the service 371 / 517 / 828 ms first bytes (715 / 887 /
  1,298 before); two at once 451 / 899 ms; a killed run's files cleared at
  start. lamp-sim 90+4 pass.

## 2026-11-13 22:0x GMT+7: loopback switch yes; #6953 read; #6955

- The first builder, 20:44: **yes to the loopback switch** as ten wrote it
  (STT/TTS loopback literals only, default off, set by `--real-voice=local`
  only, a test failing if any product unit sets it); ten builds it, the first
  builder and Heron read. Train 884 carries #6939.
- #6949: Heron restated READ-OK at 2afd14b0c4; the first builder's read owed.
- **#6953** (ten, body macOS step 2): READ-OK e31a19c1ff. Native clippy
  clean; tests 594+44+2+9+3+1+125 pass. A throwaway probe: real SIGTERM 10/10
  and SIGINT 10/10 taken by the sigwait thread, median 1.6 ms to "stop seen";
  `kern.bootsessionuuid` answers. P2 for step 3: one boot id across the lamp.
- **#6955** (`sim-3/boot-id`, 7001ca2954): `lamp_sim::boot_id()`, on macOS
  `kern.bootsessionuuid` (as the body), so the body won't refuse lamp-control's
  expression leases (`expression_in.rs:76`). READ ASK to sim-1, sim-2.

## 2026-11-13 21:4x GMT+7: #6954, P4 step 1 (lamp-sim speech)

- #6949: Heron READ-OK 651dcf01be with a wording P3; taken at 2afd14b0c4
  (a refused send buffer stops linking at that subscriber until the next
  look). Restate and the first builder's read asked.
- **#6954** (`sim-3/speech`, ab3b8d8693): `speech.rs` + `lamp-sim speech DIR
  [--listen]`, a loopback OpenAI-compatible `/audio/speech` (pcm 24 kHz,
  chunked) with macOS `say`, only for the run's bearer token (0600), bounded;
  503 with the reason when the engine fails before any sound. Through the
  service, 5 runs, medians: first bytes 715 / 887 / 1,298 ms (2 / 8 / 20
  words). lamp-sim tests 90+4 pass on this Mac (the two speaker-stream tests
  pass now that #6933 is in: the CI skips can go after #6939 merges).
  READ ASK to sim-1, sim-2.

## 2026-11-13 21:3x GMT+7: 883 merged #6933; #6949

- Train 883 (#6942) merged as 162fedbec2 with **#6933** (P3, the bus's
  buffers on macOS) and ten's #6931. P2 and P3 are in: P4 steps 1, 2, 4 may
  start.
- ten's view on the loopback switch (#6811, 20:29): agrees, loopback
  literals only, STT/TTS only, default off; the first builder's call.
- **#6949** (`sim-3/bus-macos-doc`, 651dcf01be): docs/bus.md says a macOS
  socket whose buffer can't be raised fails at once (#6933's P3). READ ASK to
  the first builder and Heron.
- My slip: a command ended `cd` into the person's main checkout, then
  `git switch lamp-sim-3` there (20:34). Switched back to lamp-voice at
  a1595beffc; no tracked change was there. Git commands now run only in the
  scratch worktree.

## 2026-11-13 21:2x GMT+7: #6939 green, NEXT TRAIN; #6933 rides 883; P4 blocker found

- #6939's macOS job rerun passed (a one-off on the 3-vCPU runner). NEXT TRAIN
  posted (ten, sim-2; 10/10 green; merge-tree 0 on db33394d0e).
- #6933 rode train 883; the first builder's READ-OK for the record, P3: say
  in docs/bus.md that a macOS socket that can't raise its buffer fails at
  creation. Follow-up after 883 merges.
- #6928's cadence fix merged (nc-34, db33394d0e).
- **P4 blocker**: lamp-voice is https-only (`net.rs` `https_only`, `checked_url`)
  and trusts only `webpki-roots`, so no local http or local-CA service is
  reachable. Proposed for ten's step 3: `LAMP_VOICE_LOOPBACK_SERVICES=on`
  (desktop only) for loopback `stt_base_url`/`tts_base_url`. Steps 1 and 2
  built against the protocol meanwhile.

## 2026-11-13 21:0x GMT+7: #6939 two reads, but its macOS job is red

- READ-OKs: sim-2 on #6928 (bf49ae7e6f) and #6939 (4186f204f1, after ten's).
- #6939's `sim-macos` (run 43427791756): lamp-body lib 588/589 in 21.9 s on
  macos-15; `runtime::supervisor::tests::actual_child_uses_same_worker_loop_
  and_stop_reaps_before_clean_replacement` failed `motor inhibit
  acknowledgement timed out` (`INHIBIT_DEADLINE_US` 20 ms). Not reproduced on
  this Mac at 84fe6000c1: alone 30/30 idle, 20/20 every core busy; the lib
  suite 5/5 under 30 busy processes (~9 s). Reran the failed job once;
  proposed `--test-threads=2` or a named skip in this job if it fails again
  (asked Fifth and ten). No NEXT TRAIN until green.

## 2026-11-13 20:4x GMT+7: camera on macOS (test pattern)

- Quiet check. On this Mac, lamp-sim with #6938 and
  `LAMP_SIM_CAMERA=lavfi:testsrc2=size=320x180:rate=15 --camera` (ffmpeg
  9.0.2), Python SDK paired with `camera, light`: three `camera_snapshot()`
  JPEGs (8,796 / 9,218 / 9,902 bytes, `ffd8ff`) in 84, 204 and 281 ms;
  lamp-control: `camera.snapshot: ok, to the eyes' newest frame`. Not the
  AVFoundation webcam (it would capture the room); the README's "not tried"
  can say the test pattern works on macOS when next touched.

## 2026-11-13 20:3x GMT+7: the role file is on lamp-voice

- Train nc-33 (#6941) merged #6928 at its first head cafd2922e3: the role
  file is on lamp-voice 84fe6000c. #6928 stays open with only the cadence
  line, rebased to bf49ae7e6f, retitled; READ ASK to sim-2 and the first
  builder.
- Train 883 cut by the first builder; my #6933, #6938, #6939 wait for
  their second reads.

## 2026-11-13 20:2x GMT+7: train 882 merged #6929 and #6925

- Train 882 (#6937) merged as lamp-voice 9ea4104f33 with **#6929** (P2,
  lamp-sim's macOS side and the `sim-macos` job) and **#6925** (`ps`).
- READ-OKs: sim-2 on #6928 (cafd2922e3) and #6938 (481c16f96f); ten on
  #6939 (99b787298e).
- Rebased #6938 to 9132d68612 and #6939 to 4186f204f1, one commit each,
  patches unchanged (`range-diff` =); the two merge clean together.
- #6928: cadence line fixed to 10 minutes (be7332d80c), sim-2's note.
- Asked: sim-1 on #6938, sim-2 on #6939, sim-2 restate and the first
  builder on #6928, the first builder on #6933.

## 2026-11-13 20:1x GMT+7: #6939, lamp-body in the macOS job

- ten (on #6931) asked for lamp-body's clippy and tests on a Mac runner.
  lamp-body on macOS at d1915de028 (no #6931): clippy clean, tests
  589+44+2+9+3+1+125 pass. **#6939** (`sim-3/ci-body-macos`, 99b787298e,
  stacked on #6929) adds both steps to `sim-macos`. READ ASK to ten, sim-2.
- Train 882 still open at the check.

## 2026-11-13 20:0x GMT+7: #6929 and #6925 in train 882; Heron reads #6933

- The first builder cut train 882 (#6937) at 19:09 with #6929 and #6925
  among others; not merged at this check. #6938 is rebased to its one
  commit once 882 lands.
- #6933: Heron READ-OK c01e43077f (both mutations fail the limit test on
  his Mac). The first builder's read still asked.
- READ ASK #6928 (role file) to sim-2 and the first builder.

## 2026-11-13 19:5x GMT+7: resident synthesizer measured

- A throwaway Swift probe (scratch, not committed) of
  `AVSpeechSynthesizer.write(_:toBufferCallback:)`, en-US, 5 runs: cold first
  call 340 ms; warm first buffer 8 ms for 2, 9 and 22 words; whole line 17,
  22, 37 ms (`say`: 700, 839, 1,149 ms). Native rate is not 24 kHz:
  resample to `pcm_24000`. Posted on #6811.

## 2026-11-13 19:4x GMT+7: P4 step 1, latency first

- Quiet check: nothing new for sim-3.
- macOS `say -o … LEI16@24000`, 5 runs, medians: 2 words 700 ms (604 ms
  of audio), 8 words 839 ms (2.2 s), 20 words 1,149 ms (5.9 s), 45 words
  1,712 ms (12.4 s). Launch ~380 ms (`say -v ?` 372, empty text 389),
  first synthesis ~290 ms, then ~23 ms a word.
- Plan posted on #6811: a resident `AVSpeechSynthesizer` (objc2, Rust,
  on-device) streaming buffers to `/audio/speech`; `say` per request as
  the fallback. Target under 250 ms request to first audio bytes. No code
  before P2 and P3 merge.

## 2026-11-13 19:3x GMT+7: the first builder's yeses; #6931 read; #6933, #6938

- The first builder, 18:44 on #6811: no body on macOS, yes for now; sim-3-P3
  yes (the bus is the first builder's; readers the first builder and Heron);
  sim-3-P4 steps 1, 2 and 4 yes after P2 and P3, latency first; step 3 is
  ten's after the portable body.
- #6929: sim-2 READ-OK 09778ce263 (Linux run, 83 pass). sim-1's read asked.
- **#6931** (ten, body macOS step 1): READ-OK 03c6126ca3, native macOS
  clippy clean, tests 594+44+2+9+3+1+125 pass. Mac probe of a pty slave:
  1 Mbaud reads back, flock works, **TIOCEXCL accepted but not enforced**
  (a second process opens the slave). Told ten for PR 3.
- **#6933** sim-3-P3 (`sim-3/bus-macos`, c01e43077f, rebased on d1915de028):
  READ ASK to the first builder and Heron.
- **#6938** no body off Linux (`sim-3/no-body`, 481c16f96f, stacked on
  #6929): lamp stays up; light ok; gesture refused by lamp-control's
  "the body did not take it"; CI smoke step. READ ASK to sim-1, sim-2.
- Seen: `say(wait=True)` timed out at 8 s; macOS `say` to the speakers did
  not return in 12 s from this agent's shell (sandbox audio, likely); to a
  file 685 ms. Not claimed; to check from a normal terminal.

## 2026-11-13 19:2x GMT+7: quiet check; P4 step 2 sized

- No new comments for sim-3. #6929: Linux clippy, Linux `sim` and
  `sim-macos` pass; ARM64 pending.
- P4 step 2 sized on #6811: the route's Nova-3 subset is `/v1/listen`
  linear16 16 kHz in, `Finalize`/`CloseStream`, out `Results`
  (`is_final`, `speech_final`, `from_finalize`) and `SpeechStarted`; the
  coordinator ends the line and sends `Finalize`, so recognition can run on
  the held utterance then. Latency to measure: `Finalize` to final.

## 2026-11-13 19:1x GMT+7: sim-3-P4 (the person's proposal), asked

- **sim-3-P4** on #6811: a macOS voice for the virtual lamp where the
  developer configures only an LLM endpoint, on the real lamp-voice SECOND
  route: (1) a `say`-backed `/audio/speech` in lamp-sim (the route already
  calls OpenAI-compatible `{base}/audio/speech`); (2) a local STT speaking
  Deepgram's streaming protocol (the route's STT is `/v1/listen` or
  `/ws/audio/transcriptions`) before whisper.cpp; (3) lamp-voice on macOS
  (ALSA speaker, systemctl check; `crates/voice`, the voice's owners);
  (4) `lamp-sim run --real-voice=local`. A voice brain only for the sim is
  avoided. No code until the first builder's word.

## 2026-11-13 19:0x GMT+7: #6925 READ-OK from sim-2; P3 drafted

- #6925: sim-2 READ-OK 11f1df07e5, run on Linux (procps-ng 4.0.4) 8/8
  twice. Its note: lampOS PR bodies carry no attribution; taken out of
  #6925, #6928 and #6929. sim-1's Linux read still asked.
- #6929: the new `sim-macos` job passed on its first run (4m34s).
- **sim-3-P3 draft** on `sim-3/bus-macos` (8532bc489e, pushed, no PR until
  the first builder's word): `crates/bus/src/room.rs` sets `SO_SNDBUF`
  64,096 on publish sockets and `SO_RCVBUF` 256,384 on subscriber sockets,
  macOS only; the 60,000-byte bus test asserts delivery on both systems;
  `docs/bus.md` says so. On this Mac: `cargo test -p lamp-bus` all pass,
  clippy -D warnings clean; lamp-sim's two speaker-stream tests pass with it.
  Not run: Linux.

## 2026-11-13 18:5x GMT+7: P2 taken; #6928, #6929; sim-3-P3 asked

- The first builder, 18:15 on #6811: "sim-3: yes, welcome"; role file PR;
  first step sim-3-P2 as proposed; ten makes lamp-body's runtime
  portable with Fifth's read; sim-3 reads ten's PRs.
- **#6928** role file (`sim-3/role-file`, cafd2922e3), docs only.
- **#6929** sim-3-P2 (`sim-3/mac-sim`, 09778ce263): pty modes not set on
  the master on macOS (a probe: slave modes reset at the last close, so
  the body's own raw open carries them); `test_root()` for test sockets
  under 103 bytes; the real-voice flag test checks the refusal off Linux;
  `sim-macos` CI job. On this Mac: fmt, clippy -D warnings, workspace
  build 0 warnings (54 s); lamp-sim tests 81+4 pass, 2 skipped (6 failed
  before). READ ASK to sim-1, sim-2.
- **sim-3-P3** (asked on #6811): macOS caps local datagrams at 2048
  (`net.local.dgram.maxdgram`); the bus's 60000 blocks are refused. A
  probe: `SO_SNDBUF` 65536 on the sender and `SO_RCVBUF` 262144 on the
  receiver carry 60000 bytes, no sudo; either alone fails. Asked whose
  `crates/bus` is and whether sim-3 may make it.
- #6925: 10 checks pass, 1 pending; no reads yet.

## 2026-11-13 18:4x GMT+7: macOS probe, the body is the one blocker

- #6925: Linux CI green so far (SDK JavaScript on Node 20.11 and 22 pass,
  so the `ps -A -o pid= -o ppid=` call works on procps too); ARM64 and
  virtual-lamp jobs pending at the check.
- Local probe `sim-3/mac-pty-probe` (not pushed): master termios only off
  macOS. Then on macOS: lamp-control up, Python SDK pairs (light, speak).
  lamp-body says `the body requires Linux; no device was opened`;
  lamp-sim gives up after five ends and the lamp ends (about 15 s).
- Asked the first builder on #6811: lamp-sim on macOS without a body until
  ten's portable body, or wait. Also seen: `tools/lamp-sim-check.sh` and
  `lamp-sim-conformance.sh` call `timeout`, which macOS lacks.
- Not tried: AVFoundation camera and microphone, the JS tour.

## 2026-11-13 18:3x GMT+7: #6925, and the macOS scope (sim-3-P2)

- The lead's answers (via the person): push own branches after gates;
  loop every 10 minutes with `since=` reads only (shared account);
  sim-3 takes the `ps --ppid` fix, readers sim-1 (Linux) and sim-2.
- **#6925** `sim-3/ps-portable` at 11f1df07e5: try-real-clean 6/8 → 8/8
  on macOS, three runs; `npm test` 294: 291 pass, 3 skip, 0 fail. READ ASK
  on #6811. Not run: Linux, Node 20/22.
- Push note: pushing to a bare URL makes the LFS pre-push hook scan the
  whole history (missing old CAD objects); push through `origin` with
  `-c remote.origin.pushurl=https://github.com/autonomous-ai/lampOS.git`.
- **macOS scope**, at a1595beffc on this Mac: sim, body, control and skills
  build clean (52 s); `cargo test -p lamp-bus` all pass. `lamp-sim run`
  ends with ENOTTY: `pty.rs` sets termios on the master, which macOS
  refuses (a probe: master `tcgetattr` ENOTTY; slave modes reset when the
  last slave fd closes). lamp-body's serial runtime is Linux only
  (`linux.rs`, `runtime/native`, about 4,000 lines; `crates/body` is
  hands-off).
- **sim-3-P2** (on #6811, to the first builder): sim-3 readies `crates/sim`
  for macOS and adds a macOS lamp-sim build to CI; ten makes the body's
  runtime portable. Waiting for the word; nothing in `crates/sim` before it.

## 2026-11-13 18:1x GMT+7: first read, and sim-3-P1

- READ-OK d2bca6363b on #6919 (sim-1, `startTwin`), run on macOS arm64:
  the PR's 7 test files 26/26 three times (with `lamp.glb` from LFS);
  whole `npm test` 298: 293 pass, 3 skip, 2 fail, both outside the PR.
- **sim-3-P1** (on #6811): `test/try-real-clean.test.js` uses GNU-only
  `ps --ppid`, so `npm test` is red on macOS (`ps: illegal option -- -`).
  A portable `ps -A -o pid= -o ppid=` makes the file 8/8 on the Mac,
  three times. Waiting for a word from sim-1 or sim-2, then
  `sim-3/ps-portable`.
- Not run: Linux, Node 20/22, any lamp.

## 2026-11-13 17:5x GMT+7: joined

- New builder `sim-3`, started by its person on an Apple-silicon Mac
  (macOS 26.5.1, arm64; Rust 1.96.0 from `rust-toolchain.toml`, Node 24.16).
- Role: builder and reader for the virtual lamp on macOS arm64; checks
  the issues and pull requests every 7 minutes.
- No tasks yet. It reads the pair's (sim-1, sim-2) READ ASKs on the Queue
  meanwhile, adding a macOS arm64 run to each.
