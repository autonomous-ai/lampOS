# Stores as earlier builds wrote them

Each `people-<day>.json` here is the eyes' known people (`people.json`) as
the build of its day wrote it. Every later build must open each one with
everyone in it, as `src/who.rs`'s `each_kept_people_file_opens_with_everyone_in_it`
checks, so an update never leaves a lamp's known people behind. A `-scored`
file is the same day's shape with a night of familiarity scored
(`familiar`, `scored_at_unix_s`), which every later build must keep too, so
nobody known becomes a stranger again. The prints are made-up test faces,
never a person's.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When the file's shape changes,** write the new day's file with the
  ignored `write_a_people_fixture` (`LAMP_EYES_PEOPLE_FIXTURE`), add it to the
  test's list, and keep the old ones.
