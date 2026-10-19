# Ring stop receipt, host (#1620, 19 Oct 2026)

What failed: lamp-52e6 (release e4f1d32b, `LAMP_RING_EYE=on`), `systemctl stop
lamp-voice` after 3 h. Records: `service_stop_requested` at t0, `worker_exit_failed`
for the ring at +1.4 ms, `run_end` failed; the journal's last line
`verified cleanup failed: ring shutdown has no verified black-write receipt`.
lamp-4ace (`LAMP_RING_EYE=strong`, newer builds) stopped cleanly five times.

## What the code says

- systemd's `KillMode=mixed` sends SIGTERM to the main process only. The ring
  child (`lamp-voice worker ring ... --continuous`, spawned in
  `process/ring_startup.rs`, no signal handling of its own) is not signalled;
  it ends on the `Control::Stop` the shared shutdown sends
  (`coordinator.rs`, `Rig::shutdown`), writes black (`GuardedRing::shutdown`)
  and says `Blanked { reason: Shutdown }` (`ring_worker.rs`, `controls`).
- One black frame is 1,028 bytes at 6.4 MHz: 1.28 ms of SPI on its own. The
  stop path sends the ring an authority with both permissions denied (one
  blank write for a resting face) and then Stop (a second). A ring that was
  live at t0 cannot have written its shutdown black and been reaped 1.4 ms
  after t0. The exit seen then was already there.
- A ring that ends on its own mid-session is disabled by the loop's check
  (`OptionalRing::check_running`), recorded as `ring_degraded` and
  `ring_degraded_exit`, and audio goes on. Its exit status is cached by
  `Child::try_wait`, so the stop's first pass sees it again at once and records
  `worker_exit_failed`, which read as the stop ending the ring.
- The stop still fails, by design: a ring that ended on its own never said its
  shutdown black (its own fault cleanup is not a Shutdown receipt).
- Before d9ba8d0f2 (19 Oct 10:25) only the last cleanup failure was returned,
  so e4f1d32b's journal said only the missing receipt; today the ring's earlier
  exit and stop-delivery causes are joined into the same line.
- The eye (`on` or `strong`) only changes how the face is drawn
  (`ring_face.rs`); the Stop path, `run_guarded` and `GuardedRing::shutdown`
  have not changed since 14 Oct.

## Change

The stop's `worker_exit_failed` carries `exited_before_stop` (true for a ring
disabled with its exit already observed). Verified cleanup and the black-write
requirement are unchanged.

## Host tests

- `coordinator::optional_ring_tests::a_ring_that_ended_mid_session_fails_the_stop_and_its_exit_is_dated_before_it`:
  without the change it fails with the 52e6 record shape
  `{"error":"ring worker exited abnormally: exit status: 1","exited_before_stop":false,"kind":"worker_exit_failed","worker":"ring"}`
  (the field forced false to show it).
- `coordinator::optional_ring_tests::a_ring_live_at_the_stop_is_not_dated_before_it`.
- `tests/ring_worker.rs::every_eye_mode_writes_and_says_the_shutdown_black_at_a_service_stop`:
  no eye, eye, strong eye; resting face, the stop's denied authority, Stop:
  each writes and says the Shutdown black and exits clean.

## Not established

Why 52e6's ring ended mid-session. The session's `ring_degraded` /
`ring_degraded_exit` records and the ring's own `lamp-voice: ring worker: ...`
journal line (hours before the stop) would say; they were not read here. If no
`ring_degraded` precedes the stop, this account is wrong and the ring died
within the last loop tick before t0 for a reason not found here.
