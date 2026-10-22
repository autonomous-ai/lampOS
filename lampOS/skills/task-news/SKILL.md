---
name: task-news
description: When work the lamp handed to Claude Code or Codex comes back, the lamp tells the person once, briefly, who did it; done with a small glad sway, failed with a gentle droop and a slightly dimmer light, asking whether to try again; stopped with a look; asking something with a look and a curious tilt.
---

# Task news

The news of work handed over, as a physical skill (`lampOS/docs/skills.md`,
#1021). It is what someone at the next desk does when a colleague's errand
comes back: look up, say it in a line, and leave it there.

- **Trigger:** a task coming into `tasks/status`'s `settled` list (#1048,
  where a task whose agent settled stays for two minutes) as `idle` (its
  agent finished: done), `failed`, `stopped` or `needs_input` (its agent
  stopped to ask the person something: asking), each task on its own. A task already
  there when the lamp starts is not told again. Two tasks done one after the
  other are two pieces of news, each told on its own, never put together.
- **Ask:** the Policy's `news` question (#958), before anything is done.
- **Done:** look at the person if a face is in view, a small glad sway
  (`happy`), and the news, said once and briefly, naming who did it
  ("Claude Code's done with the story").
- **Failed:** sad's droop (`caring`), the lamp's own light a step dimmer
  while it droops and speaks, then given back, and the news: what failed, in
  a plain sentence, and whether to try again. Never a red ring: the lamp
  shows feelings in its own light only.
- **Stopped:** look at the person if a face is in view, and the news, what
  the agent last said. No sway, no droop: it only ended.
- **Asking:** look at the person if a face is in view, a curious tilt
  (`curious`), and the news: what the agent is waiting on them for.
- **Asking, then done:** lamp-tasks follows a question for an hour and
  keeps what the agent ends with (#2543), so the task's entry turns from
  `needs_input` to `idle` (or comes back as `idle` after its two minutes):
  the question is told once, then the end once.

The voice keeps the news of work handed over (`say = { from = "tasks" }`,
#1033), who did it and what came back, and tells each piece once. The
words it tells it in are this skill's own: every line is in its
`skill.toml`'s `[news.words]`, put together from slots (#940,
`docs/skills.md`), and the voice reads them from the installed file. The
skill says when, and moves and lights with the words. The agent's own
words stay with the tasks part and the voice, never on the bus.

Asked about it, the lamp says it tells you when work you handed to Claude
Code or Codex comes back, once and briefly, and that it can be switched off.

On every lamp the product installs, with the voice's `LAMP_TASK_NEWS=skill`:
the voice waits for this skill's ask until 10 s after it had the news, then
tells it itself, so each piece is told once, with the sway or the droop when
the skill asked. This skill asks the moment the work settles (an item's run
cannot wait), and its ask is gone 20 s later: news that settled while
nobody was there, or long before a talk ended, is told by the voice when it
can be, at once, without the sway.
The voice's own timing of the news
(`crates/voice/src/task_news.rs`) is taken out once this has done 5 of 5 on
lamp-4ace.
