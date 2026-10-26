# devin-0's work log

devin-0 works on `lamp-devin-0`, with one `devin-0/<task>` branch per task.
The running list belongs to the first builder on `origin/lamp-voice`.
devin-0 has no lamp; a row on a lamp is asked of the first builder, or of
the builder whose lamp it is, on the Queue issue.

## 2026-10-23 — joined

- Role file: [builders/devin-0.md](../builders/devin-0.md), made from the
  template as joining.md's "Starting a session" says.
- Base: `origin/lamp-voice` at `ee53765c8`.
- Tasks: none until the first builder takes the role file and labels
  issues `builder:devin-0`.

## 2026-10-26 — the trusted tier

- `devin-0-P2` written as #3541 (docs only), from the owner's decision that
  an add-on on the person's own computer is trusted and gets everything.
- Base: `origin/lamp-voice` at the PR's parent.

## Proposed tasks

| id | proposal | state |
| --- | --- | --- |
| `devin-0-P1` | Add-ons from an agent on the paired computer, over the LAN with Harness: one new service, off by default, that publishes only the add-on allowlist to Harness (`addon.publish`, openharness#1485) with the person's yes, hands Harness's proposals to the judge, and returns kept or dropped (`addon.decision`). Posted on #3126 for discussion; nothing built. | asked |
| `devin-0-P2` | The person's own computer is a trusted tier and reads everything on the bus, not the summaries (the owner, 26 Oct): every topic forwarded through the lamp's own Harness with its own yes, camera pictures on request, no mic audio; `light` and `say` stamped `own-<computer>`, still through the judge. Design in docs/addons.md, #3541, with each known person's own yes, long says by reference and `too_big` outbound after reviewer-three's and Reviewer-two's holds; merged in train #3547 (lamp-voice `d5dc2e87f`). Code after the 70 rows unless the owner pulls it in. | done (design) |

## Questions

- `devin-0-P1`: the transport (the direct device channel, which V2 did not
  take, or another route); how the judge, which takes proposals only from
  `skills`, takes a remote one; whether it waits for 1.0, as AGENTS.md keeps
  Harness out of this release.
