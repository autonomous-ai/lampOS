# Stores as earlier builds wrote them

Each `session-<day>/` here holds a voice session folder as the voice of its day
laid it out under `/var/lib/lampOS/voice/sessions`: `<id>/metadata/` with
`events-0.jsonl` to `events-3.jsonl` and `health.json`. The layout is the
voice's, as found on lamp-a63e on 22 Oct (names only); the contents are made
up, never a lamp's, since lamp-records reads a session's names and sizes, not
what is in it. Git keeps no modes: the test makes the folders 0700 and the
files 0600, as the voice does.

Every later build's lamp-records must take each one as a session, with nothing
refused, as `src/store.rs`'s `each_kept_session_folder_is_taken_as_a_session`
checks, so a put-back lamp-records neither stops on a newer voice's sessions
nor leaves them unpruned. The sessions are in `/var/lib`, so a restart in the
same boot and one after a reboot see the same folders.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When the voice's layout changes,** add the new day's folder, add the day
  to the test's list, and keep the old ones.
