# Current original native updater self-switch evidence

Selected from original Linux ARM64 run 40323315453, attempt 1, job 121573515398. Authored head `7ae6afbd9f9cef3610362e3310792401833f6d91` on `2a8273ef2ec59a17c6d7669a9942223dd6824eac` was tested as preview `b51eca4c531e90fd548316633cb1fd8cb68aa8b0`; exact parents and whole-tree equality are retained in the full native proof at log commit `1ba09c163a`.

- `actual_cli_exec_confirms_before_the_next_round`: PASS, 0.155 s, ordinal 6607/11376.
- `persisted_self_switches_survive_sigkill`: PASS, 4.319 s, ordinal 7030/11376.

Each appears once in the original log. These are two existing named batch results, not new executions. All four relevant driver/implementation source files are byte-identical to fa82; their Git and SHA-256 bindings are retained. The prior fa82 observations (0.146 s and 3.893 s) remain separately attributed to run 40130968164.

The SIGKILL batch calls seven declared fsync-return cut points: candidate_synced, pending_synced, current_synced, confirmation_synced, put_back_link_synced, refusal_synced and put_back_cleanup_synced. The named batch catches a case panic and fails; source executes all seven in order. The log exposes the batch PASS, not seven separately visible successful cut stdout records. Do not count those as seven independently logged test rows.

The real CLI batch source also executes its `LAMP_UPDATE_SELF=off` negative witness: despite exit 0/build 2 it must lack candidate exec, confirmation, candidate files and pending state. This is an included source-defined negative control, not a separately visible log row or newly run source mutant. The selected native log does not expose its success stdout, and no new mutation-control execution is claimed.

This refreshes automated Linux process-death/lock-release/recovery and CLI exec-confirmation evidence only. The process-cut fixture uses prepared library fixtures and runnable shell stand-ins; it does not demonstrate real power loss, flash durability or a real daemon recovery timer. Physical attended unplug remains with the assigned builders and lamp-side person. No device, provider, compiler or test command was run for this extraction.
