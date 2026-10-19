---
name: task-news
description: When work the lamp handed to Claude Code or Codex comes back, the lamp tells the person once, briefly, who did it; done with a small glad sway, failed with a gentle droop and a slightly dimmer light, asking whether to try again.
---

# Task news

The news of work handed over, as a physical skill (`lampOS/docs/skills.md`,
#1021). It is what someone at the next desk does when a colleague's errand
comes back: look up, say it in a line, and leave it there.

- **Trigger:** a task coming into `tasks/status`'s `settled` list (#1048,
  where a task whose agent settled stays for two minutes) as `idle` (its
  agent finished: done) or `failed`, each task on its own. A task already
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

The words are the voice's (`say = { from = "tasks" }`, #1033): it keeps the
news of work handed over, who did it and what came back, and tells each
piece once. The skill says when, and moves and lights with the words. The
agent's own words stay with the tasks part and the voice, never on the bus.

Asked about it, the lamp says it tells you when work you handed to Claude
Code or Codex comes back, once and briefly, and that it can be switched off.

On every lamp the product installs, with the voice's `LAMP_TASK_NEWS=skill`:
the voice waits up to 10 s for this skill's ask, then tells the news itself,
so each piece is told once, with the sway or the droop when the skill asked.
News of work that stopped, or that asks something, has no trigger here: the
voice tells it after those 10 s. The voice's own timing of the news
(`crates/voice/src/task_news.rs`) is taken out once this has done 5 of 5 on
lamp-4ace.
