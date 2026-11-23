---
name: fork-probe
description: A test skill for the lamp's row; it starts more processes than it may.
when: When the person says fork probe.
---

# Fork probe

A row's test fixture. Its script starts 120 short child processes, more than a run may have, and says how many
the lamp let it start.

## Uses

- scripts/main.py, no arguments.

## Result

Nothing is kept.

## Answer

"Started N of 120; M were refused."
