Task 31 merge review against bc3fccb546a22baaef180fcd57b3291a44e0e4c1

Read-only review of the uncommitted Task 31 source in
`/Users/d/code/autonomous-os`, 2026-10-15. No actionable merge defect found
in the requested seams.

- Coordinator order is promised camera look (`coordinator.rs:4221`),
  SecondLook (`:4259`), due reminder (`:4298`), greeting (`:4345`), then
  upstream task news (`:4383`). Each admitted Written/Reminder sets the
  next Reply and queues provider input, preventing the following paths
  from taking that owner. `idle_reminder_offer` (`:4782`) additionally
  refuses a live Reply or an owed look/SecondLook. The reminder-disabled
  path retains upstream follow-up, greeting and task-news ordering.
- New task news is still initialized only for continuous service
  (`coordinator.rs:2461`) and keeps its upstream presence, 15-second
  quiet, room-busy and provider/readiness checks. `news.tell()` remains
  after those checks; a reminder admitted earlier cannot consume news.
- CHECK remains one of `agent_tasks::tools()` and `is_tool()`
  (`agent_tasks.rs:91`, `:102`). The merged declaration filter and route
  (`tools.rs:266`, `:345`) keep it owned by Tasks and exclude executive
  catalog duplicates. The combined flags/declaration test (`:554`)
  includes CHECK and independent Schedule/Tasks disable cases.
- Reminder tools are refused by `reminder_notice` before ToolDesk or
  ordinary attention processing (`provider_worker.rs:1179`, `:1350`),
  so CHECK receives the same reminder-only veto as all other tools.
  Ordinary task/news Written requests retain the upstream route.
- The upstream End.sound wire and short-sound branch remain present
  (`provider_worker.rs:267`, `:2483`); reminder-origin forwarding
  (`:1684`) skips invented heard/unanswered room evidence, while actual
  Audio still calls RoomTold::spoke and clears UNANSWERED (`:1717`).

Limits: this was source review, plus scoped `git diff --check` (exit 0),
not a build or test execution. The new coordinator regressions
(`coordinator/tests/reminders.rs:299`, `:390`) exercise the shared
production selector, ownership and speaker-event seams; they do not run
the complete continuous coordinator loop. No lamp, live cloud, camera,
task daemon or audible-output behavior was measured. Root owns focused
checks, the gate and lamp qualification instructions.
