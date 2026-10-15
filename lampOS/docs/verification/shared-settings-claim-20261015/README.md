# Shared logging and runtime settings: claim

Ready for lamp-voice: no

The [updated Queue](queue-never-idle.json) says to take the next task after
MVP work when every MVP task is waiting, and to return to an MVP task when it
becomes possible. Third's relative-reminder follow-up is [draft #748](https://github.com/autonomous-ai/lampOS/pull/748),
waiting on its unmet motor-fixture qualification condition. First has
[paused the fixture repair](fixture-task-paused.json) under #739 until after
the MVP. Touch #711 requires First and a person at the lamp.

Task [#699](https://github.com/autonomous-ai/lampOS/issues/699) is now
[taken by Third](https://github.com/autonomous-ai/lampOS/issues/699#issuecomment-6181158599),
session `third-20261014-violet4`, no lamp. Its stale body says Fourth, but
First's 17:10 reassignment comment, current builder label and Queue explicitly
assign it back to Third. [The re-read](issue-699-taken.json) confirms this
session has the first claim and `status:taken`.

Branch planned: `third/shared-settings`, from integration
`d128c458ad95d471b959b354a62eb75d93d2015b`. The log rebased onto that integration
before this claim entry; [all 506 prior log/evidence paths](log-rebase.json)
remained byte-identical. No task implementation or new gate result is claimed.

Scope: bounded shared log messages and a helper/follower in Msgs/Bus; a typed,
declared runtime settings component with validation and durable changes;
English docs, host/process/failure tests, source-only portability and native
ARM64 qualification. Study V1/current code and ROS 2 parameter behavior before
designing. Keep logging and settings in separate reviewable changes. Identify
the first three useful settings and their actual reader owners. Lamp-owned
RAM-log/image changes remain with their owners; no lamp is accessed.

Provisional interaction targets to assess against the existing paths before
implementation: log admission under 1 ms without waiting for storage, and a
healthy local settings mutation to durable response within 250 ms. These are
design targets, not measurements. Slow storage must not stall input or signal
success before durability. New runtime behavior requires explicit enablement.

[Retention hashes](retention.json) identify the original Queue, claim and
rebase records. The active MVP draft and all its failed raw runs remain intact.
