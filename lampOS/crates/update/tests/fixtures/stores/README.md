# Records as earlier builds wrote them

Each `state-<day>.json` and `journal-<day>.json` here is lamp-update's own
records (`/var/lib/lampos-update/state.json`, and `journal.json` for a switch
under way) as the build of its day saved them, every field filled in. Every
later lamp-update must read each with every field as it was, as
`src/records.rs`'s `each_kept_state_and_journal_reads_as_saved` checks, so an
update never loses what the one before it knew of the lamp, and a switch it
began can still be put back. The builds, commits and hashes in them are
made up.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When the state's or the journal's shape changes,** write the new day's
  pair with the ignored `write_records_fixtures`
  (`LAMP_UPDATE_RECORDS_FIXTURE`, a folder), add it to the test's list, and
  keep the old ones.
