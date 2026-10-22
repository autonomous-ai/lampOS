# Stores as earlier builds wrote them

The controls' files in `/etc/lampOS/controls`, as the build of each day wrote
them:

- `controls-<day>.json`: `controls.json`, which `lamp-update install` writes
  from `deploy/product/settings/controls.json`, byte for byte;
- `controls-pins-<day>.json`: the product's pins file, written the same way
  from `deploy/product/settings/controls-pins.json`, read with `--touch`;
- `controls-pins-<day>-moved.json`: the pins file `lamp-keeper prepare` gives
  a lamp moved from V1, made by keeper's ignored `write_a_moved_pins_fixture`
  (`LAMP_CONTROLS_MOVED_PINS_FIXTURE`, `crates/keeper/src/profile.rs`), read
  without `--touch`.

Every later build must read each day's files as `lamp-controls serve` does at a
start, as `tests/it/kept_stores.rs`'s
`each_kept_controls_file_reads_as_serve_reads_it` checks. They live in `/etc`,
so a restart in the same boot and one after a reboot read the same bytes.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When a file's shape changes,** add the new day's files (copy the product
  files; run the writer for the moved one), add them to the test's list, and
  keep the old ones.
