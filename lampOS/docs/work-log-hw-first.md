# Work log: hw-first

The hardware lead's agent ([builders/hw-first.md](../builders/hw-first.md)).
Newest first.

## 2026-10-24

- **lamp-dbda offline** (no answer on the office network). Pulled lamp-voice.
  Merged since: #1832 (burn-in v1.2), #1848, #1834 (hw-member1..6), #2184,
  #2528 (hw-member1's ring and privacy scripts).
- **#3357:** `routine-run.py`, the routine run on one lamp, each step announced
  on the lamp's speaker from pre-recorded prompts (`make-prompts.sh`, macOS
  `say`, -20 dBFS, the lamp's own volume, never a tone). lamp-voice is stopped
  for each prompt and started again for the steps that need it. Also
  `heat-steps.sh`. Host test: 15 cases with stand-ins. hw-member1's agent hit
  the usage limit before writing any of it.
- **With the hardware lead** (asked on #1764, 21 Oct): the pilot limits #2461,
  the pilot units, where records live, how the privacy slide is wired (69tc).

## 2026-10-20 (later)

- **lamp-dbda heat passes** on burn-in v1.2: CPU zone peak 79 °C, 10 min, 8
  cores, one fan, no guard pause (#1852). The morning's fail was v1.1 reading
  the hottest of 8 zones; v1.2 reads the CPU zone, as `main`'s reference does.
- Ring pixels pass (webcam), privacy switch pass, reset pass (#1849). Software
  findings went to Leo, #2208. No test tones from now on; music at the lamp's
  level only, and only on request.

## 2026-10-20

- **Handover posted.** lamp-dbda's state and next steps on #1849 (claim
  released for hw-member1's next session). The hourly line is on #1764.
- **Opened:** #2181 (lamp-dbda's camera on USB port 3-1.3.4, not 3-1.1.4)
  and #2182 (head pads on 37, 96 and 97). **Proposed** hw-first-P1 on #1722
  (body vs `--gaze`, install's missing body tools, hard-coded camera port,
  the voice declining in a busy room).
- **#2184:** the V1-to-V2 bench procedure and `deploy/lamp-dbda/`.

## 2026-10-19

- **lamp-dbda, as hw-member1, with its person at the lamp.** V1 → lampOS V2
  build 3309 (`4a39f0412`) by `lamp-update install` from a local channel
  signed with a test key. The person made the key and placed it; the
  agent's permission system refused both, rightly. Models from the draft
  release `models-2026-10-16`, checked against `models.json`. V1 disabled,
  backed up. Factory step 1 written (stand 2635/2602, watched through Chrome
  posting camera frames to localhost, since the agent's tmux has no camera
  access). Step 2: three lines, nothing written. Drop-ins for the body
  (`--gaze`) and the eyes (camera port). Step 4: 12 pass, 0 fail, 3 not
  checked.
- **hw-bench renamed hw-member1** at the hardware lead's word (#1834, label,
  #1849–#1852).
- **#1848:** 26 test cases for matrix v0 and `rtm.py`, which builds the
  traceability matrix: 39 of 39 checks covered, 18 ready. Issues #1849–#1852
  for hw-member1.
- **#1834:** hw-bench's role file, from HARDWARE-TEMPLATE.md.
- **#1832:** the burn-in, gentler: SD at ≤4 MB/s, synced 1 MB chunks; a
  thermal guard pausing at 85 °C and stopping at 88 °C; INCOMPLETE, never
  PASS, on a skipped or stopped stage. Tested in arm64 Docker with a fake
  thermal zone.
- **Read:** hardware-process.md, the Hardware Queue (#1764), #1700, #1722,
  the hardware lead's bench kit (#772, `lamp-hardware-test`). Compared the
  stress tests with the kit: only memory, storage and heat existed (burn-in).

## Open

- From the hardware lead: #2181, #2182, the inspection list, the fleet
  record's home, the 10 missing limits, the pilot lamps, who approves
  shipment, #772 and #693 (fold or close).
- lamp-dbda keeps a test update key until the owner's key and an official
  channel build exist.
