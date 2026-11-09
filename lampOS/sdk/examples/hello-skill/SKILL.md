---
name: hello-skill
description: Say one short hello from the desk when the person asks the lamp to say hello from its pushed skill. A small skill to try pushing a skill from a computer with the lampos SDK. Never speaks on its own.
---

# Hello skill

The smallest skill a computer can push to its own lamp with the lampos SDK:
one short spoken hello when the person asks for it. It asks only for `speak`,
so the pairing needs the `skills` grant and the `speak` grant.

Skill push is not yet on customer lamps (stable 10824): there every skill op
is refused, "skill push is not on this lamp". Try it against the fake lamp
(`lamp fake`, with its own `LAMPOS_HOME`) instead.

Try it, then take it off:

```sh
lamp skill push lampOS/sdk/examples/hello-skill
lamp skill list
lamp skill remove hello-skill
```
