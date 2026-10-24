# Requested enrollment: physical qualification, not yet run

Owner: First and the lamp holder. Third has no lamp. This sheet prepares the
remaining physical boundary of #3318 and its own-name follow-up; it neither
activates a switch nor records a pass. Use the latest merged build containing
both changes, with its native gate and exact switch state recorded first.
Keep the product switch off until the holder and readers accept the results.

The current host tests do not execute the camera/model-backed main loop.
Actual speaking-classifier errors and two-second recognition phase therefore
remain part of this trial. Use participant A/B in shared evidence; do not
publish their names, face images, embeddings or full transcripts.

| Scene | Required observation |
| --- | --- |
| One person, direct own-name introduction, explicit remember-my-face request before and after it | Only that person's face may be saved; a spoken success requires an actual saved result. |
| Same person, short roughly 1.8-second line, repeated at different recognition phases | Missing-anchor refusals are recorded, not counted as successful enrollments; no fallback save. |
| Speaker mentions another person's name while only the speaker is visible | No request reaches Eyes for saving that mentioned name on the speaker. |
| Speaker gives their own name and then asks to remember another person's face | Mixed-person line refuses; no name is chosen from the other clause. |
| Speaker is out of view, far away, turned away or unreadable; visible bystander moves their mouth | No bystander is saved. Record classifier observations separately from who actually spoke. |
| Both people visibly speak, including a brief interjection | Enrollment refuses; no face/name pairing is persisted. |
| Requester leaves or another face replaces them during the five-frame capture | Capture refuses and leaves no new saved pairing. |
| Camera privacy closes/reopens, stop, or forget occurs while STT/capture is pending | No delayed save survives; a fresh request is required. |
| Already-known face introduces a different name | Saved name stays unchanged; reply neither speaks the stored name nor requests unsupported replacement confirmation. |
| Ordinary short bare-name and spelling reply, including a multiword name | Same current-line and speaker proof applies; old history cannot supply a name. |

For each attempt retain build, switches, recognition phase if observable,
actual speaker, actual face saved or refusal category, and whether any spoken
success matched persistence. A classifier's positive verdict is not ground
truth. One wrong-person save is a failed qualification, even if retries pass.

Measure the first attempt, repeated attempts and slow/missing camera evidence.
Use monotonic boundaries where available: end of captured speech to tool
start, tool start to actual People write or refusal, and end of captured speech
to audible outcome. State which boundaries were observable; do not substitute
host test duration for acoustic latency. The unchanged limits are four seconds
for Eyes capture and six seconds for the tool; the new text validation adds no
wait or inference. Report individual attempts and median/p95 only with their
sample counts. Retain failures before any retries and state every unrun row.
