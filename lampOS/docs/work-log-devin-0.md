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

## 26 Oct, after P2: Harness's limits and the Harness half

- #3550 (docs only): records that Harness's `addon.proposal` note is 300 bytes too, with rates of 6 a minute per add-on, 12 per device and 4 undecided, and one value per topic. READ-OK from reviewer-three, Second and Third; merged in train #3551 (lamp-voice `9e5d3b9c6`). Questions 5–7 (event stream, long notes, trusted machine) asked on openharness#1654.
- openharness#1717, at the owner's request, answers #1654 questions 1–3:
  - the lamp's own service publishes, decides and takes its proposals over its daemon's loopback (`machine-addon/hello|publish|decision|proposals`);
  - the owner's machines use a sealed `machine_addon` request, `addon … --machine <lamp>` on the CLI;
  - each proposal carries `from: {machineId, label}`, taken from the link's pairing record.

  The owner is testing it before merge.


## 26 Oct, 15:41: devin-0-P3 taken, the lamp side of openharness#1717

Ready for lamp-voice: no (draft #3563, ARM64 run pending, not run on a lamp)

- Taken on #3504 at 15:41, after 15:13's proposal stood 15 minutes with no "no". The owner pulled it into the release (Dee on Slack, 26 Oct).
- Kind: off unless set (`LAMP_MACHINE_ADDONS=on`). The judge's one change, `runtime::writer_may`, takes `machine_addons` only under `own-` names, and refuses `own-`/`remote-` names from `skills`.
- Built on `devin-0/machine-addon` from lamp-voice `1cad670df`, commit `6a28a5482`. Gate on the Mac: fmt ok, clippy `-D warnings` ok, `cargo test -p lamp-addons` 47 + 12 + 4 passed, 0 failed, workspace build ok, switch census tests pass.
- Not shown: a run on a lamp. lamp-a0ae (devin-local-tester's lamp, lent by the owner in this session, taken under `lamp-lock.sh`) has no Harness, and its `sudo` asks for a password, so the owner runs the setup by hand.
- Found while doing it: the owner's Slack add-on (`daniel-auto/lamp-devin-add-on`) proposes `why` `messages`/`news`, which the lamp's bus check refuses. Adding them is a `crates/msgs` change, not mine; asked on #3504.
