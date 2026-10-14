# Voice fixture integration before qualification v6

The 22:14:54 UTC queue/fetch check found new integration head
`cf2f5b43c8113a704324c2743af3cd92bbc4fdda`: Schedule/Voice implementation
`20bf8cabe`, character documentation and lamp-4ace unit updates. Rules and
Fourth's assignment were unchanged. No Body scope answer or open Fourth PR
was found. These are repository/queue observations, not device measurements.

The isolated v5 source remained untouched. Its fifth full Mac suite and
current ARM64 cross-build were allowed to finish, then the sequence stopped
before run 6. All five suites and fourteen builds passed; the result is
[retained as partial evidence](../voice-fixtures-loaded-v5-20261015/README.md),
not a ten-run qualification. No active test or compiler was killed.

The task branch was rebased from `f2261ec48` onto `cf2f5b43c`. Backups and
hashes were retained before automatic autostash `f52753557` applied. Eleven
of fifteen task files were unchanged. The coordinator, endpoint fixture,
pilot fixture and module list acquired the expected upstream additions.
Root then forwarded the new reminder argument through the default wrapper
and its inner coordinator function; the test-only EarlyEnd helper explicitly
passes None. These are the only three additional integration lines.
Independent read-only review found no issue and verified the final hashes.
Reminder behavior from upstream remains intact; unexpected Reminder input
is rejected by the endpoint/pilot fixtures.

Upstream adds pinned Linux dependency `thread-priority = 3.1.1`. It was absent
from Fourth's cache, so `cargo fetch --locked` fetched it for the Linux ARM64
and Mac targets. Cargo.lock was byte-identical before and after. The archive's
SHA-256 matches its upstream lock entry; no archive, binary, account or key
is committed. Fetch commands/output are retained separately from build gates.

Frozen v6 contains 2,367 lampOS files, 80,245,979 bytes, based on
`cf2f5b43c8113a704324c2743af3cd92bbc4fdda`. Manifest SHA-256:
`4990e9717505ae432d4cd975c38c60e7ad703df0655a3f22cbe54a556977717f`.
The fresh loaded qualification runs from that source-only export. Its
results belong to its later evidence folder; this rebase record claims no
passing compilation, full sequence or native ARM64 gate.

The log branch was separately rebased on the same integration head. All
53 prepared log/evidence files survived byte-for-byte. It contains only
Fourth's documentation/evidence beyond integration, not task runtime code.

Not shown: a completed v6 qualification, native ARM64 task-head execution,
any lamp installation or measurement, or arbitrary-load determinism. The
Body question remains pending and no Body source was edited.
