---
name: relaxing-music
description: Play one quiet, 48-second original instrumental piece only when the person speaking to the lamp explicitly asks for relaxing music. Never play proactively, from a text message, or just because someone seems tired. No song search, playlist, repeat or volume change.
---

# Relaxing music

This skill plays one original instrumental sketch: slowly overlapping chords
and sparse upper notes, with a gentle beginning and ending. It contains no
speech, recordings or borrowed melody. Its name is a description, not a claim
of a health benefit.

The person must ask aloud. One short spoken confirmation precedes the piece.
Voice owns both the confirmation and playback; a Stop, interruption, privacy
closure or speaker mute cancels the request. Reopening privacy or unmuting
does not restart it. The skill never changes volume, light or body position.

`LAMP_REQUESTED_AUDIO=on` is required by the requested-audio feature. The
feature is off by default, and installing the package does not turn it on.
Messages-origin requests are refused, including messages with `speak=true`.

The source directory is a template, not a complete installed skill. See
`README.md` for rendering and packaging. `PROVENANCE.md` and `LICENSE` apply
to the score, renderer and generated recording.
