# Stores as earlier builds wrote them

Each file here is lamp-schedule's store as the build of its day wrote it:
`state-<day>.json` is the state snapshot, and `store-<day>.sql` the
database around it, as SQL. Every later build must read each one, as
`tests/it/kept.rs` checks, so an update never leaves a lamp's reminders and
timers behind.

`delivery-<day>.json` is the voice's delivery ledger (`src/delivery.rs`), as
the build of its day saved it in its own store: three timers, one said with
its acknowledgement planned, one tried again after an interrupted attempt,
and one only prepared. Every later build must open each where it was, as
`tests/it/delivery_ledger.rs`'s
`each_kept_delivery_ledger_opens_with_every_delivery_where_it_was` checks, so
an update never says a reminder twice or loses one that's due. It holds no
reminder's words.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When the state's or the database's shape changes,** write the new
  day's file with the ignored writers in `tests/it/kept.rs`
  (`write_a_state_fixture` with `LAMP_SCHEDULE_STATE_FIXTURE`,
  `write_a_store_fixture` with `LAMP_SCHEDULE_STORE_FIXTURE`), add it to the
  tests' lists, and keep the old ones. The ledger's comes from
  `tests/it/delivery_ledger.rs`'s `write_a_delivery_ledger_fixture`
  (`LAMP_SCHEDULE_DELIVERY_FIXTURE`).
