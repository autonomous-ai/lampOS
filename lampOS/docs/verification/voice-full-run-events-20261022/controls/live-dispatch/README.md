> Historical preparation plan, frozen before execution. Wording below such as
> “unexecuted” describes that preparation stage. The probes have now run;
> see the [completed verification record](../../README.md) and
> [command results](../commands.json). The two logical-clock opt-ins in the
> lease plan have since been folded into the submitted tests.

# Requested-audio dispatch controls

Apply each patch independently to a disposable worktree. None has been run.
The exact test is
`provider_worker::tests::requested_audio_crosses_the_live_worker_only_after_its_tool_reply`.

- `before-setup-pause.patch`: against `69f2228338a8df63a81d23087c3e55308fa125ca`.
  Injects four seconds immediately after the fake Skills thread is spawned,
  before `LiveRig::start_with`. Expected: the original `music_accept` unwrap
  reports `TimedOut` because its unchanged three-second accept budget runs
  during unrelated setup. The original tool-response comparison must not pass.
- `after-setup-pause.patch`: against that base plus the five-line dispatch arm.
  Injects the identical four-second setup pause. Expected: the complete original
  test passes for both direct and wrapped calls; its original reply, offer,
  audio and completion assertions all run. The timeout remains three seconds
  after the helper receives the dispatch arm.
- `after-omit-dispatch.patch`: against the same five-line candidate.
  Arms the helper but omits the model tool call. It directly reaches the
  existing `skill.join().unwrap()` so the helper's original three-second accept
  failure is observed before the competing three-second service-message bound.
  Expected: child `music_accept(...).unwrap()` reports `TimedOut`, then the
  existing join unwrap fails the named test. Require both diagnostics, not
  merely a nonzero exit. This temporary fault path intentionally does not run
  the success-path reply assertion; candidate source keeps that assertion.

No patch changes `music_accept`, its accept/read/write limits, any product
code, authority lease, freshness limit, or successful test assertion. The
setup pause occurs before any worker is created, so it does not consume an
active worker lease. The missing-dispatch control blocks only on the bounded
helper on its intentional failure path; use an external 30-second process
watchdog and treat an external timeout as a failed control, not evidence that
accept timed out correctly.

The setup-delay comparison assumes the OS schedules the helper during the
four-second injected pause. Capture the intended `TimedOut` diagnostic; if it
is absent, report the control as unproven rather than retrying until green.
Compilation is excluded from control timings. Expected success does not
establish that the full parallel suite passes.

`apply-checks.json` records exact base/candidate hashes, patch hashes and the
successful apply checks. The base check used a temporary external Git index;
candidate checks used the exact current working file. The index was removed.
A cached candidate-index check was denied object-store writes by the sandbox;
the direct candidate checks provide the required read-only alternative.
No repository file was changed by preparing these controls.
