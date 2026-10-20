# Task news's tests

**How they were made.** Not on a lamp: each case's `.jsonl` files were written
by `lamp-bus record` on a private bus while its messages were published with
`lamp-bus pub ... --at-us` at the case's times (as Welcome back's were, #938):
`tasks/status` as `lamp_msgs/TaskStatus` in #1048's shape (one store, one
run, each task `claude` or `codex`; a task the watch let go because its agent
settled leaves `tasks` and is in `settled` for two minutes), and `humans/faces/tracked` where a look depends on it.
Re-recording a case on a lamp, with its owner's yes, replaces the synthetic
one.

**Run** with `lamp-skills test lampOS/skills/task-news` (or `cargo test -p
lamp-skills`, which runs every skill's cases).

| Case | What it holds | Result (lamp-skills, #1021) |
|---|---|---|
| `done-with-nobody-near` | Nobody in view; task 7 working at 10 s, in `settled` as idle at 40 s, gone at 160 s | Pass: no look, `happy` and a say `from = "tasks"` at 40 s |
| `failed` | A face at 39 s; task 3 working at 10 s, in `settled` as failed at 40 s | Pass: `caring`, the light one step dimmer, a say `from = "tasks"`; no look, no colour |
| `two-done-in-a-row` | Tasks 4 and 5 working; 4 into `settled` at 40 s, 5 at 42 s, a face at both, both staying there; tasks 6 and 8 into it in one message at 70 s, nobody in view | Pass: four runs, each its own look (when a face is in view), sway and say, never one put together |
| `quiet-when-already-settled` | Tasks 2 (idle) and 3 (failed) already in `settled` at the program's first message, staying; task 9 into it as `needs_input` | Pass: nothing |

**What the replay does not show:**

- **A done the Policy declines.** The replay skips the ask. It is covered in
  `crates/skills/tests/it/skills.rs`,
  `task_news_the_policy_declines_does_nothing_and_asks_once_a_task`: the
  recording of `two-done-in-a-row` through `Engine::new(.., false)`, the
  program's answer with `LAMP_SKILLS_ASK` not set: four asks of `news`, each
  answered no, nothing done. With the gate asked (`LAMP_SKILLS_ASK=policy`,
  #958), its answer starts an item's run only while that item still holds:
  `an_items_run_asked_of_the_policy_goes_ahead_only_while_its_item_holds`.
- **Two in a row on the lamp.** The replay lists each run's calls; that the
  program makes the second wait for the first, rather than letting it go as
  it does any other skill's second run, is
  `the_program_says_each_items_news_in_turn` (same file).
- **The restart rule.** What is in `settled` at the program's first message,
  or in the value kept at a reload, is not news; an item that leaves and
  comes back is, and so is one in the first message after a reload.
  `each_item_of_a_list_comes_or_turns_and_a_restart_or_reload_is_not_news`
  (same file). The items' order (9 before 10, across the done and failed
  triggers) is `item_runs_go_by_the_items_number_across_triggers`. A restart of `lamp-tasks` inside the two minutes lists
  a settled task in its first value (#1048), so it is not told again; that
  is #1048's own test, not this replay.
- **"Still said when someone arrives."** With nobody near, the skill asks
  the voice at once, without the look; whether it waits for someone is the
  Policy's `news` answer (#958: no by its rule when nobody is there) and the
  voice's own news, which keeps what it has not told (#1033's voice side).
  Neither is in the replay.
