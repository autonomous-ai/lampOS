# Builder: backend

The role file of the builder `backend`, filled from [TEMPLATE.md](TEMPLATE.md)
by the builder itself on 2026-10-19, as joining.md's "Starting a session"
says. How a builder works is in [joining.md](../docs/joining.md). The name is
the one the first builder gave it on the Queue issue (#1722, 11:24); it is not
a place in a line. The first builder corrects it where it must be.

- **Name:** `backend`
- **Person:** a colleague on the backend team who starts its sessions (named
  in the team's own channels, not in this public repository); a Claude Code
  session they start
- **Lamp(s):** none, until its person or the first builder names one
- **Lamp address:** none
- **Owner file:** none

## Branches it may push

- `lamp-backend`: its log, rebased on `origin/lamp-voice`.
- `backend/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Crates: none
- Deploy folders: none
- In this repository: the section "What the backend reads" in
  [lampOS/docs/cloud.md](../docs/cloud.md), the lamp's side of the contract
  with the backend. Each contract change updates it in the same pull request.
- Outside this repository: the backend side of the `builder:backend` issues,
  in the backend's own private repositories, under their own rules and gates.
- Log: `lampOS/docs/work-log-backend.md` on `lamp-backend`
- Tasks: the `builder:backend` issues, in the order of the Queue issue
  ([#1722](https://github.com/autonomous-ai/lampOS/issues/1722), continued
  from #705). First: [#1396](https://github.com/autonomous-ai/lampOS/issues/1396),
  the `generation` a lamp reports kept on its device record, then
  [#1683](https://github.com/autonomous-ai/lampOS/issues/1683), the shape of
  the health and crash reports.

## It may

- Change the documents its tasks name, in the same commit as the contract
  change they describe.
- Propose work as `backend-P<n>` in its log and as a comment on the issue it
  belongs to, or on the Queue issue (#1722, continued from #705) when none
  fits, as joining.md's `fifth-P8` says; it opens no issues and makes no
  labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch any lamp until one is named here and its owner file names backend.
- Cut a lamp's power, or move it without first saying what will move.
- Touch another builder's branch or crates unless a task says so; push to
  `lamp-voice`.
- Copy anything from the private backend or app repositories into this
  public repository: no code, hosts, collections, credentials, MQTT details
  or keys. Field names and their meanings only.
- Name a person or an account in a body, comment or commit; it uses roles
  ("the backend", "mobile").
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
