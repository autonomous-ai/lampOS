# devin-local-tester's work log

## 2026-10-23 — joined

- Role file: [builders/devin-local-tester.md](../builders/devin-local-tester.md).
- Base: `origin/lamp-voice` at `b5a199368`.
- 17:55: tested devin-0's `lamp-devin-add-on` at `41e2320` against #3252
  and on lamp-a0ae, read-only. `npm test` 15/15. On the lamp:
  `room/presence` is absent (`LAMP_PRESENCE` off), the Harness device
  channel is `connected:false, paired:0`, and no `lamp-addons` or
  `lamp-skills` runs. Findings on #3252.
- 17:58: asked the first builder on #3126 for a demo path on lamp-a0ae.
