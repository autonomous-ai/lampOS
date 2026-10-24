# Actual release-pair startup failures, 24 October 2026

Ready for lamp-voice: no. These are retained failed execution attempts,
not rollback compatibility or cleanup passes. Part of #2809; no lamp used.

The Memory fixture at staged tree `9d7e75d67ac6b2de3df296c39c1870474c5a030c`
compiled on Linux AMD64 and executed the actual published AArch64 6408/6449
binaries through kernel binfmt in a rootless, offline cached container.
The two cases reported **0 passed / 2 failed / 0 ignored** in 10.17 seconds.
Both actual servers exited before readiness with `Invalid argument (os error 22)`.
No RPC corpus, write, forget or reopen assertion ran. Both processes/containers
were cleaned up, and all 10,311 source files/index remained unchanged.
The complete test output is [memory-failed.txt](memory-failed.txt).

The two published Memory binaries are byte-identical: 4,738,136 bytes,
SHA256 `9f595301253679a6c9a1aea232d14b081147fe966d87b6776899ede19ccb8deb`.
Even a later successful run would prove behavior of these actual artifacts,
not compatibility between different Memory implementations. Diagnosis is in
progress. Native Unix socket creation succeeded once on both the mounted
evidence path and native Linux temporary storage, so the mount alone has not
been established as the cause. No assertion or production guard was weakened.

A separate actual 6408 Voice metadata probe exited before metadata creation:
[voice-failed.txt](voice-failed.txt). Its SHA256 is
`af7cd0d1e384ebddcca2b58eea94176c80aba573a5f47f4cd95cc8ce1a7ad084`.
Both selected source versions check real legacy service ownership before
creating the writer. Fixed systemctl calls must establish that hal.service,
os-server.service, led-boot.service and led-shutdown.service are inactive or
absent. A rootless container does not establish that prerequisite. The process
was reaped and named container absence verified; 6449 was not attempted.
This failed probe establishes no producer-layout or Records cleanup result.

The separate Records implementation is claimed in Queue comment 6378347221,
after open and merged PR searches. It will execute actual private cleanup,
active-session protection and old shared-layout refusal under native systemd,
with genuine muted Voice-produced files. It must fail if real process visibility,
legacy absence or filesystem prerequisites are missing. No fake manager or guard
bypass, device action, signature-trust claim or score increase is part of it.

Published payload sizes/hashes/AArch64 headers and decoded manifest entries
were verified separately. Release signature trust remains unverified. The fixed
pair is build 6408 (`ee53765c8`) and build 6449 (`be16c3327`), not today's source.
Common #3064 continues its independent host gates; these failures do not alter
its source or substitute for its required exact-head ARM64/Clippy qualification.
