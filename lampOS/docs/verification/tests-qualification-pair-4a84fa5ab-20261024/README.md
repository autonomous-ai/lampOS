# #3190 matched qualification on frozen 4a84fa5ab6

This bounded comparison follows [First's 00:11 rule](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6370483473) and the [reader's interpretation](https://github.com/autonomous-ai/lampOS/issues/3126#issuecomment-6370516845). One full clean-base run and one full candidate run used the same twelve packages, default parallelism and recorded environment settings. Integration advanced after this pair was frozen; these results belong only to the explicit source identities below, with latest-head qualification recorded separately. Each original or new candidate failure not matched by a reviewed base failure then received exactly three standalone observations. No failed full run was retried.

| Source | Head | Passed | Failed | Ignored |
| --- | --- | ---: | ---: | ---: |
| Matched clean base | `4a84fa5ab` | 7219 | 17 | 34 |
| Candidate | `93c995a6b` | 7234 | 2 | 34 |

Full Voice counts are broken out separately:

| Source | Voice scope | Passed | Failed | Ignored |
| --- | --- | ---: | ---: | ---: |
| base | library | 3672 | 8 | 15 |
| base | whole package | 3950 | 11 | 15 |
| candidate | library | 3679 | 1 | 15 |
| candidate | whole package | 3959 | 2 | 15 |

[Clean-base evidence](base/README.md) and [candidate evidence](candidate/README.md) retain exact commands, complete actual case censuses, executable hashes, failed diagnostics and raw-stream projections. [Comparison data](comparison.json) retains base-only failures as well. Original historical observations stay at their original heads in [the original nine](original-nine.json); no latest-base result is substituted for them.

| Case | Base full | Candidate full | Standalone 1/2/3 | Qualification category | Issue / owner |
| --- | --- | --- | --- | --- | --- |
| Original nine: `lamp-body:lamp_body::runtime::supervisor::tests::dropping_reaped_predecessor_does_not_remove_active_successor_sockets` | ok | ok | ok, ok, ok | 3 of 3 standalone passes (load timing qualification) | [#3315](https://github.com/autonomous-ai/lampOS/issues/3315) / Tests |
| Original nine: `lamp-expression:runtime::enabled_output_uses_a_new_session_and_expires_after_input_loss` | FAILED | ok | Not required | matched base failure | [#3315](https://github.com/autonomous-ai/lampOS/issues/3315) / Tests |
| Original nine: `lamp-expression:runtime::only_a_service_with_the_offline_face_draws_listening_unavailable_when_offline` | FAILED | ok | Not required | matched base failure | [#3315](https://github.com/autonomous-ai/lampOS/issues/3315) / Tests |
| Original nine: `lamp-expression:runtime::sleep_name::an_expired_sleeping_name_is_journaled_once_by_the_service` | FAILED | ok | Not required | matched base failure | [#1386](https://github.com/autonomous-ai/lampOS/issues/1386) / Tests |
| Original nine: `lamp-expression:runtime::without_a_pause_said_expression_asks_exactly_as_before` | FAILED | ok | Not required | matched base failure | [#3315](https://github.com/autonomous-ai/lampOS/issues/3315) / Tests |
| Original nine: `lamp-memory:it::voice::a_refused_follow_up_forget_survives_the_next_turn_note_refresh` | ok | ok | ok, ok, ok | 3 of 3 standalone passes (load timing qualification) | [#3315](https://github.com/autonomous-ai/lampOS/issues/3315) / Tests |
| Original nine: `lamp-voice:lamp_voice::coordinator::tests::second_route::a_push_mid_search_drops_its_looks_picture` | FAILED | ok | Not required | matched base failure | [#1569](https://github.com/autonomous-ai/lampOS/issues/1569) / Tests |
| Original nine: `lamp-voice:lamp_voice::privacy_bus::tests::the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second` | FAILED | FAILED | Not required | matched base failure | [#3315](https://github.com/autonomous-ai/lampOS/issues/3315) / Tests |
| Original nine: `lamp-voice:it::light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them` | FAILED | FAILED | Not required | matched base failure | [#3315](https://github.com/autonomous-ai/lampOS/issues/3315) / Tests |

[Standalone observations](standalone-observations.json) retain each actual command, working directory, bound executable hash, exit, watchdog and complete stdout/stderr. They invoke the already-censused candidate executable directly, preserving its full-run feature union. The [environment audit](standalone-env-audit.json) distinguishes fixed environment settings from uncaptured Cargo child injections; byte-for-byte complete runtime environment equality is not claimed. No case was repeated beyond the three fixed observations. The category "load timing" is the rule's qualification category, not evidence of the cause of any historical failure. [Base-failure review](base-failure-review.json) records the actual assertion/phase comparison behind each matched-base category.

The local rule is satisfied. Exact-head native ARM64 and Linux Clippy, the current base before merge and reader disposition remain separate requirements; this table does not assert them complete. Historical native runs are not relabelled.

[Equal range-diff](range-diff.json) and [source self-review](source-self-review.json) preserve authored scope and its test-helper limitations. Source-only Linux formatting finished before full tests started; [the stage coordination receipt](execution-load-hold.json) retains the timing. [Shared-host context](shared-host-context.json) describes the recorded execution conditions. Commands and environment were matched; idle-machine conditions or equal instantaneous host load are not claimed, and no failure cause is inferred. No lamp access, product change or local ARM64 build is part of this qualification.
