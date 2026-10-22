> Historical preparation plan, frozen before execution. Wording below such as
> “unexecuted” describes that preparation stage. The probes have now run;
> see the [completed verification record](../../README.md) and
> [command results](../commands.json). The two logical-clock opt-ins in the
> lease plan have since been folded into the submitted tests.

# Lease and DeclineMusic pause probes

These are external, unexecuted probes. Apply one combined patch at a time in
a disposable worktree. Compile each variant once, then run each of the three
exact tests once. Retain all six outcomes and raw failures; do not retry until
green. Compilation is excluded from test wall times. Use a bounded external
process watchdog (30 seconds per exact case) and reject a watchdog timeout as
an inconclusive probe, not the expected scenario failure.

`before-three-pauses.patch` applies to exact base
`69f2228338a8df63a81d23087c3e55308fa125ca`.
`after-three-pauses-with-existing-clock.patch` applies to the current candidate:
its DeclineMusic dispatch arm is present, and timer/reminder are still unchanged
before applying this scratch patch. Exact input and output SHA256 values and
successful apply checks are in `apply-checks.json`.

The first two probes inject the identical `std::thread::sleep(200 ms)` in both
variants. Only their after variant selects the existing paused Tokio runtime
and `let _clock = logical_clock()`. The product's 100 ms lease, all provider
limits, assertions, observation loops and deadlines remain byte-identical.

| Case | Injection point | Before expectation | After expectation |
| --- | --- | --- | --- |
| Timer | In the first loop, immediately after `rig.speak` returns and before `desk.decide_with`; repeated for the original true/false rows | An expired owner makes the original response/dispatch sequence fail; preserve actual diagnostic | All original timer, volume and cancel assertions pass despite the same host pause |
| Reminder | Immediately after `rig.reminder`, before awaiting `Started` | The original Started observation fails after owner revocation; preserve actual diagnostic | Started, audio, original 120/181-second room checks and completion all pass |
| DeclineMusic | Immediately after `DeclineMusic::new`, before `LiveRig::start_with`, four real seconds | Expected captured family signature: unavailable reply instead of `Confirm.`, with executive did not answer in 0 ms | The complete original direct/wrapped single-offer/single-confirmation test passes |

Exact names:

- `provider_worker::tests::policy_holds::a_timer_in_a_turn_that_follows_the_lamp_is_set_though_the_policy_said_stay_quiet`
- `provider_worker::tests::reminders::a_reminder_rung_into_a_busy_room_is_no_answer_to_end_it_by`
- `provider_worker::tests::review_2026_10_14::a_music_action_before_decline_keeps_its_single_offer_and_confirmation`

The lease mechanism is specific: `LiveRig::heartbeat` calls
`Controller::set_capture`, which first calls `advance`. If the prior capture or
admission lease elapsed, `advance` revokes the turn before the new lease is
installed (`turn-owner/src/controller.rs:116` and `:392`). Thus renewing after
an actual 200 ms host stall cannot resurrect the original owner. The next
snapshot has no owner. Timer's request was already submitted; Reminder's start
was still queued. Retirement/missing owner can therefore fail these two phases
with different diagnostics. Neither phase is guaranteed to print literal
`StaleOwner`; retain the actual response, worker-end, or missing-output failure.
A different/unexpected failure remains an observation, not proof of this cause.

The after clock is already used by adjacent provider-worker fixtures. Its
scenario time advances only in rig ticks, while the injected host sleep does
not consume the 100 ms logical lease. This is a diagnostic alternative only;
it has not been applied to candidate source. Gemini capture still uses its
existing clock configuration; no new capture seam is part of these probes.

DeclineMusic's first accept currently starts in its constructor on the original
base, before the rig's shared lock/startup work. Its timeout silently ends the
helper and closes the listener. The four-second pause reproduces that setup
ordering if the OS schedules the helper during the pause. If the expected
unavailable reply is absent, report the control unproven. The candidate waits
for its explicit arm immediately before the first music-containing message;
its original three-second accept/read/write limits are unchanged.

The full unmodified local baseline passed all seven initially named cases and
failed two other cases. These injected probes do not establish the cause of an
original x86 failure unless the captured diagnostic matches that evidence.
No candidate source, test assertion, production code, or deadline was edited
while preparing these patches. No Cargo command or probe has been run here.
