# Stores as earlier builds wrote them

Each file here is memory across days (`memory-days.sqlite3`) as the build of
its day wrote it, as SQL: its tables and indexes as that build made them,
every row, the full-text index's rows through its own table, and
`user_version`. Every later build must open each one with what it held, as
`tests/it/kept.rs` checks, so an update never leaves a person's memories
behind.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When the database's shape changes,** write the new day's file with the
  ignored `write_a_days_fixture` (`LAMP_MEMORY_DAYS_FIXTURE`), add it to the
  test's list, and keep the old ones.
