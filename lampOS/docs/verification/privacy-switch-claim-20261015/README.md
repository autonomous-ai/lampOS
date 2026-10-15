# MVP priority transition — 2026-10-15

Ready for lamp-voice: no

Coordination evidence, not hardware or runtime qualification. The Queue adds
Third's #755 privacy-switch task and #754 fallback-voice proposal. First
explicitly releases #748 from its unchanged Mac timing-test blocker when its
native ARM64 gate is green. The raw failures remain failures.

`relative-ready.json` verifies non-draft #748, unchanged head 365e36d and its
exact published body. `privacy-claimed.json` verifies Third's first claim.
`log-rebase.json` records rebase onto e4922a3cb and byte preservation of all
517 previous log/evidence paths before adding this entry. The previously posted
MVP waiting notices remain historical, superseded for #748 by First's reply.

Task #699 source is preserved at d82463130c7ba2012eb704733be2dff02c62019c
on third/shared-settings, with no PR. It is unfinished and not ready. Its
source checkpoint retains the failed initial compilation and successful
three-package test compilation; no tests were executed and no full gate,
source-copy, latency or native qualification is claimed.
