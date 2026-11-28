# Correction, 28 Nov 2026 11:4x (first builder): the lamp did answer during session 1

README.md says "with lamp-name stopped so the lamp answered nothing". That is wrong, and README.md is kept as written.
- At 08:18:50 a bare "Lamp" clip (request 563) was answered in a held line's place: #8892's simple call with
  LAMP_SECOND_NAME_CARRIES and LAMP_CALL_TAKES_NEXT_LINE answered the name although lamp-name heard no call (it was
  stopped). Each answer opened the 20 s window, and the next clip, about 3 s later, came inside it. The lamp went
  on answering the clips until the session ended at 10:23:52, about 1,200 requests.
- The lamp's sound over the clips: ../lamp-speech-session-1.tsv (tools/speech_intervals.py, from the journal):
  speech 331 rows, 17.0 min, from 08:18:57; the wait sound, 40.9 min estimated; rings, 17.4 min at most.
  Of the 3,200 clips, 1,758 overlap one of them (speech 591, wait 1,469, ring 313) and 1,442 overlap none (the
  helper's count, 11:2x).
- The speaker: 35% until 08:24:13, two volume_downs (08:24:13, 08:33:59), muted at 10:12:35.863. Timers set by
  the clips rang between 09:45 and 10:28. 29 reminders the clips set were cancelled on the lamp at 11:14-11:15.
- Ten short mechanical bursts in blocks 0 and 3-6 (07:38-08:11, about 50 ms, near 450 Hz) are not speech; most
  likely the body's motors turning to the sound.
- Session 1's blocks are recorded again with the lamp kept from answering: ../recording-r1/.
- This recording stays as it is: kept as the condition "over the lamp's voice" (a barge-in test, and later
  training data), never deleted.
