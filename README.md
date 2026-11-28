# lamp-channel 28 Nov 2026: metadata only (first builder, for the Reviewer's measuring stick, #8850)

Two recordings of the same 3,200 "Hey Lamp" clips (#8907), played from Mac 2's speaker into lamp-dbda and recorded as
its detector hears them (the bus's mic2/audio). The audio and the clips' words stay on Mac 2 (in
~/lamp-voice-data/heylamp-8907/lamp-channel-1128/). This branch holds numbers and opaque IDs only.

- index-meta.tsv: one row per clip. clip_id is sha256 of the clip's source file path, first 16 hex. block, level
  (afplay -v: even blocks 1.0, odd 0.5), clip_in_block, offset_s (from the block's start), duration_s, provider, voice,
  style, label (call / not-call), split, use. Left out: the words and the file path.
- session-1/: the first recording (07:34-10:24). The lamp answered the clips from 08:18:57 (CORRECTION.md):
  lamp-speech-session-1.tsv has the lamp's sound over them (speech, the wait sound as an estimate, rings as upper
  bounds), in unix ms of the lamp's system clock.
- r1/: the clean re-record (11:27:31-14:18:00), the lamp kept from answering; lamp-speech-r1.tsv has no rows.

**Clocks.**
- plays*.tsv: lamp_mono_ns_before is the lamp's CLOCK_MONOTONIC (ns) read just before afplay; mac_wall_ns_before
  and mac_wall_ns_after are Mac 2's wall clock (ns since the epoch) around afplay. A clip plays from
  mac_wall_ns_before + offset_s (plus afplay's start-up, about 0.1-0.2 s).
- The recordings' frames carry at_us on the lamp's CLOCK_MONOTONIC and, from the recorder's header, at_unix_ms on its
  system clock, so a clip's frames are found from lamp_mono_ns_before + offset_s. Both machines' wall clocks are
  NTP-kept.
- lamp-speech*.tsv: start/end_unix_ms from the lamp's journal (system clock).
- SHA256SUMS and SHA256SUMS.lamp in each folder: the sealed files on Mac 2 and the lamp's own sums before the copy.
