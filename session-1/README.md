# Session 1 of the lamp-channel recording, lamp-dbda, 28 Nov 2026 07:34–10:25

Backend's "Hey Lamp" clips (#8907) played from Mac 2's speaker in 32 blocks (blocks/, index.tsv: 2,400 train clips,
600 held-out test clips, 200 OpenAI test-only clips), recorded as lamp-dbda's detector hears them: the bus's
mic2/audio, with lamp-name stopped so the lamp answered nothing. The office was empty (Saturday), so no person.
- mic2_audio.jsonl: lamp-bus record (header at_us on the lamp's CLOCK_MONOTONIC; payload_b64: a JSON meta line, "\n",
  then PCM 16 kHz S16LE mono, 20 ms frames).
- plays.tsv: each block's lamp monotonic ns before afplay, and Mac 2's wall ns around it. session.log, index.tsv.
- SHA256SUMS.lamp: the lamp's checksums before the copy. SHA256SUMS: every file here.
Kept for training (the product owner, 28 Nov: "always store all data. they are available for training").
