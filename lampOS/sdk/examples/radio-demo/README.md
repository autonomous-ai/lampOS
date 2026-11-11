# Radio drama demo (lamp-e82d, record only)

Demo app for the lamp SDK. It needs this branch's local `lamp-control` (`speaker_write`, `language` in `hello`, head `touch`). Not for merge.

- `radio.py serve`: waits for a skill's sign on the ring (a named colour at level 1), then plays, stops or continues a story from the radio-drama backend on the lamp's speaker. It dims the ring by genre, with a red flicker for dark stories, and moves the lamp with gestures. Saying the lamp's name pauses the story, and a tap on the head stops it.
- `skills/radio-play`, `radio-stop`, `radio-continue`: pushed with `lamp skill push`. The model calls them by voice, and each one shows its sign.
- `call.py`: a ring-tone and raise-your-hand-to-answer call demo.

The backend is asked on the lamp, with the lamp's own key, over SSH: set `SSHPASS` to the lamp's password. Edit `LAMP_IP` in `radio.py` to point at your lamp. Start an SSH tunnel to port 7731, because macOS blocks Python's LAN access without the Local Network permission.
