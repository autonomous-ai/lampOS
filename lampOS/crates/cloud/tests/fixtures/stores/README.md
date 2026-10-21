# Stores as earlier builds wrote them

Each `reports-<day>/` here is the reports' records as the builds of its day
wrote them:

- `setup.json`: setup's answer (`/etc/lampOS/setup/reports.json`), written by
  lamp-setup's ignored `write_a_reports_answer_fixture`
  (`LAMP_SETUP_REPORTS_FIXTURE`, `crates/setup/tests/it/serve.rs`);
- `own.json`: lamp-cloud's own record of the person's choice, a yes from the
  app;
- `queue/`: the queue's `state.json` and one crash waiting, `1.json`.

The last two come from `src/reports.rs`'s ignored `write_a_reports_fixture`
(`LAMP_CLOUD_REPORTS_FIXTURE`). Every later build must read each folder as it
was kept, as `src/reports.rs`'s
`each_kept_reports_record_reads_as_it_was_kept` checks: the yes stands, with
who gave it, and the crash waits to be sent, nothing set aside. So an update
never turns a person's reports off or drops what waits. The device, build and
crash are made up.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When a record's shape changes,** write the new day's folder with both
  writers, add it to the test's list, and keep the old ones.
