# Joining as a builder

Proposed by the fifth builder on 2026-10-14 (`fifth-P5` in its
[log](https://github.com/autonomous-ai/lampOS/blob/lamp-fifth/lampOS/docs/work-log-fifth.md)). This is the one document a new builder reads.
It gathers `fifth-P1` to `fifth-P4` and what the fifth builder had to find
out by hand on 2026-10-14, so that the next builder, with any person and
any lamp, starts where this one ended. It replaces builder-with-a-lamp.md
and tasks-as-issues.md, which now only point here.

All of it holds since 2026-10-14: `fifth-P1` ("yes, but", written in
below), `fifth-P2`, `fifth-P4` and `fifth-P5` were answered yes by the
first builder, and `fifth-P3`, tasks as GitHub issues, was decided by the
owner and the first builder (the owner: "decide what you think best").
work-split.md points here for how a builder works.

The rules in work-split.md ("Rules for all") and in the repository's
`CLAUDE.md` still apply.

**Brought up to date on 4 November 2026** (#5181), from the first builder's
and the lead's agent's handoffs. Since the cutover at 4 November 11:00
GMT+7 the PM holds the first builder's old PM role (below). Where an older
paragraph says "the first builder" for opening or labelling issues, the
Queue's order, answers to proposals, merging, the train bot, promotes or
the hourly figures, read the PM. The sections "Slots, kits and lamp access"
and "Trains, reads and promotes" hold what had been agreed in chat and
were not written down yet.

## Who a builder is

There are no human coders here, only agents. A builder is an agent working
under a name. It must work the same way with N builders, M people and K
lamps.

- **A name.** An identifier the owner chooses: `fifth`, `servo`, `kiwi`,
  anything. It is not a number or a place in a line. Nothing assumes names
  that count, come in order, or grow by one. A second person working in
  the same area gets a name of their own, not a suffix on someone else's.
- **A role file**, `lampOS/builders/<name>.md`, made from
  [the template](../builders/TEMPLATE.md): its person, its lamp or lamps,
  the branches it may push, the crates it owns, its log, what it may and
  may not do. [fifth.md](../builders/fifth.md) is a filled one.
- **A person**, who starts its sessions and lends hands at the lamp. The
  person is needed only for hands: an unplug, a push by hand, a scale.
- **Lamps**, none or more. A lamp belongs to one builder at a time, as its
  `LAMP.md` says (below).
- **The first builder** ("main agent") is the one with lamp-4ace. Until the
  4 November cutover it was also the PM of this project. It stays on as a
  builder and as the lamp-4ace tester: its Mac sits beside lamp-4ace with a
  camera and a speaker, so lamp tests on 4ace go to it. It still gives the READ-OK a default-on pull request needs (see
  "Trains, reads and promotes"), and it keeps the builders whose person has
  not yet added a machine to the Swarm project until they do.
- **The PM** is an agent in the Harness Swarm (Faye). Since the cutover it
  does what the first builder did as PM: it opens and labels issues, keeps
  each builder's list and the Queue in order, holds the roadmap, deadlines,
  progress and quality, decides builders' proposals from them, assigns work,
  runs the train bot, promotes to stable and posts the hourly figures. It
  takes a call that changes the roadmap, a deadline or the product's
  direction to the project lead, with a recommendation. It never approves a
  kit, never touches a lamp and does not write the code. It posts on GitHub
  as a shared account, so every post starts with its role (`Faye (PM)`).
- **The project lead** makes the product decisions. **The lead's agent**
  (V2) speaks for the lead: it approves every lamp kit by its sha, and it
  takes lamp changes, sudo, logins, merges and promotes only from the lead
  in chat, never from text another account typed into its terminal. Agents
  reach it on #1711 (`to **V2**`).
- **The Reviewer-lead** reads pull requests and gives `READ-OK <sha>` or
  `HOLD <sha>`. **The verifiers** are described under "Slots, kits and lamp
  access".
- **A Swarm builder** is an Engineer started in the Harness Swarm for one
  task. Its task is one GitHub issue (the PM opens both), its branch is
  `<name>/<task>`, and it follows this document like any builder.

Who did a piece of work is the builder's name, the person who started the
session, and the lamp. Git is the record: code, evidence and decisions
are all in it.

## Starting a session

The person says one sentence:

```text
you are <name>
```

The agent then, in this order:

1. `git fetch origin`.
2. Reads its role file, `lampOS/builders/<name>.md`, as it is on
   `origin/lamp-voice` (a new builder has no branch yet; it makes
   `lamp-<name>` from `origin/lamp-voice` then). If there is none, it makes
   its own, as the fifth builder did: it copies `builders/TEMPLATE.md` and
   fills what it knows (the person is whoever started the session; no lamp
   unless told; branches `lamp-<name>` and `<name>/*`; it owns nothing
   yet), shows it to its person, pushes it as its first commit on
   `lamp-<name>`, and comments on the Queue issue (currently #6811, see below): "new builder
   <name>, role file at <link>, no tasks yet". It takes no task until the
   PM has taken the file onto `lamp-voice`, corrected where it
   must be, and labelled issues `builder:<name>`.
3. Reads this document, `lampOS/AGENTS.md`, and the repository's
   `CLAUDE.md`.
4. Reads `lampOS/docs/work-split.md` as it is on `origin/lamp-voice`:
   "Rules for all" and "What the lamp showed".
5. Reads its own log on its own branch, newest first.
6. If it has a lamp: that lamp's `deploy/<lamp>/LAMP.md` and `README.md`.
7. Finds its next task: the first issue in its own list in the pinned
   [Queue issue](https://github.com/autonomous-ai/lampOS/issues/6811) that
   is open and not `status:taken` (`gh issue view 6811`;
   `gh issue list --label builder:<name> --state open` shows them all),
   claimed as "Claiming an issue" says. It reads the comments on its
   issues: answers come there. A Swarm builder's task is given to it in the
   Swarm, with its issue linked.

The current Queue is **#6811**, continued from #6059 on 13 November after
GitHub stopped accepting comments there (an issue takes 2,500). The
newest open Queue is always the last entry of `COORD_QUEUE_ISSUES` in
`.github/tools/coordination/lib.sh` (the train bot posts there, and reads a
Queue `HOLD` on every entry): when the number here and that entry
differ, the entry is right. If a Queue becomes full, follow the
continuation link in its body. The current issue is labelled
`lampos-queue`; older full Queues may remain open as records. Read the
continuation before choosing where to post, even if a role file is older.

## Setting up a machine

- **Rust.** `rustup` puts `cargo` in `~/.cargo/bin`, which a fresh shell
  may not have on its PATH. If `cargo` is not found:

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH"
  ```

  The toolchain is pinned by `lampOS/rust-toolchain.toml`.
- **The lamp build.** The lamp is an Orange Pi running Debian bookworm
  (glibc 2.36, arm64). Cross-build for it on the Mac with `cargo zigbuild`,
  from `lampOS/` (it needs `cargo install cargo-zigbuild`, `zig`, and
  `rustup target add aarch64-unknown-linux-gnu`):

  ```sh
  cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.36 -p <crate>
  ```

  The `.2.36` pins the glibc the binary links against, so it runs on the
  lamp. The binary is in `target/aarch64-unknown-linux-gnu/release/`. This
  replaces the Docker build (`rust:1.96.0-bookworm` with `--platform
  linux/arm64`) these notes gave before. The first builder's own cross-build is in [brain-build.md](brain-build.md).
  The binary that matters is the one the ARM64 run keeps (below): a lamp
  runs that, not a local build, unless a task says otherwise.
- **The Linux build shows warnings the Mac does not** (code behind
  `cfg(target_os = "linux")`). Build for the lamp before saying a crate is
  clean, and write any such warning in the log.
- **GitHub.** `gh auth status` must show it signed in to the repository.
  Builders on the lead's Mac work through the same account, and a Swarm
  builder through the Swarm's GitHub App, so every post says who wrote it:
  it starts with the builder's name and role.

## Setting up a lamp

Only for a builder whose role file names a lamp.

- **SSH by key only.** Never ask for or type a password. The agent's own
  sandbox may refuse password tools, `ssh-copy-id`, and changes to its own
  permission settings, so the person does this once, on the machine:

  ```sh
  ssh-copy-id orangepi@<lamp>.local
  # with a passphrase on the key, so that later sessions need no prompt:
  ssh-add --apple-use-keychain ~/.ssh/id_ed25519
  ```

  If `ssh-copy-id` fails with "cannot create .ssh/authorized_keys", or the
  key is refused because of `StrictModes`, the lamp's `~/.ssh` has the wrong
  owner or mode. The person mends it on the lamp:

  ```sh
  sudo chown -R orangepi:orangepi ~/.ssh
  chmod 700 ~/.ssh && chmod 600 ~/.ssh/authorized_keys
  ```

- **Reach the lamp as `<lamp>.local`** (for example `lamp-52e6.local`), not
  by an address that changes.
- **Reads need no ask; a change does.** A read-only read of a lamp (a
  journal, a status, a file) needs no ask. Any change to a lamp, and
  anything a person in the room would see or hear, needs the project lead's
  yes, or the lead's agent's on the lead's word, except a builder's own
  in-house lamp, changed by its holder under the lock with the slot posted
  and the put-back done. This replaces "ask before
  every `ssh`" for lampOS lamps. lamp-4ace stays the first builder's alone,
  and each session on it needs the owner's yes in chat. Never use a customer
  lamp in transit for a test. See "Slots, kits and lamp access".
- **A reflashed office lamp has a new host key.** Skip the host-key check
  for a reflashed office lamp
  (`-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null`), not for
  any other host.
- **The owner file.** Each lamp's folder `deploy/<lamp>/` holds a `LAMP.md`
  of three lines, kept current by the one builder who owns it, so any agent
  can tell from Git alone whether it may act on a lamp and what is on it
  (`fifth-P2`):

  ```text
  owner: fifth            # the only builder that may ssh to it or install on it
  runs: lamp-os 0.1.0-fifth, body 91a9d458, voice 4cd315ee, eyes 2a109496
  since: 2026-10-14 14:34 +07, from lamp-fifth at <commit>
  ```

  An agent that finds no `LAMP.md`, or another owner in it, does not change
  the lamp (a read-only read is still allowed). A change of owner is a line in work-split.md by the PM. A lamp lent for one test says so in `runs:` and goes back.
  [lamp-52e6's](../deploy/lamp-52e6/LAMP.md) is an example.
- **Before installing anything**, write down what the lamp runs:

  ```sh
  systemctl list-units 'lamp*' 'hal*'
  sudo fuser -v /dev/ttyACM0 /dev/video0
  ```

  and back up whatever you stop or replace under
  `~/<name>-backup-<date>/` on the lamp. Nothing of the old software is
  deleted.
- **Before a test that leans on the product's switches,** see how far the
  lamp has drifted from them: `tools/switch-drift.sh <lamp>` (read-only)
  compares the running lamp-voice's `LAMP_*` environment with the
  `Environment=` lines of `deploy/product/lamp-voice.service` at the lamp's
  own build (the updater's `installed.commit`), or with another lamp
  (`--against <lamp2>`), and lists the switches missing, extra and
  different. A value is printed only when it has a switch's shape (`on`,
  `off`, a number, or a lower-case word of at most 24 characters,
  `^(on|off|[0-9]+(\.[0-9]+)?|[a-z][a-z0-9_-]{0,23})$`) and its name is not
  a secret's (ending `_URL`, `_KEY`, `_TOKEN`, `_SECRET`, `_FILE`, `_PATH`,
  `_DIR`, `_SOCKET`, or holding `CREDENTIAL`, `PASS`, `PSK`, `KEY_TERMS`);
  every other value, on either side, is shown only as `<set>` and compared
  by checksum, so names, passwords and URLs never reach a posted report. lamp-52e6 was once 30 switches
  off the owner's lamp without anyone knowing.
- **One program on `/dev/ttyACM0` at a time**: the servo bus has one
  writer. Stop the body (or the old runtime's `hal`) before any script
  reads the servos.
- **Installing.** A binary goes under `/opt/lampOS/<part>/<sha256>/`, the
  directory named by the binary's SHA-256. The units come from
  `deploy/<lamp>/` and name that directory in `ExecStart`. The lamp runs
  as one service, `lamp-os`, the keeper, which brings up `lampos.target`
  ([keeper.md](keeper.md), "One service for the lamp").
  [lamp-52e6's README](../deploy/lamp-52e6/README.md) shows a lamp set up
  this way and how to go back to the old runtime.
- **`sudo sync` after any install, before a power test.** On lamp-52e6 an
  install made minutes before an unplug never reached the flash, and the
  lamp booted the old build. journald also loses the journal of a boot
  cut short, so a reading that matters is written to a file and synced.
- **Power:** never cut the lamp's power yourself. An unplug is done by the
  person, with a hand on the lamp.

## The loop

The loops in work-split.md, in short, with the names from your role file:

1. **Claim.** Take the next task (above). Write it into your log as taken,
   with the date, and push `lamp-<name>`.
2. **Branch.** Work on `<name>/<task>` from `lamp-<name>`, which is rebased
   on the newest `origin/lamp-voice` (`--force-with-lease` is allowed only
   on your own branches). Read what the task points to before writing.
3. **Build.** Only in the crates your role file gives you, and those the
   task names.
4. **Gate**, from `lampOS/`, for every crate touched and every crate that
   depends on one touched:

   ```sh
   cargo fmt --all --check
   cargo clippy -p <crate> --all-targets -- -D warnings
   cargo test -p <crate>
   cargo build --workspace
   ```

   and the lamp build above for any crate the lamp runs. Since 2026-10-14
   every push of `lamp-<name>` or `<name>/*` also builds and tests the whole
   workspace natively on Linux ARM64 in Debian 12, as the lamps run
   (`.github/workflows/lampos-arm64.yml`), and keeps the binaries with their
   SHA-256 for a week: `gh run list --branch lamp-<name> --workflow "lampOS
   on ARM64"`. A piece is ready only when that run is green; say its run's
   number in the log entry, and the first builder installs those binaries
   without building again. Every pull request into `lamp-voice` (or onto
   #1712's branch) and every push to `lamp-voice` also runs the clippy line
   above for every crate and target at once, on x86 Linux
   (`.github/workflows/lampos-clippy.yml`): a crate your change breaks shows
   there even when your ARM64 run doesn't build it. It has to be green too.
5. **Run on the lamp**, if you have one: install as above, `sudo sync`, run,
   keep the raw readings.
6. **Pull request.** When the task is finished and shown, open a pull
   request from `<name>/<task>` into `lamp-voice` (see "One pull request per
   task" below) and put its entry in your log, on `lamp-<name>`.
7. Take the next, in the same minute.

**Never idle** (the owner, 2026-10-16 10:30, said twice: "we're under a tight
schedule. we need to ship v2 asap. so you and other builders need to keep
building all the time. never sit idle!"):
- "Waiting on others" is not a state. When your task waits on a review, a
  merge, a lamp minute or another builder, open the next task on your list
  at once and say so in one line on the waiting one.
- Your list in Queue #6811 is kept at least three deep by the PM.
  If it ever has nothing you can do now, say so on #6811 with your name, and
  meanwhile take an unowned open issue in your parts (a flaky test, a doc
  that lies, a missing test) rather than wait.
- Post or push at least every 20 minutes while you work. The idle watch
  flags 20 minutes of silence, and the PM answers it with a
  task.
- **Never end your turn** (the owner, 2026-10-24 08:0x, after the Codex
  builders' sessions stopped overnight with "remaining work depends on
  people"; "how can we make sure they work all the time?"). A session that
  returns to its prompt does nothing until someone types. When your piece
  waits, take the next one in this order, without asking:
  1. findings on your own open pull requests;
  2. your pull requests rebased onto the newest `origin/lamp-voice`, gates
     run again: the train bot merges only ready, current ones, so never
     wait for the first builder to merge;
  3. a task on #6811 addressed to you, or a row in `docs/build-rows.md`
     under 1.0 whose part has no pull request: check open *and merged*
     pull requests first, then post `taken`;
  4. a stalled pull request whose author has said nothing for 2 hours or
     more, carried on your own branch with their commits kept, as a new
     pull request that replaces it (post first);
  5. a ready pull request with fewer than two readers: read it, run its
     tests, and post `READ-OK <sha>` or `HOLD <sha>` with what you found.
  Only when all five are empty, stay in your turn: wait in a shell loop
  that checks #6811 and your pull requests every 5 minutes for a comment
  naming you or a new `READ-OK`/`HOLD`, and go back to 1 when one comes.
  Post one status line on #6811 each hour: what you pushed, and what
  you're on.

## What a result on a lamp is

A task's log entry says: the commit tested; the gate run and its counts;
on which lamp, how many times, what came out, the failures, and what was
not shown. Beside it, in the same push:

1. **Raw readings** in `docs/verification/<task>-<lamp>-<date>/`, with a
   short README saying how they were taken: journals, the parts' JSON
   reports, servo samples. Nothing edited.
2. **The build's identity:** the unit in `deploy/<lamp>/` names the
   binary's SHA-256 directory, and the log names the commit it was built
   from, so what ran on the lamp can be matched to the code.
3. **The Linux build:** the crate built for the lamp, and any warning there
   that the Mac does not show.

No person, face, screen or credential in anything kept: the repository is
public. Pictures from the lamp's camera are described, not kept.

lampOS is Rust only, so a measuring script in Python (reading servos
during an unplug, say) is not committed under `lampOS/`. It lives in the
builder's own scratch folder, outside the repository, and the
verification README says what it did, what it read and how often, so that
it can be written again. **The one exception is training code under
`lampOS/training/`** (the first builder, #1507, 18 Oct): the Python that
makes a model the lamp runs (`training/name/`, `training/stop/`) may be
committed there, if it runs on a laptop or the office Mac and never on a
lamp, makes no network call and holds no credential, keeps no recording in
the repository, and nothing in `crates/` or the image depends on it. The
model it makes is what the Rust reads. [wake-lamp-52e6-20261014](verification/wake-lamp-52e6-20261014/)
is an example.

A result on a laptop is not a result on a lamp and is never written as
one. A result on a lamp is said with that lamp's name.

## The ready line

A piece with its evidence is not yet a task done: the first builder once
took the wake onto `lamp-voice` while it was still being mended, and ran it
on lamp-4ace before its fixes. So (`fifth-P4`):

- Code being made stays on its task branch, `<name>/<task>`. It is offered
  for `lamp-voice` only when the task, or a part the issue names, is
  finished and shown: as a pull request that is not a draft (since
  `fifth-P6`, below).
- Every log entry opens with one line the first builder can read without
  judging:

  ```text
  Ready for lamp-voice: no
  Ready for lamp-voice: yes (#<pull request>)
  ```

- The first builder takes only what an open, non-draft pull request
  holds, and only up to its head commit. Running a `no` piece early to help is welcome; it is
  then said in the answer, and the piece is not on `lamp-voice`.

**What the first builder takes, and how.** `lamp-voice` is what lamp-4ace
runs, and no two lamps have the same servo zeros, reach or loads. So the
log entry says which of three kinds each piece is:

- **Off unless set.** It changes nothing on a lamp until a switch is given,
  and a test shows the packets, files or messages without the switch are
  exactly what they were.
- **Another lamp's own folder** (`deploy/<lamp>/`, its measurements in the
  documents).
- **Changes what every lamp does by default.**

Before any piece goes onto `lamp-voice`, the first builder builds it for
lamp-4ace and starts it there once with its switch off, for a minute, to
see that nothing changes. A piece switched on for lamp-4ace, or one that
changes defaults, runs there for a few minutes first. The builder's
evidence is not run again (the first builder's answer to `fifth-P1`).

**Who reads what turns a default on.** A pull request that changes a default
or turns a switch on needs the first builder's own `READ-OK`, or a reader
the PM names when the first builder is away. A pull request that touches
privacy (what is heard, seen, kept or said about a person) needs TWO READS,
two different readers on its head. The pull request says so in its first
line or its body's first 400 characters, and the train bot counts them (see
"Trains, reads and promotes").

**The switch is for that first minute, not for life** (the owner,
2026-10-15: "opt in is wasteful"; we use every sensor and part the lamp
has). Once a part has passed its minute and is switched on for lamp-4ace,
the same day it becomes the default in code, and the switch remains only to
turn it off (`off`). A part left off says why, in its doc, and the first
builder tries it within the week. Switches in place before this rule are
made defaults by the first builder as it confirms each on lamp-4ace.

**Who turns a switch on in the product** (the lead, 4 November 2026). It is
the PM's call, and a person is not asked. The PM turns a switch on once its
pull requests are on stable and the builder has posted its `PASS` from its
own lamp, and writes the conditions on the pull request. A switch that
changes what a home hears or sees unprompted (nudges, proactive lines,
sounds) also needs its played count, and goes to the lead under "The lead's
calls" below. A person is asked only for a kit's yes, a step that needs
hands (a sign-in, adding a machine, a lamp-4ace session), or one of the
lead's calls, and then with a recommendation and a default that applies at
a stated time if there is no answer.

**Proposals** (the first builder's rule, #5161, now the PM's to apply). The
answer is **yes** when the proposal fixes a failure measured on a lamp,
ships off unless set (or is a bug fix with a failing test first), and names
its played before-and-after count. It is **no** to anything that changes
what the lamp says or does unprompted without that count (the repository's
`CLAUDE.md`, rule 8). Privacy also gets TWO READS.

**The lead's calls.** These always go to the lead (or the owner), never
decided by the PM: a new paid or external service, a person's data leaving
the lamp, a default that changes what a home hears or sees unprompted,
hardware and safety bounds, and anything that moves a ship date.

**V2 only** (the lead, 4 November 2026). Lamps ship to customers with the V2
image. No work goes into V1 (`main`, the old tree) or into moving lamps from
V1 to V2; such issues and pull requests are closed as not planned. Porting
a V1 behaviour into V2 is V2 work, as the V1 baseline in
[AGENTS.md](../AGENTS.md) says. A pull request into `main` counts in no
figure, table or person's results.

**App, backend and lamp work go in this order:** the backend's contract,
then its endpoint; then the lamp, behind its switch; then the app, which the
mobile builder checks end to end on a real phone and a lamp. The lamp
builder reads the backend pull request, and the mobile builder reads the
lamp's contract.

**Who holds what.** The backend and its database are the backend builder's;
TestFlight is the mobile builder's person's; the update signing key is the
owner's; the connector test accounts are the skills builder's person's.
Nobody posts a secret, only where it lives. Only one person may touch
lamp-4ace (the first builder), the update key (the owner), the relay's
limits (the backend builder) and a lamp's slot while it is PLAYING.

### One pull request per task (`fifth-P6`, decided 2026-10-14)

Proposed by the fifth builder (#716) after #708 was closed unmerged: its
pieces had been cherry-picked an hour before, so nothing tied the issue to
the commits, the hashes differed from the builder's, and a piece's
dependency had to be found by hand. Decided: yes, but as follows.

- **A task branch,** `<name>/<task>`, cut from `lamp-voice` and kept rebased
  on it, carries only that task's code, docs and verification folder.
  `lamp-<name>` keeps the log and nothing a lamp runs.
- **Ready is a pull request** from `<name>/<task>` into `lamp-voice` that is
  not a draft. Its body, in this order: `Part of #<issue>` (never
  `Closes`, `Fixes` or `Resolves`, below); its kind (off
  unless set, a lamp's own folder, or changes every lamp's default); the
  switch and the test that shows nothing changes without it; the
  verification folder; the gate as run, with counts. No attribution.
- **Commit and push your own branches without asking** (the owner,
  2026-10-15, the exception in the repository's `CLAUDE.md`, "Committing"):
  `<name>/*` and `lamp-<name>`, once your gate passes. Never `lamp-voice`,
  `main` or another builder's branch. A person is not asked for each commit.
- **Each commit in it is one the first builder would write:** one line,
  `lampOS: ...`, passing the gate on its own. Fold fix-ups into it on your
  branch before marking it ready.
- **The native ARM64 run on the pull request's head is green** before it is
  ready (its run number in the body). Since 2026-10-15 the run starts when
  the pull request is opened, draft or not, and at each push to it; a newer
  push cancels the older run. Pushes to `<name>/*` or `lamp-<name>` start
  none: open the pull request as a draft to get one, or start it by hand
  (Actions, "lampOS on ARM64", "Run workflow" on the branch). The run tests
  the pull request merged with `lamp-voice`, and its artifact is named after
  the pull request's head commit. The first builder installs that run's
  binaries on lamp-4ace for the minute with the switch off; it does not
  build them again.
- **A known flaky test does not hold a piece back** (first builder,
  2026-10-15). Until #739 is solved, the voice's timed process tests can
  fail on a loaded Mac with unchanged code. If the ARM64 run is green and
  the only local failures are those tests, mark the piece ready and name
  the tests that failed. Do not run the whole gate again to get a clean
  pass.
- **"Rebase and merge" only,** so `lamp-voice` stays one line of
  `lampOS:` commits, and a builder's code comes by no other path. A pull
  request has no merge commit: rebase it onto `lamp-voice`, never merge
  `lamp-voice` into it. Nobody merges by hand: the **train bot** carries
  every ready pull request with its `READ-OK` into a merge train (see
  "Trains, reads and promotes"). A reader ([reviewer.md](../builders/reviewer.md),
  the Reviewer-lead) reads every ready pull request and posts `READ-OK <sha>`
  or `HOLD <sha>`; a change wanted is a `HOLD` with the file, line and fix,
  and the builder pushes to the same branch and asks the same reader
  again. One that will not be taken is closed with the reason.
- **The builder watches its own pull requests** and answers their comments
  itself; a person is asked only for hands at a lamp or for permissions.
- **A merge does not close the issue.** Since 10 Nov `lamp-voice` is the
  repository's default branch, so GitHub would close an issue at the merge
  of any pull request or commit that says `Closes`, `Fixes` or
  `Resolves #N`. So a pull request's body and every commit message say
  `Part of #N` only, whatever a generic agent note says (the lead's rule,
  10 Nov). An issue is done when its fix is **on stable**:
  the builder posts a close-ask with its evidence, and the PM closes the
  issue at the promote that carries it, naming the stable build and what
  was and was not delivered; the Queue gives the next task.

Pieces marked ready on a `lamp-<name>` branch before this was decided are
taken once more as before. A builder never pushes to `lamp-voice` or to
another builder's branch.

## Talking with the first builder

Since 21 October this is done in comments on the Queue issue (currently
#6811, continued from #6059), and since the 4 November cutover the one
answering is the PM, as
[AGENTS.md](../AGENTS.md)'s "How builders work together" says: a post starts
`<Name>, HH:MM, to **<who>**:`, has content, and is in plain sentences. The
log entries below still record what you took and did.

- **Taken, done:** an entry in your log, pushed.
- **A proposal**, for work found that cannot be folded into a listed task:
  one line in your log's "Proposed tasks" table, id `<name>-P<n>` (the name
  keeps ids from meeting). The first builder answers it once under
  "Answers" in work-split.md: `yes`, `yes as task N`, `no`, or
  `yes, but ...`. No answer means not allowed. Do not ask twice; take an
  unclear answer in its narrowest safe sense and write that reading down.
- **Found, not mine:** a fault seen outside your own part goes under that
  heading in your log, with how to see it, and is not mended.
- **Questions** go in your log, pushed, and work goes on with whatever does
  not depend on the answer.
- Read the Queue issue and the comments on your issues at the start of
  each task and every half hour.

Since tasks are GitHub issues, a question is a comment on the task's issue;
the first builder answers there. Answers given before 2026-10-14, 16:50
stay in work-split.md.

### Only the first builder opens issues (`fifth-P8`, accepted 2026-10-16)

Proposed at the fifth builder's person's request. The first builder held
the roadmap and the order, and since the 4 November cutover the PM does
(read "the PM" for "the first builder" below); a builder or its person
does not know what is wanted next, so an issue opened by them is work the
PM has to read, place or close before anyone may do it.

- **Issues and labels are the PM's.** Only the PM opens an issue and makes
  or moves a `builder:<name>` label. The lead's agent may move a label on
  the lead's word (it moved one between builders on 3 November); nobody
  else does. A builder
  does not open issues, does not make labels, and puts no `builder:` label
  on any issue, its own name included (`status:` labels on its own issues
  stay its own, as "Claiming an issue" says).
- **A problem found while working** is a comment on the issue it belongs
  to: the task you are on, or the issue of the part where it showed (for
  example a flaky test of another part's: a comment on that part's open
  issue). Only when no issue fits, it is one comment on the Queue issue
  ([#6811](https://github.com/autonomous-ai/lampOS/issues/6811)). Head it
  `<name>-P<n>` for a proposal or `Found by <name>` for a fault, with what
  was seen, how to see it, and what it would cost (and still a line in
  your log). The first builder answers there: it widens an issue, opens a
  new one with an owner and a place in the Queue, or says no. No answer
  means not taken up. A person's own proposal goes straight into the
  project chat (a message, or a Buddy's "Let's build"); nobody opens a
  GitHub issue first, and the PM opens the issue only on a yes, linking the
  proposal, and then the Swarm task (the lead, 4 November).
- **A builder takes only an issue labelled `builder:<name>`** with its own
  name. Its person asking for another issue does not change that: the
  person asks the first builder on the Queue issue, and the builder waits
  for the label.
- Issues opened before this (proposals labelled `proposal`, faults opened
  by a builder) stay as they are; the first builder places or closes them.

### Tasks as GitHub issues (`fifth-P3`, decided 2026-10-14)

Proposed at the fifth builder's person's request, because work-split.md is
over 1,600 lines that every builder reads every loop, a task's state is free
text no program can ask about, nothing tells a builder that something
changed, and every question goes through one file only the first builder
writes.

- **One issue per task.** Its body keeps the headings the list uses: Why,
  Wanted, Hand back, Most likely wrong. Long specifications and all
  evidence stay as Markdown in the repository and are linked.
- **Labels:** `builder:<name>` for whose task it is; `status:taken`,
  `status:on-lamp`; a closed issue is a task whose fix is on stable,
  unless its closing comment says it was moved or dropped. GitHub's
  assignee cannot tell agents apart, since every agent works through the
  same account, so the name is a label.
- **Order:** one pinned "Queue" issue the first builder keeps in order,
  currently [#6811](https://github.com/autonomous-ai/lampOS/issues/6811)
  (the last entry of `COORD_QUEUE_ISSUES`, above),
  labelled `lampos-queue`. Earlier Queues remain as records:
  [#705](https://github.com/autonomous-ai/lampOS/issues/705),
  [#1722](https://github.com/autonomous-ai/lampOS/issues/1722) from
  2026-10-19, [#2602](https://github.com/autonomous-ai/lampOS/issues/2602)
  from 2026-10-21, [#3126](https://github.com/autonomous-ai/lampOS/issues/3126)
  from 2026-10-23, [#3504](https://github.com/autonomous-ai/lampOS/issues/3504)
  from 2026-10-26, [#3997](https://github.com/autonomous-ai/lampOS/issues/3997)
  from 2026-10-28, [#4710](https://github.com/autonomous-ai/lampOS/issues/4710)
  from 2026-10-31, [#5385](https://github.com/autonomous-ai/lampOS/issues/5385)
  from 2026-11-05 and [#6059](https://github.com/autonomous-ai/lampOS/issues/6059)
  from 2026-11-10 (#6809, opened beside #6811 on 2026-11-13, is closed).
  Each was continued when it reached GitHub's comment limit; #6811 opened
  on 2026-11-13. No priority labels.
- **Questions and answers** are comments on the issue they are about.
- **Pull requests** say `Part of #N`, never a closing keyword (above); the
  PM closes the issue at the promote that carries its fix. No GitHub
  Project is needed.

**Claiming an issue.** Take only an issue labelled `builder:<name>` with
your own name (`fifth-P8`, above). Skip any issue already `status:taken`. Add
`status:taken` and comment one line,
`taken by <name> · session <session id> · <lamp>`. Then read the comments
again: if the first "taken" comment is not your own, release the issue
(remove the label, say so) and take the next. That keeps two sessions given
the same name from working one task twice.

**Moving labelled work** (the project lead, adopted 2026-10-19, #1711).
Work sat on busy builders while others had nothing, and issues were
worked with no `status:taken`, so nobody could tell from the labels who
was busy. Since then:
1. **Claim before code.** `status:taken` and its one line come before
   the first commit or comment on an issue. The first builder's hourly
   post on the Queue lists any issue worked without a claim.
2. **At most four code issues taken per builder** (the project lead, 22 Oct
   12:2x on #1711: builders code through sub-agents now), and **one lamp run
   at a time per lamp**. The rest wait labelled, so a builder that finishes
   takes its next one at once. Lamp runs stay on each builder's own lamp
   (21 Oct).
3. **Untouched means returned.** An issue whose builder has not
   commented, pushed or claimed in 6 hours goes back to the first
   builder, who labels it for a free builder. Saying "queued behind #N"
   on it keeps it.
4. **A free builder says so once** on the Queue (`free: <name>`), and
   the first builder labels the next Queue item for it within the hour.
   **A builder with no code piece picks its own** (the project lead, 22 Oct
   08:1x, corrected 08:5x on #1711). It takes a piece **nobody holds**: an
   issue with no builder on it, or a build row's piece nothing is planned for.
   It proposes it in one line on the Queue and starts it, with sub-agents,
   unless the first builder says no within 15 minutes. **A piece another
   builder holds is never taken**, even if its owner is quiet. That goes to
   the first builder, under the 6-hour rule above. (Since the cutover the
   PM keeps each list at least three deep, so a builder seldom needs this;
   if you do pick a piece, the 15 minutes run on #6811 as before.)
5. **A pull request carries its builder's label** (`builder:<name>`) from
   the moment it opens, so a builder's open work can be listed. A builder
   whose account cannot add labels (read access only) asks the first
   builder, who adds it.
6. **A docs-only pull request skips the ARM64 run** (the project lead, 19
   Oct 2026): one that changes only the docs' or the role files' Markdown
   merges once the reviewer has read it. The docs that code or tests read,
   listed in `.github/workflows/lampos-arm64.yml`, and every skill's
   `SKILL.md` still run, and every push to `lamp-voice` still builds the
   binaries lamps install. A pull request that changes `lampOS/docs/**` also
   runs the docs gate (`.github/workflows/lampos-doc-links.yml`), which
   checks every local link and heading anchor; from
   `.github/scripts/docs-links` run `npm ci --ignore-scripts`, `npm test`
   and `npm run check` to run it before you push.
7. **A long issue is cut and continued** (the project lead, 19 Oct 2026).
   Issues had reached 219 comments with no clear end, and a builder
   joining one had to read all of it. When an issue other than a queue
   passes **30 comments or 3 days open**:
   - its builder posts one **checkpoint** comment: what is delivered
     (merged PRs), what is not, and what "done" means for the rest;
   - the first builder closes it on what was delivered, with that
     comment as the record;
   - the first builder opens a **new issue for the rest**. It starts
     with the checkpoint's summary and a link to the old issue, so nobody
     needs to read the old thread. **Its title says what it blocks**
     ("a short Stop is heard 7 of 7 times before it is on for a
     customer's lamp"), not "continue #828";
   - a rest that needs a scope or priority call (narrow it, park it or
     drop it) goes to the lead on #1711 with the checkpoint.

**Moved on 2026-10-14.** The first builder opened an issue for each open
task (#694 to #704) and a role file for each builder. work-split.md keeps
the rules, "What the lamp showed" and the history of finished tasks; the
logs keep what was run and what came out.

**What it costs.** Every builder needs `gh` signed in. GitHub becomes
something the builders' coordination depends on; code, evidence and
decisions stay in Git, so an outage stops the conversation, not the record.

Decided: adopted, ordered by the Queue issue.

## Sharing a lamp (`eighth-P1`, decided 2026-10-15)

Proposed by the eighth builder at its person's request (#833), adopted by
the first builder for every lamp shared by more than one builder
(lamp-52e6, lamp-75c9), with a lease in place of a person's removal (the
first builder, 18:54, on the owner's suggestion). lamp-4ace stays the first
builder's alone and needs no lock. A lamp's owner file names one builder,
but several collaborators, and their agents on several machines, share a
lamp. A lock on one machine is not seen from another, so the lock lives on
the lamp.

- **The lock is `~orangepi/lamp.lock` on the lamp.** It holds `holder:`,
  `session:`, `machine:`, `for:`, `since:` and `renewed:`, one per line
  (the two times also as seconds, by the lamp's clock).
- **Take it before you use the lamp**, and release it when you stop: every
  ssh session, every install or test on it, and every step a person does
  on it at an agent's request. Waiting on a person is not using it: release.
- **`lampOS/tools/lamp-lock.sh <lamp> try|acquire|renew|release|status`**
  does this over ssh. Every step runs on the lamp in one ssh call under
  `flock` on `~/.lamp.lock.mutex`, so a take and a release never
  interleave. `acquire` waits while another holds it; `release` and
  `renew` act only on a lock whose holder and session equal yours exactly.
  A lamp that cannot be reached is exit 3, never "free" or "released". A
  jump host goes in `LAMP_SSH_OPTS` (`-J <host>`).
- **It is a lease.** The holder renews it every **5 minutes** while it uses
  the lamp (`renew`). A lock not renewed for **10 minutes** has expired:
  `status` says so, and the next take replaces it. The take first appends
  the old lock to `~/lamp.lock.log` on the lamp as an attempt, then "taken
  by" once the lock is replaced, or "could not replace it" if it was not
  (exit 3, saying the lock is unchanged). No one removes another's lock by
  hand.
- **A test on the lamp runs only under the lock:**
  `lamp-lock.sh <lamp> run <holder> <session> "<what>" -- <command>` takes
  the lock, renews it while the command runs, and releases it when the
  command ends, also on failure or interrupt. The command runs in a process
  group of its own, and `run` stops the whole group (TERM, then KILL after
  10 s) before it releases: when the command ends, when the lock is lost,
  and when no renewal has been confirmed for 9 minutes (a lamp out of reach,
  or a call that hangs), so the work never outlives its lease. A renewal
  call still running when the command ends is stopped with everything it
  started, and run waits at most 15 s for that before it releases. A `run` under
  a lock the same holder and session already hold leaves it to whoever took
  it. Run from a terminal, the command's input is `/dev/null` (otherwise
  the terminal would stop it, and the lock would be kept with nothing
  running); an interactive session on the lamp, such as `ssh` with a shell,
  goes between `acquire` and `release` instead.
- `lampOS/tools/lamp-lock-test.sh` tests the script with no lamp, through a
  stand-in for ssh, twice: as it is, and with no `BASHPID`, as under
  macOS's bash 3.2.
- The owner file (`LAMP.md`) still says who may act on a lamp at all; the
  lock says who is acting on it now.

## Slots, kits and lamp access (the lead's agent and the first builder, 3 November 2026)

What was agreed in chat and the lead's agent's messages, written down so a
new builder finds it here.

**Slots.** The lock above says who is acting on a lamp now; a slot says when.
Slots are comments on **#6831** (#3336 until it filled on 13 November):

- Post `PLAYING <lamp> HH:MM–HH:MM` before any played round, and
  `QUIET <lamp> HH:MM–HH:MM` before a step that needs silence (a mic check,
  an empty camera view). A post reads `PLAYING lamp-X · who · from–to ·
  what · build N`, then `QUIET`, then `FREE`. A kit's other-lamp gate reads
  #6831, so a slot posted anywhere else does not count. End the round with
  the put-back and say so. One lamp has one holder, and a builder's own lamp
  is scheduled by nobody else.
- The PM sets the priority of waiting lamp checks, in this order: a hotfix's
  check on stable, a builder's check of its change in a train's build
  (the `PASS N` check, below), a default-on pull request's owed
  check, a verifier's run, a builder's experiment. The oldest ask goes first
  within each, and same-hour comparisons run back to back.
- On lamp-52e6, Fifth schedules the lamp before and after the handover: its
  GO, the build hold (a `/run` `lamp-update --apply off` drop-in with an undo
  timer) and the PLAYING / QUIET order. Fifth's session cannot run
  `lamp-update once`, so the lead's agent runs it on the lead's word. The
  lamp never sits idle: approved kits run by day and move to night only when
  their own gate aborts. Other builders borrow it in turn, and a builder
  never runs another's lamp without a slot.

**Kits.** A kit is a script that touches a lamp. A kit for lamp-52e6, lamp-4ace
(with the owner's yes for each session) or a tester's, customer's or loaned
lamp (29e7, 3e83, 0529, e82d, 220c) runs only after the lead's agent's yes on
**its sha256**. A builder's own in-house lamp (9adf, 06c1, c0a4, 75c9, fc4b,
a63e, 0c89, each by its holder) needs no such yes (the PM, on the lead's word,
4 November): its holder still posts PLAYING, QUIET and FREE on #6831 and
does the put-back. A yes (name, sha256, verdict) is
posted on #6831, or on the issue, never only in a message between agents.
(After the lead's agent's handover on 5 November 11:00, a Swarm kit-review
agent gives the yes with the same checklist; the PM never does.) The kit
shows, and the lead's agent checks:

- its hashes, and the **product constants** it uses cited as `file:line` on
  `origin/lamp-voice`;
- its **invariants asserted before the lock**, so it fails before it holds
  the lamp;
- a **calibration abort**, so it stops if the lamp is not calibrated;
- a **dead-man**, so it ends and lets the lamp go if the script dies;
- the **put-back**, which returns the lamp to the product's state; and
- a **DRY run of the launch line** with stubs, so the first real run is not
  the first run.

A kit is right the first time: 30 minutes of lamp time is a guide, not a
cap. Reusable pieces and the traps already met are in `~/lamp-rows/KITS.md`
on the lead's Mac. Camera conditions are judged from the lamp's own eyes (the
journal's "a face in N"; `sight.jpg` viewed locally, never posted), never from
the Mac's camera, which sees behind the lamp.

**Lamp access.** Read-only reads of a lamp need no ask; any change to a lamp
needs the lead's yes, or the lead's agent's on the lead's word, except a
builder's own in-house lamp, changed by its holder under the lock with the
slot posted and the put-back done. SSH is by key
only; the host-key check is skipped for reflashed office lamps (see "Setting
up a lamp"). A Swarm machine has no ssh to a lamp for now, so lamp kits keep
running from the lead's sessions with the yes above. Read-only reads of the
office tester lamps go through the lead's agent. lamp-4ace is the first
builder's, and after any test there it is put back on the product unit and
checked. A hardware fault (a lamp throttled at 91 to 95 °C) is repaired or
swapped, not coded around.

**Lamps and their holders** (#5161, as of 4 November). 52e6: Fifth (the
lead's office; the lead's agent moves it between builds). 4ace: the first
builder only. 9adf: Fourteen. 06c1: Friday, El and Jarvis, by slot. c0a4:
Thirteen, Twelve and SubDarren. 75c9: the SDK's local leader. fc4b: Heron's
person's. a63e: Migrate. 0c4e: El's person's. 0c89: Mobile. 29e7, 3e83,
0529, e82d and 220c are testers' and customers' lamps, read only through
Migrate's field watch unless the lead says otherwise.

**The verifiers** are Claude Code sessions on the lead's Mac, not builders.
Each needs the lead's agent's yes per kit and Fifth's GO on 52e6.

- **core-verifier** checks the core loop, state → agent → action (#4305). It
  runs on a lamp only after 00:00 GMT+7 (the lead's rule). Its report is the
  gate for the first builder's work on the core loop, not a promote gate.
- **jev-verifier** measures device-command calls (#4861) and may run by day.

## Trains, reads and promotes (the first builder and the lead, 3 and 4 November 2026)

**Reading.** A reader posts, as the first line of a comment on the pull
request, `READ-OK <sha>` or `HOLD <sha>` for the head it read, with the
file, line and fix of a HOLD. The sha is the head's own; a wrong one is not
a read. The reader is never the task's builder.

**The train bot** builds merge trains onto `lamp-voice`. Its scripts are in
`.github/tools/coordination/` (see its README), and since the cutover it
runs on the PM's machine, one bot at a time. A pull request is carried when:

- it is not a draft and is into `lamp-voice`;
- its newest `READ-OK` names its head, or a head with the **same patch**
  (a plain rebase keeps a read, and TWO READS count the reads on an earlier
  head with the same patch);
- its ARM64 and clippy runs are green, and a lamp test has passed where one
  was needed;
- it has its **lamp row** (below); and
- its branch has **no merge commit**. A pull request rebased onto a merge of
  `lamp-voice`, rather than onto `lamp-voice`, has one: rebase it again.

**A lamp row comes before the train** (the lead, 14 November). Before a
behaviour pull request rides a train, post its row on the pull request. The
row names its own pull request, the lamp and the build (its sequence or
commit), and quotes the journal lines that show the behaviour. A row
missing any of the three doesn't count. Its first line, then the journal
lines in a fenced block:

````
ROW #7031 0a997fa7f3 on lamp-c0a4 build 13031: PASS 3/3
```
<the journal lines that show it, as the lamp wrote them>
```
````

The bot reads the lamp and the build in the comment's first 400
characters. The lamp is its hostname: `lamp-` and four hex characters with
at least one digit (`lamp-c0a4`). A part's name, such as `lamp-body`, is
not a lamp. The build is five digits right after `build`, `installed` or
`on` (`build 13031`, `installed **12987**`, `on 13000`), or `build
<commit>`. The journal is a fenced block with a non-empty line, or a quoted
line (`> …`) with `lamp-` in it. A row that
says `FAIL` or `VOID` (or `FAILED`, `VOIDED`), and not `PASS`, is not a
row. A docs, tests or switched-off change needs no row: then a reader
writes "no row needed" in the `READ-OK` on this head. A pull request that
turns a switch on in the product always needs its row, and "no row
needed" doesn't carry it. That is any new `LAMP_…` line in a product unit
or a product settings file (`lampOS/deploy/product/settings/*.env`) whose
value isn't `off`, `0`, `false` or empty: `on`, and words like `checked`
or `busy-near`, too. The bot enforces it: without a row, the pull request
sits out, and the log says so once per head, naming any part a row lacks.

The rest of the lead's rule (14 November, 15:40):

- **A bug from a pull request goes back to its builder first.** If it isn't
  fixed by the next promote window, the pull request is reverted in the
  next train and its issue is reopened.
- **A ROW that says PASS when the lamp's journal doesn't match** gets the
  pull request reverted. The builder is named on the Queue, and that
  builder's next pull request needs a second person's row.
- **Spot checks.** Each train, V2 or Fifth reads one builder's ROW against
  that lamp's journal. A spot check doesn't replace the builder's own
  testing.

A `HOLD` stands until the same reader's later `READ-OK` (it is keyed by the
reader's account and signed name). A `HOLD <sha>` as the first line of a
comment on the Queue holds that head too. A HOLD closed in words only is
lifted by the PM with a line in `holds-lifted.tsv`, noted on the pull
request. **TWO READS** (in capitals), said in a comment's first line,
anywhere in a `READ ASK` or anywhere in the body, need two different
readers on the head, as does a Queue `READ ASK` naming the pull request
(its head or `#N` on the first line). Put `#N` or the full sha on the
ask's first line, and post one `READ ASK` per pull request when any of them is
ONE READ; the bot reads only the Queue's last 3 days (7 Nov, #5653).
A line opening
`ONE READ` then `:`, a dash, `(` or the line's end, outside quotes, fences
and indented code, opts out of a
mere mention in its own text only; a later `ONE READ` doesn't undo an ask. A pull
request that conflicts is told once per head: rebase it. A train is one
ARM64 run and then a rebase merge; there is one train open at a time, and a
pull request that fails gets `HOLD train N` and is left out. Pull requests
that landed by train show as **Closed**, not Merged, on GitHub.

**`NEXT TRAIN` is only an ask.** Say it on the Queue when a ready pull
request is waiting. The bot trains by itself; the PM unsticks it when a
marker is misread, and closes a train before its merge if a `HOLD` lands
after the train was cut. A builder never waits for a person to merge: it
rebases its pull requests and keeps going ("Never end your turn").

**Promotes** go to stable between **07:00 and 21:00 GMT+7**, and only the PM
promotes. A hotfix is promoted the same day, not batched; a clearly better
hotfix ships and the rest becomes a follow-up issue. The steps:

1. **Each builder checks its own change on its own lamp** (the lead, 4
   November). When the build N of a builder's train publishes (N is the
   build's feed sequence, the number in its release tag
   `lampos-lamp-voice-N`), the builder
   takes N on its lamp with its kit and slot, runs its rows plus the basics
   (every unit active, `listening_ready`, a spoken turn answered, not
   muted), and posts `PASS N #<pull request>` or `FAIL N #<pull request>`
   on #6811. When every lamp-visible pull request in N has a `PASS`, the PM
   promotes N; a `FAIL` holds N for that change. A docs-only or test-only
   pull request needs no lamp check. **Every `PASS` also covers the core
   rows:** a light colour change, a one-minute timer set and rung, "what
   reminders do I have?", "Stop" over the lamp's talk, and one ordinary
   question. A `FAIL` on a core row holds N for everyone. The promoted
   build is the one the builders' checks ran on, never a newer one.
   **A bug that reaches stable is rolled back within the hour:** the
   previous build is promoted back, and the fix ships the same day (the
   lead: a promoted build has no bug).
2. **Dispatch:** `gh workflow run lampos-arm64.yml --repo
   autonomous-ai/lampOS --ref lamp-voice -f promote_sequence=N`.
3. **Verify:** decode the base64 `manifest` in the `lampos-stable` release's
   `lampos-manifest.json` and check its sequence and commit.
4. **Notes:** add "What's new for you in build N" to the `lampos-stable`
   release notes (`gh release edit lampos-stable --notes-file`), right
   before "Built but not on yet": one plain bullet for each change a person
   would notice, checked against its switch's row; switches still off get
   none.
5. **Close the issues** the build carries to stable: search the open issues
   for its pull requests' numbers and read each one's last comment first.
6. **Post on #1711:** the build, its commit, what is in it and what is not,
   the checks and the figures.

**The figures.** The PM posts build % (the 76 build rows) and quality %
(17 rows, scored only from lamp runs on the product unit) on #1711 each
hour. A row moves on one lamp run on the product unit, on a build that
carries the change, scored by its effect on the row's own lines. A failure
that real people or a verifier see moves it down at once; a success moves
it up only with a played count. Scores: 0.5 works with failures a person
notices, 0.7 passes the played test with rare misses, 0.9 passes with real
people at the row's bar.

**Standing decisions.** Skills and connectors never depend on Harness;
there is no `programs-reply` for home accounts; customers speak English;
GitHub names roles, not people; quality comes before latency.

### The PM's checks (the first builder, on the lead's ask, 6 November 2026)

Five slips in the PM's first two days, each written here as a check so
none recurs:

- **A default-on pull request trains only after its lamp check.** The bot
  reads only `READ-OK`. So a pull request that turns a switch on in the
  product goes on `train-hold.txt` when it is first read, and comes off only
  when both are posted on its head: a `PASS` of its named lamp check, and the
  first builder's default-on `READ-OK`. On 5 Nov #5430, #5404 and #5331 merged
  before their checks, so build 10434 could not be the candidate and #5464
  had to turn four switches off.
- **Before naming N, read every pull request merged since the last
  stable.** That means every `HOLD`, P1 and P2 on it, reads after its merge
  included. Any open P1 or P2 holds N until its fix lands. On 5 Nov 10395 was
  named with an open P2 on #5414.
- **The figures, each hour,** on #1711, from `docs/build-rows.md` on
  `lamp-voice`. A rescore is posted there with its lamp run. The first
  builder regenerates the file (`build-rows-pr.sh` reads the review page,
  which stays off this public repo).
- **Check what a change can reach before setting its priority.** A shipped
  lamp takes a change only through the updater it has. Its first walk runs
  the build it shipped on (#5458 was set P1 for the ten lamps, which it could
  not reach).
- **Say whose decision it is.** The lead's words are relayed as said, and
  the PM's own calls are named as the PM's.

The first builder reads the PM's promote and gate calls for the next three
days, and posts on the Queue only where one of these is missed.

### PR markers (as the first builder ran them; the lead, 4 November 2026)

All of them are plain pull request comments, never GitHub reviews: the
train bot counts only comments (until #5212 lands).

- `READ ASK <sha>`: the builder asks for a read and names the reader. A
  reader who hasn't answered a `READ ASK` on the current head with a
  `READ-OK` or a `HOLD` within one hour is replaced (the lead's rule, 10
  Nov). The Reviewer-lead takes the read, saying "standing in for
  <role>", or hands it to a free reader, and the PM assigns reads the same
  way.
- `NEXT TRAIN`: only an ask, once a pull request is eligible.
- `READ-OK <full sha>`: the first line of the reader's comment. It carries
  over a rebase with the same patch.
- `HOLD <sha>`: with file:line and the fix. After the fix, the reader posts
  a new `READ-OK`.
- `TWO READS`: privacy and default-changing pull requests need two readers;
  the PM names the second.
- A lamp check: the tester posts the verdict with the journal excerpt and
  removes `needs:lamp-check`; at a promote, `PASS N #PR` or `FAIL N #PR`
  goes on #6811.
- "Done" is not a pull request marker: a Swarm agent sets its task done once
  its pull request has merged and its lamp check has passed. A Swarm reviewer
  also records its verdict as its review step's result.

## Lamp checks for code you can't test yourself (the lead, 4 November 2026)

For a change that runs on a lamp its builder cannot test: the coder's pull
request body has a **"Lamp check" section**, and a tester runs it.

- **The section holds:** the switches and the build (the pull request's
  head); the scene (the lines, the voices, and what the lamp must do); the
  exact journal lines that prove `PASS` or `FAIL`; and the product constants
  at `file:line`.
- **Testers are a pool.** Fifth, Sixth or Seventh on the lead's Mac (Fifth
  keeps scheduling lamp-52e6), or the holder of the lamp the change fits:
  Fourteen on 9adf, Friday, El or Jarvis on 06c1, Thirteen, Twelve or
  SubDarren on c0a4, and Mobile's person on 0c89.
- **Labels.** The PM sets `tester:<name>` on the pull request, by who is
  free and which lamp fits, and `needs:lamp-check`. `status:taken` stays
  with the coder.
- **The tester** builds the head, assembles the kit from `~/lamp-rows/KITS.md`,
  gets the lead's agent's yes on its sha (not needed on a builder's own
  in-house lamp; Fifth's GO on 52e6), posts
  PLAYING and QUIET on #6831, and then posts the verdict on the pull request
  with the journal excerpt for the scene's window, and removes
  `needs:lamp-check`. The excerpt has played clips only: no real person's
  words, no audio and no images.
- **At a promote** the tester posts `PASS N #PR` or `FAIL N #PR`, with the
  core rows, for a Swarm builder. Any builder whose pull request needs a
  lamp it does not hold uses this section and these labels.

## Working with the person beside the lamp

- **Before anything moves, say what will move and how far**, and what will
  be read. Then wait for the person to be ready.
- **Hands only for what needs hands:** an unplug, a push by hand, a scale.
  Do everything else yourself.
- Ask before a change on the lamp, and before anything the room would see
  or hear; a read-only read needs no ask ("Setting up a lamp").
- The person may ask for fewer runs than a task asks for (five unplugs, not
  ten). Do what they agree to and write the difference in the log.
- **Photographs only of the lamp**, with no person, face or screen.

## The arm hour (`tools/arm-hour.sh`)

A morning's rows on a real lamp, each with its run sheet's steps, run row
by row from the Mac by the builder at the keyboard, so the person at the
arm never reads a pull request (the first builder, #1722, 20 Oct 2026). The
rows are the table at the top of the script; `--dry` prints them, and
`lampOS/tools/arm-hour-test.sh` tests it with no lamp.

- **What runs where:** `lamp` steps on the lamp through `ARM_LAMP`, `mac`
  steps on the Mac, `trial` steps as `tools/trials.sh` rows on the lamp from
  `ARM_TRIALS` (the build's own copy, with its `skills/` beside it). `ARM_LAMP`
  and `ARM_TRIALS` are the builder's own command and path: the script holds
  no address and no credential. `ARM_LAMP_HOST` (`user@host`) is the lamp
  for a file sent from the Mac and for `presence-check.sh`; it is the
  builder's own too, with no default, and `--preflight` names it when a row
  needs it unset.
- **`--preflight` first, read-only:** each running part's build, the voice's
  end-of-speech worker, then one line a row, ready or what is missing. It
  runs none of a row's steps.
- **Nothing moves without a hand:** a row that moves the body asks "Hand near
  the arm? (y)", and anything but `y` runs nothing.
- **Every exit puts the lamp back:** a pass, a fail, a failed step, `q`, the
  input ending or ^C runs the row's put-back before the next row, once. A
  stopped `trials.sh` row is ended on the lamp and its own put-back runs.
- **The log** (`ARM_LOG`) is one line a row, to post from. Proofs go to the
  screen only.

## The voice to build and test with (the owner, 19 Oct 2026)

Every builder builds and tests the voice on the second route
(`LAMP_VOICE_ROUTE=second`, [second-voice.md](second-voice.md)): Deepgram
for speech to text, DeepSeek for chat and search, and ElevenLabs
(`eleven_v4_turbo`) as the lamp's one voice. Gemini Live costs too much for a
lamp people talk to all day.

- **Played tests, scenes and checks of the voice run on the second route.**
  A voice feature is done when it works there. One that works only on Live
  is not done.
- **No Live sessions on a lamp,** except a comparison the first builder names
  on #1712, the issue where the flow is built.
- **A suite, tool or script that starts a Live session** is named on the
  Queue (#6811), and it is moved to the second route.
- **Live stays a setting** (`LAMP_VOICE_ROUTE=gemini`) for a lamp that is
  set up with it, not the lamp to build against.

## Facts every lamp builder needs

Measured on lamp-52e6 on 2026-10-14 ([body.md](body.md), the
[fifth builder's log](https://github.com/autonomous-ai/lampOS/blob/lamp-fifth/lampOS/docs/work-log-fifth.md)). Another lamp may differ in its
numbers, not in kind.

- **After power returns every servo reads torque 0 and goal 0.** Writing
  torque on without writing the goal first sends a joint to 0. Write the
  goal (the position read) first.
- **A goal written while torque is off turns torque back on.**
- **A power cut moves the lamp.** The head droops about 250 counts toward
  the base. A cut while standing leaves the shoulder up (held by its gears)
  and the elbow folded.
- **The servo gains held** are the old runtime's: P 16/16/32/16/32, D 32,
  I 10 on servos 3 and 5.
- **The servos' own protection** is 80% load for 2 s, then 20% force.
- **`systemctl stop lamp-body` folds the lamp first.** A body that is
  killed leaves the lamp where it is, with torque as it was.
- **The servo port appears about 14 s after power.** `lamp-os` starts about
  11 s after the kernel.
- **Write down and sync** before any power test (above).

## Hardware members (production testing, #1700, 19 Oct 2026)

Hardware staff test lamps before they ship, each with their own agent. The
production testing plan and its deliverables are
[#1700](https://github.com/autonomous-ai/lampOS/issues/1700).
How the hardware side works, start to finish, is
[hardware-process.md](hardware-process.md), and its tasks are ordered on the
Hardware Queue, [#1764](https://github.com/autonomous-ai/lampOS/issues/1764),
kept by `hw-first` ([builders/hw-first.md](../builders/hw-first.md)).

- **Their own GitHub account.** Each hardware member joins with their own
  account, which a repository admin grants (read the queue, comment, push
  their own branches). Access is taken away the same way. Labels route work;
  they don't control access.
- **A role file** from [builders/HARDWARE-TEMPLATE.md](../builders/HARDWARE-TEMPLATE.md):
  the agent's name, the person's role, the stations and bench lamps, and
  where the fleet record lives. The person's account is mapped to the role in
  the fleet record, never in this repository.
- **A queue of their own.** Hardware issues are labelled `team:hardware` (a
  label made when the hardware lead is named). Software builders skip them,
  including when they look for an unowned issue.
- **The hardware queue is the hardware lead's** (the project lead, 19 Oct
  2026, #1711), the same shape as the software side: a builder proposes, an
  approver decides. Hardware builders post proposals and faults
  (`<name>-P<n>`, `Found by <name>`) on #1700 or their hardware issue. The
  hardware lead (or the hardware lead's agent) approves or declines them,
  and opens, labels, assigns and closes `team:hardware` issues: one for each
  confirmed common failure, with the affected lamps and runs linked.
  Routine results stay in the fleet record, never one issue per run. A
  defect closes on repair evidence plus a verified retest.
- **The first builder comes in only for software:** when a hardware finding
  needs a software change, the hardware lead posts a linked proposal on the
  Queue (#6811) and the first builder assigns it to a software builder.
- **The hardware lead** (a person) approves the suite's limits, durations,
  fixtures and stop conditions, and the shipment gate. A changed limit is a
  versioned decision.
- **The suite and the record:** the coverage matrix, its file
  (`production/matrix-v<N>.toml`), the station tool `lamp-station` and the
  record each run leaves are in [production-testing.md](production-testing.md).

## What was not shown, and open questions

- Builders with and without a lamp, and with different people, have
  started from this document since 2026-10-15 (role files in
  [builders/](../builders/)).
- `fifth-P1` to `fifth-P8` are answered and written in above (`fifth-P8`,
  only the first builder opens issues, accepted by the first builder on
  2026-10-16 11:38).
