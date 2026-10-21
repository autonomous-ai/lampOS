# Stores as earlier builds wrote them

Each file here is lamp-schedule's store as the build of its day wrote it:
`state-<day>.json` is the state snapshot, and `store-<day>.sql` the
database around it, as SQL. Every later build must read each one, as
`tests/it/kept.rs` checks, so an update never leaves a lamp's reminders and
timers behind.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When the state's or the database's shape changes,** write the new
  day's file with the ignored writers in `tests/it/kept.rs`
  (`write_a_state_fixture` with `LAMP_SCHEDULE_STATE_FIXTURE`,
  `write_a_store_fixture` with `LAMP_SCHEDULE_STORE_FIXTURE`), add it to the
  tests' lists, and keep the old ones.
