# The body's stores as earlier builds wrote them

What a lamp already holds for its body, kept so every later build is tested
on it (#5759, the build row "Updates cover the whole lamp, V3 to V5").

- `lamp-env-*`: the lamp's own poses file, `/etc/lampOS/body/lamp.env`
  (`BODY_POSES`), one file per shape:
  - `lamp-env-standing-<day>` and `lamp-env-stood-swept-<day>`: written by `src/poses.rs`'s ignored
    `write_the_store_fixtures` (`LAMP_BODY_STORE_FIXTURES`) with the build's
    own writers: lamp-body-measure's rest read, and the first stand's write
    over a rest read with a hand sweep's line appended.
  - `lamp-env-06c1-by-hand-2026-10-16` and `lamp-env-fc4b-factory-2026-10-30`:
    the real files of lamp-06c1 (written by hand, with `--gaze`) and lamp-fc4b
    (the factory's steps, `--gaze` last), copied from `deploy/`.
- `measured-env-seated-<day>`: the generic package's seated read,
  `/var/lib/lampos-body/measured.env` (lamp-body-measure `--seated`, #2163),
  written by the same test. The product unit never reads it (it refuses a
  file with no `--stand`); the generic unit does.
- `first-stand-*.json`: the first stand's record,
  `/var/lib/lampos-body/first-stand.json` (`first_stand::Record`), stood and
  still waiting, written by the same test.

Three readers, and each later build of each must read every file:
- lamp-body, `lamp-env-*` from the product unit and `measured-env-*` from
  the generic one: `src/bin/lamp-body.rs`'s
  `the_body_starts_on_every_kept_poses_file`;
- the let-go before the power goes: `src/bin/lamp-body-measure.rs`'s
  `let_go_reads_every_kept_lamp_env`;
- the first stand's record: `src/first_stand.rs`'s
  `every_kept_record_reads_as_it_was_written`.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When a shape changes,** write the new day's files with the writer and
  keep the old ones.
