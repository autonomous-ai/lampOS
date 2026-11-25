---
name: morning-headline
description: Every morning at eight, the lamp looks up the top story on Hacker News and tells me when I'm there.
made: here
---

# Morning headline

At eight each morning a helper script fetches the top story from a public
web service and prints it; the lamp's agent sums it up, and the lamp tells
the owner once they are alone at the lamp and it has been quiet a minute.

## Uses

- scripts/main.py, no arguments: a public web service through the run's proxy.

## Result

scripts/main.py prints the top story's title, or why the web couldn't be reached.

## Answer

The agent's final answer is only the spoken summary: the top story in one plain sentence, or that it couldn't be reached just now.
