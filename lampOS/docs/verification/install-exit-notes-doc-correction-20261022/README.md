# Installer review documentation corrections

PR [#2767](https://github.com/autonomous-ai/lampOS/pull/2767) is pushed at
`daa23b2832cd7460a4fd5c7d87461bb4b523092d`, amending the reviewed
`19d1a46d952d8301871a69caf48c04d5c21c0bfa` in one task commit.

Reviewer-lead found no P1/P2 and pointed out two stale descriptions. The
product README now describes the 33 per-service exit-note drop-ins installed
by install; the own-keys comment no longer says install never writes drop-ins.
The install guide also records that systemd waits for the stop hook: a failed
stop adds a recorder process and a systemctl query capped at two seconds.
Total restart overhead has not been measured on a lamp.

Only the two documentation files and one comment changed. The entire
`lampOS/crates` Git tree is identical; own-keys unit directives are identical
after excluding comments/blank lines. Exact-file scope and diff-check passed.
No extra local crate gate was run for these prose/comment corrections. Earlier
execution receipts remain attributed to their original heads. Fresh required
[ARM64 39744451280](https://github.com/autonomous-ai/lampOS/actions/runs/39744451280)
and [Clippy 39744451308](https://github.com/autonomous-ai/lampOS/actions/runs/39744451308)
are pending at this checkpoint. First owns the default-install merge lane.
No device access or latency measurement was performed.
