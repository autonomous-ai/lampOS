# Stores as earlier builds wrote them

Each `off-<day>` here is the skills a person turned off by voice
(`/var/lib/lampos-skills-state/off`, `--state-dir`), as the build of its day
kept it: one skill's name a line. It is written by `tests/it/skills.rs`'s
ignored `write_an_off_fixture` (`LAMP_SKILLS_OFF_FIXTURE`), which turns `sip`
off through the engine's own `stop`.

Two parts read it, and every later build of each must keep the same skills
off:
- lamp-skills at a start, `keep_off_in`: `tests/it/skills.rs`'s
  `each_kept_off_file_keeps_its_skills_off`;
- lamp-cloud for the app's skills page, `read_off`: `src/skills.rs`'s
  `each_kept_off_file_reads_the_same_skills_off`.

The folder is in `/var/lib`, so a restart in the same boot and one after a
reboot read the same bytes.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When the file's shape changes,** write the new day's file with the
  writer, add it to both tests' lists, and keep the old ones.
