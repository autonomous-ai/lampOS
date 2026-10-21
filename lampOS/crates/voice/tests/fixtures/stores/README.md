# Stores as earlier builds wrote them

Each `thread-<day>.json` here is the voice's thread file (`thread.json`) as
the build of its day wrote it. The file outlives the voice's restart, so an
update's new build reads what the old one wrote. Every later build must read
each one with every exchange in it, as `src/thread.rs`'s
`each_kept_thread_file_reads_back_as_it_was_said` checks, so an update never
sets the conversation aside. The words and the person (A) are made up, never
a person's.

- **Never edit or delete one.** Each records what a lamp may still hold.
- **When the file's shape changes,** write the new day's file with the
  ignored `write_a_thread_fixture` (`LAMP_VOICE_THREAD_FIXTURE`), add it to
  the test's list, and keep the old ones.
