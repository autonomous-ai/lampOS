---
name: prepare-unplug
description: When the person explicitly says they will unplug or turn off the lamp, call prepare_to_unplug. It requests a guarded fold; the initial answer is not permission to unplug. Tell them to wait, and never claim readiness yourself. The skill reports readiness separately only after fresh body and asleep-presentation confirmation. It does not shut down the operating system. Do not call for ordinary bedtime, a pause in conversation, or a quoted/example unplug sentence.
---

# Prepare to unplug

This core skill runs only when asked. With `LAMP_UNPLUG=on` on Skills, Body,
Expression and Voice, it requests the body's existing guarded fold. It never writes
servo goals, disables protections, or cuts power.

The tool acknowledges preparation immediately. The body must prove that the
folded joints are physically settled, and Expression must publish the asleep
presentation, before a separate short voice confirmation is requested. Missing,
stale, mismatched or failed evidence never authorizes that confirmation.

Preparation inhibits movement until reboot, including name calls, gestures and
the readiness speech itself. A failed or interrupted attempt does not resume
motors. If the body cannot confirm the fold, keep power connected and have the
operator inspect it. This is not an operating-system shutdown command.
