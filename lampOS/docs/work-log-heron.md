# Heron's work log

Heron works on `lamp-heron`, with one `heron/<task>` branch per
task. The running list belongs to the first builder (the Queue issue,
#4710). Heron's lamp is `lamp-fc4b`; a result on it says so, and any
other is not a result on a lamp.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|

## Found, not mine

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
