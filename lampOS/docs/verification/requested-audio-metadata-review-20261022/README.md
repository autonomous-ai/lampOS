# Requested audio: retained metadata refusal

This is a read-only source/evidence review for [#1757](https://github.com/autonomous-ai/lampOS/pull/1757),
not a new test run or a fixed-cause claim. The original failed gate remains
in [its record](../second-music-integration-linux-20261021/README.md).

The original pack command reported:

```text
lamp-skills: not packed: audio file changed while checking
```

The `not packed:` prefix comes from the initial `load()` in
[package.rs](https://github.com/autonomous-ai/lampOS/blob/0a878cd4d9d44867b2971e70c118244f0c21ad38/lampOS/crates/skills/src/package.rs#L348),
before archive creation. [Asset::load](https://github.com/autonomous-ai/lampOS/blob/0a878cd4d9d44867b2971e70c118244f0c21ad38/lampOS/crates/skills/src/requested_audio.rs#L261)
checks the description, manifest and WAV. Any of their reads or final
stability checks can produce this generic refusal; it does not establish
that the WAV changed.

`PlainFile::stable` compares file types, symlink status, and descriptor/path
stamps: device, inode, size, modification/change times, mode, owner and
group. Access time is excluded; a parent-directory rejection uses different
words. The gate driver waited for rendering to exit before packing, and
the renderer writes synchronously. No overlapping producer is recorded.

The saved later metadata probe read each of the three files three times;
all four stamps per read agreed. It records ctime later than mtime by
377,477,813 ns for the manifest and 461,508,577 ns for the WAV. These later
values do not establish which stamp differed during the original refusal,
or why the times differ. Earlier fresh-render/storage probes also passed
and are not repeated here.

[review.json](review.json) records the original output, hashes, source
identity and selected observations from that saved probe, without host
paths. The requested-audio source remains byte-identical to the frozen
file involved in the original failure.

## Next useful observation and limits

If this boundary is instrumented, record only on failure: the fixed file
role (description, manifest or recording), comparison phase, original/opened/
listed stamps, and which check differed. Omit file contents and absolute
paths; retain every existing refusal. An unchanged retry would add little
discrimination. This diagnostic slice is recommended on Queue #2602.

The available evidence cannot identify the root cause and does not justify
changing the guard. No new test, local ARM64 gate, container or device was
run during this review. Current native qualification remains separate.
