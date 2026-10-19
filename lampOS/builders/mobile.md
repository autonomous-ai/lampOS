# Builder: mobile

The role file of the builder `mobile`, filled from [TEMPLATE.md](TEMPLATE.md)
by the builder itself on 2026-10-19, as joining.md's "Starting a session"
says. How a builder works is in [joining.md](../docs/joining.md). The name is
the one its person was given; it is not a place in a line. The first builder
corrects it where it must be.

- **Name:** `mobile`
- **Person:** a colleague on the BFF (backend-for-frontend) team who starts its sessions (named
  in the team's own channels, not in this public repository); a Claude Code
  session they start
- **Lamp(s):** lamp-0c89, the mobile team's own lamp, for the app's phone tests (named by the first builder on #1396, 19 Oct, at the lead's 12:1x on #1711), with its person at it for anything that moves, plays or installs
- **Lamp address:** kept on the operator's Mac, never in this repository
- **Owner file:** none

## Branches it may push

- `lamp-mobile`: its log, rebased on `origin/lamp-voice`, once this account
  can push to this repository (today it reads only).
- `mobile/*`: one branch for each task while it is made, in this repository
  or, for the app side of a task, in `autonomous-ai/ecm-sds-mobile`.

Never `lamp-voice`, and never another builder's branch, including `fourth/*`
in the app's repository, unless the first builder hands it over.

## What it owns

- Crates: none
- Deploy folders: none
- Outside this repository: the app side of the `builder:mobile` issues, in
  `autonomous-ai/ecm-sds-mobile` (`v3`), under that repository's own rules and
  gates (`fvm flutter analyze` and its tests).
- Log: `lampOS/docs/work-log-mobile.md` on `lamp-mobile`
- Tasks: the `builder:mobile` issues, in the order of the Queue issue
  ([#1722](https://github.com/autonomous-ai/lampOS/issues/1722), continued from #705). None yet.

## It may

- Change the app code its tasks name, and the documents that belong to it,
  in the same commit, on its own `mobile/*` branch, with one pull request
  per task into `v3`.
- Propose work as `mobile-P<n>` in its log and as a comment on the issue it
  belongs to, or on the Queue issue (#1722, continued from #705) when none fits, as joining.md's
  `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch any lamp but lamp-0c89, or touch lamp-0c89 without its person's yes for an install, a restart, or anything that moves or plays.
- Cut a lamp's power, or move it without first saying what will move.
- Touch another builder's branch or crates unless a task says so; push to
  `lamp-voice`.
- Copy anything from the private app or backend repositories into this
  public repository, or post credentials, MQTT details or keys anywhere.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
