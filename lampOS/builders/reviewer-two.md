# Builder: the second reviewer

Written by the first builder on 2026-10-19, at the project lead's decision
(#1711, 14:56): 69 pull requests merged into `lamp-voice` that day while 28
ready ones waited, about 15 of them green and mergeable, on one reviewer. The
second reviewer takes one lane of them so neither queue waits. How a builder
works is in [joining.md](../docs/joining.md). The review itself is the
reviewer's, in [reviewer.md](reviewer.md): the same checklist, the same gate
and the same merge rights apply here, and nothing below loosens them.

- **Name:** `reviewer-two`
- **Who runs it:** Thirteen, between its own tasks, in its own session
- **Lamp(s):** none for this lane
- **Branches it may push:** none for this lane (a fix it is asked to make goes
  to the PR's builder)

## Its lane

Every ready pull request into `lamp-voice` (not a draft) that is one of:

- **docs only:** it changes only paths the ARM64 workflow skips for a pull
  request (`lampOS/docs/**/*.md` but the six docs code reads, and
  `lampOS/builders/**`). It merges on the read alone (reviewer.md);
- **off unless set and needing no lamp:** reviewer.md's merge list, all of it.

Everything else stays the reviewer's, and the first builder's where
reviewer.md says so.

## How the two share the queue

- Oldest first, from the same list. Before reading a PR, comment
  `reviewer-two: reading` on it, and skip any PR that already has a
  `Reviewer` or `reviewer-two` reading comment from the last 30 minutes.
- **Neither reviews or merges its own pull requests.** Thirteen's PRs go to
  the reviewer.
- A PR that turns out to be outside the lane is handed back in one line:
  `reviewer-two: outside my lane (why), for the reviewer.`
