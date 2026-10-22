# Reachability audit of EC to 735

This is a read-only source audit of the exact pair in [results.json](results.json).
No direct mechanism was found by which the newly opted-in fixture clocks or
new guard could change another test thread's clock or the failing motor,
body, ring, microphone or signal fixtures' shared state. Indirect contention
and scheduling effects remain unexcluded. The single before pass and 18 after
failures do not settle causality.

The full source delta and unchanged-path hashes are in
[source-audit.json](source-audit.json). The eight changed Rust files contain
fixture dispatch arming, timer/reminder logical-clock opt-ins, a conversation
guard, memory reply readiness, and per-service requested-audio fixture time.
No failing fixture file or scenario-clock implementation changed in this pair.

| Mechanism or failure path | Exact source boundary and finding |
|---|---|
| Scenario clock | `scenario_clock.rs:41` stores ACTIVE and NEXT_ID in thread-local cells. Scope contains `PhantomData<Rc<()>>`, so cannot move to another thread; Drop at 113 removes its own scope. New OS threads use their own actual clock. |
| Existing isolation coverage | `concurrent_scopes_and_new_threads_do_not_share_a_clock` at 348 concurrently advances two scopes independently and checks a newly spawned thread has no scope. It passed in both recorded runs. |
| Provider logical clock | Existing capture CLOCK is thread-local. The timer/reminder current-thread Tokio runtimes opt into paused time; LiveRig advances its own fixture scope. Neither is a process-wide time override. |
| Requested-audio fixture service | `requested_audio/runtime.rs` holds the new test clock/condition variables in each Service's fixture state. Ordinary construction leaves that optional state absent; there is no new static clock. Coordinator requested-audio fixtures explicitly construct the stepped service. |
| Added guard | The new acquisition at `second_provider/worker.rs:9178` uses existing reentrant ONE_THREAD (`provider_worker.rs:9429`). It protects the helper's existing conversation-global mutation through restoration. The failing paths do not acquire it or read conversation/room globals. |
| Existing room helper limitation | `provider_worker/tests/reminders.rs:482` RestoreRoom has no OneThread field in either head; it still changes room state outside LiveRig's guard lifetime. This is unchanged, not a new guard delta or demonstrated cause of the 18 failures. |
| Nine motor preflight failures | `coordinator/motor/tests.rs:676` unwraps a real `std::Instant` 15-second child preflight watchdog. This path has no dependency on another test's thread-local clock. |
| Three motor step failures | `coordinator/motor/tests.rs:134` unwraps the existing real 30-second step watchdog. STEPPED/STEPPED_NOW_US are only set after the inert-child environment guard at 458-477, in the re-executed child. The ignored ordinary fixture returns without that environment. Session directories are per instance. |
| Other motor failure | At `coordinator/motor/tests.rs:883`, the retained child output says connected, not ready, reaped and ControlLost, with largest poll gap 1,888 us. This does not prove why the child missed readiness. |
| Ring restart ticks | `coordinator/optional_ring_tests.rs:881` measures the actual restart tick against 50 ms using the host clock. The observations were 97.852923 and 78.187015 ms. It owns separate worker children and does not use the stepped requested-audio Service. |
| Body motion | `body_motion.rs:389` asserts the body state after three existing 80 ms waits on a real bus with a per-test directory. The returned state is Still. It has no conversation-clock dependency. |
| Duplex microphone | The case at `coordinator/tests/duplex.rs:1502` explicitly uses `Harness::on_the_host`, sleeps the hearing-stale interval plus 20 ms, and reaches real owner freshness. The failure at 424 is StaleOwner. It does not select the scenario-clock harness constructor. |
| SIGINT child | `coordinator/tests/termination.rs` re-executes `isolated_signal_child` in a separate process, so does not inherit parent TLS or mutex state. The primary child failure is at 328: downcasting an unprinted non-io error returns None. Its actual error cannot be inferred from the four later ConnectionReset panics. |

Prior reports establish similar symptoms, not attribution for this observation.
The locally preserved issue snapshot includes the exact body-motion Still
case under a controlled filesystem delay and an earlier motor-child timeout
([row 47/48 record](https://github.com/autonomous-ai/lampOS/issues/1569#issuecomment-6288791434)).
It also records the same two ring tick assertions
([row 65 record](https://github.com/autonomous-ai/lampOS/issues/1569#issuecomment-6308229830));
those can represent actual product-loop latency and cannot automatically be
classified as artificial fixture waits. An older SIGINT/StaleOwner family
([record](https://github.com/autonomous-ai/lampOS/issues/1569#issuecomment-6254295695))
does not supply the missing original error in the present SIGINT output.

No additional run, instrumentation or correction was made during this audit.
One separately authorized unchanged-current-upstream observation would test
whether the requested current head can exhibit failures without this patch;
a pass would still leave this pair unresolved, while a matching failure would
establish that symptom on that distinct upstream snapshot. It would not
retroactively match host conditions or explain the present 18 failures.
