# Segment r1 of the lamp-channel recording, lamp-dbda, 28 Nov 2026 recording–14:18:00

Backend's "Hey Lamp" clips (#8907) played from Mac 2's speaker in blocks (see session-r1.log for which, and
index.tsv or index-2.tsv one folder up), recorded as lamp-dbda's detector hears them: the bus's mic2/audio.
- How the lamp was kept from answering: lamp-name stopped; lamp-voice running (it publishes mic2/audio) with a /run
  drop-in turning LAMP_SECOND_NAME_CARRIES and LAMP_CALL_TAKES_NEXT_LINE off (session 1's spoiling path); a watchdog
  on lamp-voice's journal stopped playback at any sign of the lamp letting a line out or speaking (watchdog-r1.log).
- plays-r1.tsv: each play's lamp monotonic ns before afplay and Mac 2's wall ns around it, its try, and complete
  (1, or 0 when the watchdog stopped it; a block stopped is played again after 40 s of quiet).
- lamp-speech-r1.tsv: the lamp's speech, wait sound and rings from journal-lamp-voice-r1.txt.gz (tools/
  speech_intervals.py); empty when the lamp said nothing.
- mic2_audio.jsonl: lamp-bus record (header at_us on the lamp's CLOCK_MONOTONIC; payload_b64: a JSON meta line,
  "\n", then PCM 16 kHz S16LE mono, 20 ms frames).
- SHA256SUMS.lamp: the lamp's checksums before the copy. SHA256SUMS: every file here.
Kept for training (the product owner, 28 Nov: "always store all data. they are available for training").

Seal repair, 28 Nov 14:2x (first builder): the seal script read the window's start as the word "recording", so its
journal fetch failed (kept as journal-lamp-voice-r1.failed-seal-1419.txt.gz, 20 bytes) and no intervals were written.
The window is 11:27:31 (recording since) to 14:18:00 (finishing): journal-lamp-voice-r1.txt.gz now holds lamp-voice's
4,300 lines for it, and lamp-speech-r1.tsv has no rows. The lamp let no line out, said nothing and rang nothing (0 of
"its answer is played", "handed to the speaker", "answered in its place", "offered to the voice"); the watchdog never
fired (watchdog-r1.log is empty); 32 of 32 blocks played complete.
