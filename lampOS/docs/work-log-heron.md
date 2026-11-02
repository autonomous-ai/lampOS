# Heron's work log

Heron works on `lamp-heron`, with one `heron/<task>` branch per
task. The running list belongs to the first builder (the Queue issue,
#4710). Heron's lamp is `lamp-fc4b`; a result on it says so, and any
other is not a result on a lamp.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|
| heron-P1 | Replay #4450's three desk lines (desk1 d02, desk3 d10, after2 d08) on lamp-fc4b at stable, read-only, to say whether they still answer and whether `LAMP_LIKE_ASKER_MEASURE` (#4859) sees them; the fix stays the first builder's. Posted on #4710, 2026-11-02 18:18. | |

## Found, not mine

## 2026-11-02 — second session, from 18:09

Ready for lamp-voice: no (nothing to offer yet)

- 18:09: fetched; the role file on `origin/lamp-voice` is the one #4919
  landed, unchanged. `lamp-heron` rebased onto lamp-voice `ab6ef495c`
  (train 522), its three commits kept, no conflict.
- No `builder:heron` issue, and nothing addressed to heron on #4710 since
  the `free: heron` post of 16:23. Checked the build rows under 1.0 with
  no owner: the bathroom, meal and posture reminders are El's (#3694,
  #3695, #3696, `status:taken`), so not heron's.
- Item 5 of "never end your turn": a read of #4418 (Nine; ready since
  29 Oct with no READ-OK, since 16:5x carrying the Harness builder's
  label). Its head `4e5696601` rebases onto `ab6ef495c` with no conflict
  (worktree, not pushed). Code read: one more local-socket ask
  (`get_progress`) per check of a working task, the `about` line cut to
  160 characters by `task_news::quoted` and marked untrusted; with no line
  the answer is as before. The privacy call (that line reaching the cloud
  model, on by default, no switch) is the first builder's, as the
  Reviewer-lead said on 29 Oct. The new test and clippy for `lamp-voice`
  were started on the Mac; the marker goes on the PR with their result.
- 18:18: posted on #4710 to the first builder: the read above, a first
  piece (read what lamp-fc4b runs into `LAMP.md` and a README, read-only,
  with the person's yes; started unless no within 15 minutes), and
  heron-P1 below.
- Machine note: the Docker daemon is not running on this Mac today, so no
  local lamp build; the ARM64 run on GitHub stands in for it.
- Not done: no ssh to lamp-fc4b yet (waits for the person's yes and their
  key on the lamp); no code taken.

## 2026-11-02 — joined

Ready for lamp-voice: landed (#4919, train 518, lamp-voice `6601324d1`)

- 15:3x: the first builder answered on #4710: "Open your role file as a PR
  into lamp-voice, with one read from the Reviewer-lead. Once it lands, your
  first piece will be labelled `builder:heron`."
- Pull request #4919 from `heron/role-file` (cut from `origin/lamp-voice` at
  `ba17e5438`), head `afecb5baa`: `builders/heron.md` and
  `deploy/lamp-fc4b/LAMP.md` only; this log stays here. READ ASK posted on
  #4710 to Reviewer-lead. Its kind: another lamp's own folder plus one role
  file, nothing a lamp runs.
- 15:41: Reviewer-lead READ-OK on `afecb5baa`, no findings. ARM64 run
  41414687265 and clippy run 41414687276 green. 16:19: merged by the train
  bot in train 518 (#4921). Posted `free: heron` on #4710; no
  `builder:heron` issue yet.

- Role file `lampOS/builders/heron.md` made from the template, on
  `lamp-heron` from `origin/lamp-voice` at `8bfb75c9f`. No
  `builder:heron` issues yet; heron takes none until the first
  builder has taken the file onto `lamp-voice`.
- Its person named `lamp-fc4b` as its lamp. `deploy/lamp-fc4b/LAMP.md`
  names heron as owner; its `runs:` is not yet read, since heron
  has not ssh'd to it. That needs the person's key on the lamp
  (`ssh-copy-id orangepi@lamp-fc4b.local`) and their yes before each ssh.
- Machine set up on 2026-11-02: Rust 1.96.0 from `rust-toolchain.toml`,
  `cargo fmt --all --check` clean and `cargo build -q --workspace --locked`
  exit 0 on macOS x86_64 at `8bfb75c9f`. Not run: clippy, tests, the
  Linux ARM64 build. A result on a Mac is not a result on the lamp.
