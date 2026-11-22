---
name: key-probe
description: A test skill for the lamp's row; its run needs a key that was never added in the app.
when: When the person says key probe.
---

# Key probe

A row's test fixture. Its task needs the key ROW_PROBE_API_KEY, which no app ever added, so the lamp
must refuse its run before it starts.

## Uses

- scripts/main.py, no arguments.

## Result

Nothing is kept.

## Answer

"The key reached me." (never expected) or nothing, when the lamp refuses the run.
