---
name: prepare-unplug
description: When the person explicitly says they will unplug or turn off the lamp, call prepare_to_unplug. It requests a guarded fold; the initial answer is not permission to unplug. Tell them to wait, and never claim readiness yourself. The skill reports readiness separately only after fresh body and asleep-presentation confirmation. On a product lamp it then asks whether to turn itself off, and turns off only on the person's spoken yes; say nothing for it. Do not call for ordinary bedtime, a pause in conversation, or a quoted/example unplug sentence.
---

# Prepare to unplug

This core skill runs only when asked. With `LAMP_UNPLUG=on` on Skills, Body,
Expression and Voice, it requests the body's existing guarded fold. It never writes
servo goals, disables protections, or cuts power.

The tool acknowledges preparation immediately. The body must prove that the
folded joints are physically settled, and Expression must publish the asleep
presentation, before a separate short voice confirmation is requested. Missing,
stale, mismatched or failed evidence never authorizes that confirmation.

Preparation inhibits movement, including name calls, gestures and the
readiness speech itself, until reboot or a "never mind": the reserved
`stay_plugged_in` tool, refused unless preparation is latched, or 10
unbroken minutes Ready (`LAMP_UNPLUG_STAY_MIN`). The skill says it will stay before the
body, after its own checks, stands again. A failed or interrupted attempt does
not resume motors. If the body cannot confirm the fold, keep power connected and have the
operator inspect it. On a product lamp (`LAMP_UNPLUG_OFF_SOCKET` set), a confirmed fold asks the person whether to turn off, and only their spoken yes turns the lamp off through setup's shutdown; with no yes it stays and stands up. Without that socket it prepares the arm only and never shuts the operating system down.
