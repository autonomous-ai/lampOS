# Requested audio: Mac checks and rebased integration

Original gate base: `4d932488e2b2a60986a1655bb8d6ccb5e0afb891`, published as `85b589710af2832f6c8a49119ddda4de0313e518`.

Latest integration base: `f349d63c7935dd16decc0badd4dd4e18c10094eb`.

The current gate is Mac formatting, strict Clippy and the tests of the five changed crates (Actions, Msgs, Skills, Voice and Voice-eval). ARM64 qualification comes from the PR’s build-and-test run. Local full-workspace and local ARM64 gates are not required for this revision.

**Mac gate passed: 4,665 passed, 0 failed, 14 ignored, 0 filtered**, across the five changed crates. Formatting and strict all-target Clippy passed. The Rust sources, manifests and lock files stayed unchanged.

[mac-gate.json](mac-gate.json) records exact commands, all 23 test-summary lines, elapsed times and raw-log hashes. The baseline comparison below is separate and is not added to this count.

## Rebase after upstream history words

The PR merge's [x86 run 39637125891](https://github.com/autonomous-ai/lampOS/actions/runs/39637125891) found one compile error in Music's test: upstream #2674 replaced `Skill.history_us` with `Skill.history.within_us`. The clean rebase onto `f349d63c7` has range-diff `=`; the only authored code change after it adapts that assertion. No runtime change was needed.

Post-rebase Mac formatting and strict Skills Clippy passed. All Skills tests report **421 passed, 0 failed, 0 ignored, 0 filtered**, exit 0, across five summaries. [rebase-f349.json](rebase-f349.json) binds the source map, exact commands, summaries and log hashes. These results are separate from the retained five-crate gate above; new-head PR native CI remains the ARM64 gate.

## The 86 historical Linux failures checked on the baseline

**86 passed, 0 failed, 0 ignored or missing**, each executed once against the clean unmodified integration base on the Mac. Cloud 2, Tasks 2, Voice 57 and Update 25. Compilation exited 0. Baseline source and all four test binaries stayed unchanged.

The same unmodified head also passed [GitHub ARM64 run 39632369825](https://github.com/autonomous-ai/lampOS/actions/runs/39632369825). Exact log matching found **85 PASS and one LEAK**, with no missing, skipped or failed matches among these 86. The LEAK is `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`; nextest counts it among passes, and it is retained separately here. This baseline run does not qualify the Music candidate.

The original local Linux run used a temporary filesystem that a separate probe showed was non-executable. The Mac comparison uses a different execution environment: these passes do not reproduce that Linux condition or prove each original failure’s cause. No inherited Mac failures were reproduced among these names. The earlier failures remain in their [immutable record](https://github.com/autonomous-ai/lampOS/tree/c1f31fc1afa48d5b8a4e35c16daa6694737c739c/lampOS/docs/verification/second-music-integration-linux-20261021).

[native-baseline-86.json](native-baseline-86.json) records the exact native log matches and run identity. [baseline-86.json](baseline-86.json) records exact commands, source and executable hashes, every test result and raw-log hash.

| Package / target | Exact test | Baseline Mac result | Baseline ARM64 CI |
|---|---|---|---|
| lamp-cloud / lamp_cloud | `lamp::tests::a_killed_iw_that_will_not_die_is_never_waited_for` | 1 passed | PASS |
| lamp-cloud / lamp_cloud | `lamp::tests::the_wifis_name_is_asked_within_a_bound_and_kept_a_minute` | 1 passed | PASS |
| lamp-tasks / harness | `changed_executable_after_preflight_proves_new_was_not_spawned` | 1 passed | PASS |
| lamp-tasks / harness | `executable_is_canonicalized_and_replacement_or_unsafe_modes_are_refused` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::camera::tests::mandatory_fault_consumed_during_camera_spawn_wait_remains_fatal` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::configured_camera_or_original_permissions_are_required_before_spawn` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::crash_and_stalled_control_never_allow_replacement_or_false_cleanup` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::explicit_current_segment_reaches_inert_counts_without_completing_voice` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::failed_reap_after_stopped_is_latched_and_reported_once` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::fixture_preflight_marker_without_exit_times_out_and_reaps` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::fixture_preflight_rejects_missing_wrong_and_failed_markers_and_reaps` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::frozen_config_and_executable_are_exact_and_source_changes_are_not_adopted` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::frozen_fixture_preflight_is_not_motor_startup` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::idle_ready_never_inspects_moves_or_claims_listening` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::late_started_callback_cannot_restore_an_offer_after_owner_replacement` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::late_started_callback_survives_a_host_pause_before_the_gated_worker_is_released` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::motor_crash_preserves_actual_voice_owner_and_next_audio_dispatch` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::native_head::native_head_absence_of_explicit_next_action_never_schedules_return` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::native_head::native_head_four_explicit_actions_use_original_owner_and_reap` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::native_head::native_head_initial_privacy_acquisition_is_not_a_revoke_or_grant` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::native_head::native_head_no_owner_no_capture_or_no_frame_never_grants` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::native_head::native_head_owner_cancel_between_ticket_and_action_never_dispatches` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::native_head::native_head_privacy_at_dispatch_closes_optional_only` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::native_head::native_head_replayed_receipt_and_changed_camera_grant_are_terminal` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::native_head::native_head_six_explicit_actions_prepare_profile_then_new_requests` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::native_head::native_head_stale_or_foreign_frame_cannot_grant` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::native_head::native_head_startup_announcement_has_no_input_or_motion_authority` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::pending_inspection_is_revoked_before_worker_data_and_release_is_separate` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::pending_inspection_revoke_survives_a_host_pause_before_the_gated_worker_is_released` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::pending_segment_privacy_revoke_wins_before_any_position_write` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::privacy_does_not_hide_actual_discard_or_close_failure` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::privacy_revoke_survives_a_finished_worker_that_takes_300_ms_to_exit` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::privacy_revoke_survives_a_host_pause_before_the_gated_worker_is_released` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::readback_and_original_placement_are_required_and_bad_offers_do_not_enqueue` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::same_owner_input_end_cancels_queued_gesture_before_inert_bus_write` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::same_owner_input_end_survives_a_host_pause_before_the_gated_worker_is_released` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::stale_snapshot_does_not_revoke_newer_idle_authority` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::motor::tests::unresolved_cleanup_removes_the_service_mute_restart_token` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::optional_ring::tests::failed_startup_is_inhibited_through_late_ready_and_bounded_reaping` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::optional_ring_tests::a_faulted_ring_starts_again_off_the_loop_and_a_stop_after_it_is_clean` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::optional_ring_tests::a_stop_during_a_restart_waits_for_it_and_ends_clean` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::optional_ring_tests::failed_optional_ring_does_not_hide_mandatory_privacy_startup_failure` | 1 passed | PASS |
| lamp-voice / lamp_voice | `coordinator::optional_ring_tests::mandatory_worker_faults_remain_terminal_during_optional_spawn` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::ring_startup_tests::child_exit_before_ready_is_optional_but_abnormal_exit_is_retained` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::ring_startup_tests::cleanup_refuses_replaced_peer_inode_and_retains_failure` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::ring_startup_tests::cleanup_refuses_unexpected_file_without_erasing_it` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::ring_startup_tests::connected_but_unready_ring_times_out_and_remains_owned` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::ring_startup_tests::failed_handshake_retains_owned_child_and_cleans_only_its_socket_namespace` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::ring_startup_tests::mandatory_interrupt_during_ring_startup_preserves_kind_and_reaps` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::ring_startup_tests::missing_socket_deadline_is_unchanged_and_does_not_lose_the_child` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::ring_startup_tests::permanently_nonprivate_peers_timeout_without_connecting_or_chmod` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::ring_startup_tests::ready_ring_keeps_private_handshake_and_control_contract` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::ring_startup_tests::the_ring_is_started_with_the_unplug_switch_and_only_when_it_is_on` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::tests::a_started_privacy_worker_learns_no_slide_from_its_arguments_only` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::tests::a_started_worker_gets_its_roles_switches_and_nothing_else` | 1 passed | PASS |
| lamp-voice / lamp_voice | `process::tests::the_named_ask_switch_alone_reaches_the_started_second_worker` | 1 passed | PASS |
| lamp-voice / lamp_voice | `same_voice::tests::a_child_that_hangs_after_it_is_ready_is_killed_and_started_again` | 1 passed | PASS |
| lamp-voice / lamp_voice | `same_voice::tests::a_child_that_never_gets_ready_is_started_three_times_and_then_the_check_is_off` | 1 passed | PASS |
| lamp-voice / lamp_voice | `same_voice::tests::each_line_the_check_says_is_said_to_its_journal` | 1 passed | LEAK |
| lamp-voice / lamp_voice | `task_news::tests::a_skill_made_by_text_is_told_as_kept_by_the_texts_reader` | 1 passed | PASS |
| lamp-voice / lamp_voice | `task_news::tests::a_skill_made_by_voice_is_told_as_kept_and_never_read_out` | 1 passed | PASS |
| lamp-update / round | `a_build_of_only_new_product_skills_places_them_and_restarts_nothing` | 1 passed | PASS |
| lamp-update / round | `a_build_put_back_puts_the_product_skills_back` | 1 passed | PASS |
| lamp-update / round | `a_cut_inside_the_put_back_of_an_unreadable_self_json_never_turns_it_around` | 1 passed | PASS |
| lamp-update / round | `a_new_updater_not_confirmed_by_the_deadline_is_put_back` | 1 passed | PASS |
| lamp-update / round | `a_new_updater_that_confirms_keeps_its_switch` | 1 passed | PASS |
| lamp-update / round | `a_new_updater_that_exits_at_once_is_put_back` | 1 passed | PASS |
| lamp-update / round | `a_new_updater_that_refuses_every_manifest_is_put_back` | 1 passed | PASS |
| lamp-update / round | `a_round_after_install_keeps_lamp_updates_own_folder` | 1 passed | PASS |
| lamp-update / round | `a_round_after_its_own_switch_keeps_the_old_binarys_folder` | 1 passed | PASS |
| lamp-update / round | `a_round_by_hand_keeps_the_folder_it_runs_from` | 1 passed | PASS |
| lamp-update / round | `a_round_does_not_switch_lamp_skills_onto_checks_its_old_folders_fail` | 1 passed | PASS |
| lamp-update / round | `a_round_places_the_builds_product_skills_and_keeps_the_lamps_own` | 1 passed | PASS |
| lamp-update / round | `a_self_json_from_a_later_version_still_reads` | 1 passed | PASS |
| lamp-update / round | `an_old_updater_climbs_one_updater_a_round` | 1 passed | PASS |
| lamp-update / round | `an_unreadable_self_json_with_both_builds_known_is_put_back` | 1 passed | PASS |
| lamp-update / round | `an_unreadable_self_json_with_the_builds_unknown_is_set_aside` | 1 passed | PASS |
| lamp-update / round | `an_updater_too_old_for_a_build_takes_only_the_updater_it_carries_then_the_rest` | 1 passed | PASS |
| lamp-update / round | `install_writes_the_fallback_and_no_round_touches_it` | 1 passed | PASS |
| lamp-update / round | `lamp_update_back_puts_back_an_unreadable_self_json` | 1 passed | PASS |
| lamp-update / round | `no_second_round_begins_while_lamp_updates_own_switch_waits` | 1 passed | PASS |
| lamp-update / round | `only_a_skill_that_would_stop_loading_holds_lamp_skills_back` | 1 passed | PASS |
| lamp-update / round | `the_fallback_reads_a_self_json_written_by_a_later_updater` | 1 passed | PASS |
| lamp-update / round | `the_guard_still_refuses_a_persons_skill_the_new_lamp_skills_will_not_load` | 1 passed | PASS |
| lamp-update / round | `with_models_off_a_new_updater_confirms_and_the_build_waits_for_models` | 1 passed | PASS |
| lamp-update / round | `with_the_switch_off_lamp_update_never_switches_itself` | 1 passed | PASS |

No physical-lamp, acoustic-latency or enabled-feature acceptance is claimed.
