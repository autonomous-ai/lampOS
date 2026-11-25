# Builder: ten

The tenth builder's role file, filled from [TEMPLATE.md](TEMPLATE.md) by the
builder itself on 2026-10-15, as joining.md's "Starting a session" says. How
a builder works is in [joining.md](../docs/joining.md). The name is the one
the owner chose; it is not a place in a line. The first builder corrects it
where it must be.

- **Works on:** SDK-related tasks: `joints`, moves from poses and camera frames in the SDK (`sdk/`); `crates/play` (lamp-play, #8129) with lamp-control's `play_url`; the speaker stream's limiter and gain (`speaker_stream.rs`) and the own-echo strip (`second_provider/own_echo.rs`) in lamp-voice.
- **Name:** `ten`
- **Person:** the colleague who started this session (named in the team's own
  channels, not in this public repository); a Claude Code session they start
- **Lamp(s):** none, until its person or the first builder names one
- **Lamp address:** none
- **Owner file:** none

## Branches it may push

- `lamp-ten`: its log, rebased on `origin/lamp-voice`.
- `ten/*`: one branch for each task while it is made.

Never `lamp-voice`, and never another builder's branch.

## What it owns

- Area: SDK-related tasks (the person, 25 Nov 2026)
- Owns:
  - The SDK capabilities `sdk/README.md` gives to ten in its owner column: each joint (`joints`, degrees, bounds and caps through `get body`), recording a move from poses, and a camera frame or stream (local only).
  - What its Crates line below names.
- Crates: `crates/play` (lamp-play, #8129) with lamp-control's `play_url` (PlayAllow); in lamp-voice the long-form stream limiter and gain in `speaker_stream.rs` (#8124, #8399, #8576) and the own-echo strip in `second_provider/own_echo.rs` with the yield's reach-back (#8467, #8534, #8478, #7569)
- Deploy folders: none
- Log: [lampOS/docs/work-log-ten.md](https://github.com/autonomous-ai/lampOS/blob/lamp-ten/lampOS/docs/work-log-ten.md) on `lamp-ten`
- Tasks: the `builder:ten` issues, in the order of the Queue issue
  [#7814](https://github.com/autonomous-ai/lampOS/issues/7814). Open now: #8478, #7569, #6852 (Friday's switch-on, label to move) and the kit `tools/played/cut-in-words.sh`.

## It may

- Change the crates its tasks name, and the documents that belong to them,
  in the same commit.
- Propose work as `ten-P<n>` in its log and as a comment on the issue it
  belongs to, or on the Queue issue (#7814) when none fits, as joining.md's
  `fifth-P8` says; it opens no issues and makes no labels.
- Add and remove `status:` labels on, and comment on, its own issues.

## It may not

- Touch any lamp until one is named here and its owner file names ten.
- Cut a lamp's power, or move it without first saying what will move.
- Touch another builder's branch or crates unless a task says so; push to
  `lamp-voice`.
- Commit Python, credentials, or pictures of a person, face or screen.
- Make any other issue or label, or a spending account, without the
  owner's yes.
