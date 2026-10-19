# Joining as a builder

Proposed by the fifth builder on 2026-10-14 (`fifth-P5` in its
[log](work-log-fifth.md)). This is the one document a new builder reads.
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
- **The first builder** ("main agent") is the one with lamp-4ace. It owns
  `lamp-voice`, the list in work-split.md, and the answers.

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
   `lamp-<name>`, and comments on the Queue issue (#705): "new builder
   <name>, role file at <link>, no tasks yet". It takes no task until the
   first builder has taken the file onto `lamp-voice`, corrected where it
   must be, and labelled issues `builder:<name>`.
3. Reads this document, `lampOS/AGENTS.md`, and the repository's
   `CLAUDE.md`.
4. Reads `lampOS/docs/work-split.md` as it is on `origin/lamp-voice`:
   "Rules for all" and "What the lamp showed".
5. Reads its own log on its own branch, newest first.
6. If it has a lamp: that lamp's `deploy/<lamp>/LAMP.md` and `README.md`.
7. Finds its next task: the first issue in its own list in the pinned
   [Queue issue](https://github.com/autonomous-ai/lampOS/issues/705) that
   is open and not `status:taken` (`gh issue view 705`;
   `gh issue list --label builder:<name> --state open` shows them all),
   claimed as "Claiming an issue" says. It reads the comments on its
   issues: answers come there.

## Setting up a machine

- **Rust.** `rustup` puts `cargo` in `~/.cargo/bin`, which a fresh shell
  may not have on its PATH. If `cargo` is not found:

  ```sh
  export PATH="$HOME/.cargo/bin:$PATH"
  ```

  The toolchain is pinned by `lampOS/rust-toolchain.toml`.
- **The lamp build.** The lamp is an Orange Pi running Debian bookworm
  (glibc 2.36, arm64). Build for it in Docker, from `lampOS/`:

  ```sh
  docker run --rm --platform linux/arm64 \
    -v "$PWD":/src \
    -v lampos-cargo:/usr/local/cargo/registry \
    -v lampos-target:/src/target \
    -w /src rust:1.96.0-bookworm \
    cargo build --release -p <crate>
  ```

  The two named volumes keep the registry and the build between runs. The
  binary is in the `lampos-target` volume, under `release/`. The first
  builder's own cross-build is in [brain-build.md](brain-build.md).
- **The Linux build shows warnings the Mac does not** (code behind
  `cfg(target_os = "linux")`). Build for the lamp before saying a crate is
  clean, and write any such warning in the log.
- **GitHub.** `gh auth status` must show it signed in to the repository.
  Every builder works through the same account.

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
- **Ask the person before every `ssh`** (the repository's `CLAUDE.md`), and
  before anything a person in the room would see or hear.
- **The owner file.** Each lamp's folder `deploy/<lamp>/` holds a `LAMP.md`
  of three lines, kept current by the one builder who owns it, so any agent
  can tell from Git alone whether it may act on a lamp and what is on it
  (`fifth-P2`):

  ```text
  owner: fifth            # the only builder that may ssh to it or install on it
  runs: lamp-os 0.1.0-fifth, body 91a9d458, voice 4cd315ee, eyes 2a109496
  since: 2026-10-14 14:34 +07, from lamp-fifth at <commit>
  ```

  An agent that finds no `LAMP.md`, or another owner in it, does not touch
  the lamp. A change of owner is a line in work-split.md by the first
  builder. A lamp lent for one test says so in `runs:` and goes back.
  [lamp-52e6's](../deploy/lamp-52e6/LAMP.md) is an example.
- **Before installing anything**, write down what the lamp runs:

  ```sh
  systemctl list-units 'lamp*' 'hal*'
  sudo fuser -v /dev/ttyACM0 /dev/video0
  ```

  and back up whatever you stop or replace under
  `~/<name>-backup-<date>/` on the lamp. Nothing of the old software is
  deleted.
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
   without building again.
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
- Your list in Queue #705 is kept at least three deep by the first builder.
  If it ever has nothing you can do now, say so on #705 with your name, and
  meanwhile take an unowned open issue in your parts (a flaky test, a doc
  that lies, a missing test) rather than wait.
- Post or push at least every 20 minutes while you work. The idle watch
  flags 20 minutes of silence, and the first builder answers it with a
  task.

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

**The switch is for that first minute, not for life** (the owner,
2026-10-15: "opt in is wasteful"; we use every sensor and part the lamp
has). Once a part has passed its minute and is switched on for lamp-4ace,
the same day it becomes the default in code, and the switch remains only to
turn it off (`off`). A part left off says why, in its doc, and the first
builder tries it within the week. Switches in place before this rule are
made defaults by the first builder as it confirms each on lamp-4ace.

### One pull request per task (`fifth-P6`, decided 2026-10-14)

Proposed by the fifth builder (#716) after #708 was closed unmerged: its
pieces had been cherry-picked an hour before, so nothing tied the issue to
the commits, the hashes differed from the builder's, and a piece's
dependency had to be found by hand. Decided: yes, but as follows.

- **A task branch,** `<name>/<task>`, cut from `lamp-voice` and kept rebased
  on it, carries only that task's code, docs and verification folder.
  `lamp-<name>` keeps the log and nothing a lamp runs.
- **Ready is a pull request** from `<name>/<task>` into `lamp-voice` that is
  not a draft. Its body, in this order: `Closes #<issue>`; its kind (off
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
  `lampOS:` commits, and a builder's code comes by no other path. Since
  2026-10-15 the **reviewer** ([reviewer.md](../builders/reviewer.md))
  reviews every ready pull request and merges the ones that are off unless
  set and pass its list; the first builder merges the rest after a minute
  on lamp-4ace. A change wanted is a review comment, and the builder
  pushes to the same branch. One that will not be taken is closed with the
  reason.
- **The builder watches its own pull requests** and answers their comments
  itself; a person is asked only for hands at a lamp or for permissions.
- **A merge does not close the issue.** GitHub acts on `Closes #N` only for
  merges into the default branch (`main`), and every pull request here
  merges into `lamp-voice`. Whoever merges closes the issue by hand, saying
  what was and was not delivered; the Queue gives the next task.

Pieces marked ready on a `lamp-<name>` branch before this was decided are
taken once more as before. A builder never pushes to `lamp-voice` or to
another builder's branch.

## Talking with the first builder

Everything goes through Git, once each way. No back-and-forth.

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

Proposed at the fifth builder's person's request. The first builder holds
the roadmap and the order; a builder or its person does not know what the
first builder wants next, so an issue opened by them is work the first
builder has to read, place or close before anyone may do it.

- **Issues and labels are the first builder's.** Only the first builder
  opens an issue and makes or moves a `builder:<name>` label. A builder
  does not open issues, does not make labels, and puts no `builder:` label
  on any issue, its own name included (`status:` labels on its own issues
  stay its own, as "Claiming an issue" says).
- **A problem found while working** is a comment on the issue it belongs
  to: the task you are on, or the issue of the part where it showed (for
  example a flaky test of another part's: a comment on that part's open
  issue). Only when no issue fits, it is one comment on the Queue issue
  ([#705](https://github.com/autonomous-ai/lampOS/issues/705)). Head it
  `<name>-P<n>` for a proposal or `Found by <name>` for a fault, with what
  was seen, how to see it, and what it would cost (and still a line in
  your log). The first builder answers there: it widens an issue, opens a
  new one with an owner and a place in the Queue, or says no. No answer
  means not taken up.
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
  `status:on-lamp`; a closed issue is a task on `lamp-voice`. GitHub's
  assignee cannot tell agents apart, since every agent works through the
  same account, so the name is a label.
- **Order:** one pinned "Queue" issue the first builder keeps in order,
  [#705](https://github.com/autonomous-ai/lampOS/issues/705), label
  `lampos-queue`. No priority labels.
- **Questions and answers** are comments on the issue they are about.
- **Pull requests** say `Closes #N` (closed by hand at the merge, above) or
  `Part of #N`. No GitHub Project is needed.

**Claiming an issue.** Take only an issue labelled `builder:<name>` with
your own name (`fifth-P8`, above). Skip any issue already `status:taken`. Add
`status:taken` and comment one line,
`taken by <name> · session <session id> · <lamp>`. Then read the comments
again: if the first "taken" comment is not your own, release the issue
(remove the label, say so) and take the next. That keeps two sessions given
the same name from working one task twice.

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

## Working with the person beside the lamp

- **Before anything moves, say what will move and how far**, and what will
  be read. Then wait for the person to be ready.
- **Hands only for what needs hands:** an unplug, a push by hand, a scale.
  Do everything else yourself.
- Ask before every `ssh`, and before anything the room would see or hear.
- The person may ask for fewer runs than a task asks for (five unplugs, not
  ten). Do what they agree to and write the difference in the log.
- **Photographs only of the lamp**, with no person, face or screen.

## Facts every lamp builder needs

Measured on lamp-52e6 on 2026-10-14 ([body.md](body.md), the
[fifth builder's log](work-log-fifth.md)). Another lamp may differ in its
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

- **Their own GitHub account.** Each hardware member joins with their own
  account, which a repository admin grants (read the queue, comment, push
  their own branches). Access is taken away the same way. Labels route work;
  they don't control access.
- **A role file** from [builders/HARDWARE-TEMPLATE.md](../builders/HARDWARE-TEMPLATE.md):
  the agent's name, the person's role, the account, the stations and bench
  lamps, and where the fleet record lives.
- **A queue of their own.** Hardware issues are labelled `team:hardware` (a
  label made when the hardware lead is named). Software builders skip them,
  including when they look for an unowned issue.
- **Issues** are still opened by the first builder: one for each confirmed
  common failure, with the affected lamps and runs linked. Routine results
  stay in the fleet record, never one issue per run.
- **The hardware lead** (a person) approves the suite's limits, durations,
  fixtures and stop conditions, and the shipment gate. A changed limit is a
  versioned decision.

## What was not shown, and open questions

- Builders with and without a lamp, and with different people, have
  started from this document since 2026-10-15 (role files in
  [builders/](../builders/)).
- `fifth-P1` to `fifth-P8` are answered and written in above (`fifth-P8`,
  only the first builder opens issues, accepted by the first builder on
  2026-10-16 11:38).
