# Stores as earlier builds wrote them

Each file here is lamp-tasks' store as the build of its day wrote it:
`state-<day>.json` is the state snapshot, and `store-<day>.sql` the
database around it, as SQL. Every later build must read each one, as
`engine.rs`'s `each_kept_state_reads_with_its_tasks` and `store.rs`'s
`each_kept_store_opens_with_its_snapshot` check, so an update never leaves a
lamp's tasks behind.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When the state's or the database's shape changes,** write the new
  day's file with the ignored writer (`write_a_state_fixture` with
  `LAMP_TASKS_STATE_FIXTURE`, `write_a_store_fixture` with
  `LAMP_TASKS_STORE_FIXTURE`), add it to the test's list, and keep the
  old ones.
