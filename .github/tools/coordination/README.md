# Coordination tools

The first builder's tools for merging, reading asks and publishing the build rows.
They ran from one Mac. Now any operator can run them from a checkout of this repo.
They are zsh and Python scripts. They use `gh`, `git`, `jq`, and BSD `date` (macOS).
They live in `.github/tools/`, not `lampOS/`, which takes no Python scripts (`lampOS/AGENTS.md`, `lampOS/docs/joining.md`).

Each script keeps its dated comments. They are the history of each rule.

## The tools

| Tool | What it does |
|---|---|
| `train-bot.sh` | Builds merge trains onto `lamp-voice`, back to back, in two lanes (see "Two lanes" below). It picks every eligible PR, cherry-picks its commits, pushes `first/train-N` (a no-crates train: `first/train-nc-N`), opens the train PR, waits for the checks and merges by rebase. `DRY=1` prints each eligible PR's lane and what one round of each lane would carry, and stops. |
| `merge-when-green.sh PR [--behind-unrelated] [--no-crates] [--precheck PR...]` | Used by the bot. Waits for a PR's checks, then merges it by rebase, pinned to its head, only if it is green and contains `lamp-voice`'s tip. Green: at least two ARM64 build-and-test and clippy checks, every one completed with `SUCCESS`; any other completed conclusion (a null one too) fails it. A PR no file of which the ARM64 run takes (read from `lampos-arm64.yml`'s own `pull_request` paths on the PR's head) waits instead for every check that starts on it; one whose files no workflow takes (skipped docs, `lampOS/builders/`) waits for none. `--no-crates` (the no-crates lane) passes `--no-crates` to `behind-unrelated.sh`. `MWG_POLL` sets the seconds between looks (default 30). With `--behind-unrelated` (the bot passes it), a PR behind `lamp-voice` goes on waiting and merges as it is when `behind-unrelated.sh` says so; a merge as is that GitHub refuses exits 3, not 4. A merge GitHub answers with `HTTP 5xx` is looked at (a 5xx can follow a merge that went through), then after `MWG_RETRY_S` (default 30) the tip and the precheck are read again and it is tried again, at most 3 tries. Three 5xx exit 7, so the bot builds the same PRs again, and stops after two such trains in a row (train 662, 5 Nov). A gh timeout or reset names no 5xx, so it stays a refused merge. With `--precheck`, it runs `still-eligible.sh` on those PRs just before merging. `DRY=1`: waits and decides as usual, but skips `lv-quiet.sh` and the merge and prints what it would do; `LV_AT=<sha>` (dry only) stands in for `lamp-voice`'s tip, to replay a past case. Exit codes: 0 merged, 2 failed, 3 behind `lamp-voice`, 4 refused, 5 not done in 45 min, 6 precheck failed. |
| `behind-unrelated.sh [--no-crates] TRAIN_HEAD [LAMP_VOICE]` | Used by `merge-when-green.sh --behind-unrelated`. Read-only. Prints `merge as is: ...` (exit 0) or `rebuild: <reason>` (exit 1); see "A green train behind lamp-voice" below. |
| `still-eligible.sh PR...` | Used before a merge. Fails if any carried PR is now held (on the PR or on the Queue), unread, short of a TWO READS ask, or can't be fetched. |
| `lv-quiet.sh` | Used by `merge-when-green.sh`. Waits (at most 45 min) until no `lamp-voice` push build is still running. |
| `markers.py [--writers FILE] [--held] [PR]` | Reads a PR's comments and review bodies as JSON lines `{login, body}` on stdin. Prints `HELD`, `READ-OK <sha>` or nothing. With `--held`, a held PR that has a READ-OK prints `<READ-OK sha> <key>=<commit> …`, one per standing HOLD (the train bot logs it once per head), else nothing. With `--ignored`, it prints `READ-OK <sha7> by <login> ignored: not a writer` for each READ-OK skipped because its login isn't a writer (the train bot logs each once per head). `markers.py --merge COMMENTS REVIEWS` merges the two fetches into one list in time order (`lib.sh`). |
| `reads2.py [--writers FILE] [--queue QUEUE_JSONL --pr N] HEAD_SHA [EARLIER_HEAD...]` | Reads the PR body, comments and review bodies as JSON lines, and the Queue's comments with `--queue`. Prints `none`, `met N` or `short N` for a TWO READS ask. A READ-OK for an earlier head given (the bot gives those with the same patch) counts as one for the head. |
| `qholds.py [--writers FILE] HEAD QUEUE_JSONL` | Reads the PR's comments and review bodies as JSON lines `{login, body, created_at}` on stdin, and the Queue's from the file. Prints the time of a Queue HOLD that holds this head, or nothing. |
| `lib.sh` | Sourced by the scripts: the writers list and the fetches, each failing closed, and the build-input and lane tests (`coord_input`, `coord_lane`), read from `lampos-arm64.yml`'s and `lampos-sdk.yml`'s `pull_request` paths. |
| `lane.sh PR...` | Prints the lane each PR goes to, as the bot decides it. Read-only but for local fetches. |
| `tests/test_markers.py` | Tests for the three readers. Run `python3 tests/test_markers.py` from this folder. |
| `tests/test_behind_unrelated.py`, `tests/test_lanes.py` | Tests for `behind-unrelated.sh`, `merge-when-green.sh`, the lanes and the bot's dry round, on a temporary repository with a stand-in `gh`. Run all: `python3 -I -m pytest -q .github/tools/coordination/tests` from the repo root. |
| `asks-for-me.sh [HH:MM]` | Prints, in full, every comment on the Queue (#3997, #4710, #5385) and the lead's channel (#1711) that names the first builder. With no time it starts 5 min before its own last run. |
| `build-rows-pr.sh` | Regenerates `lampOS/docs/build-rows.md` from the review page. If it changed, it pushes a branch `first/build-rows-MMDD-HHMM` and opens a docs-only PR. |
| `gen-build-rows.py BUILD_PY OUT_MD LABELS_JSON PRIVATE_NAMES` | The generator `build-rows-pr.sh` calls. |

The review page itself (`build.py`) is not in this repo.
It is written to the owner and holds private notes and photo paths.
`gen-build-rows.py` reads only its `PROGRESS` and `BUILD` tables.
Get a copy from the first builder and point `ROWS_SOURCE` at it.

## Environment

| Variable | Default | Used for |
|---|---|---|
| `TOOLS_DIR` | the script's own folder | finding the other tools |
| `LAMP_COORD_STATE` | `~/.lamp-coordination` | all mutable state (below) |
| `REPO_DIR` | the repo holding `TOOLS_DIR` | the checkout `git fetch` and `gh` run in |
| `WORKTREE` | `$LAMP_COORD_STATE/wt-trainbot` | the worktree trains are built in |
| `WORKTREE_NC` | `$LAMP_COORD_STATE/wt-trainbot-nc` | the worktree no-crates trains are built in |
| `NOCRATES_LANE` | `on` | `off`: one lane, the bot as before part 3 of #5212 |
| `ROWS_WORKTREE` | `$LAMP_COORD_STATE/wt-rows` | the clean worktree the rows branch is made in; added if missing |
| `ROWS_SOURCE` | `$LAMP_COORD_STATE/review-page/build.py` | the review page |
| `PRIVATE_NAMES` | `$LAMP_COORD_STATE/private-names.txt` | names the rows file must never hold |
| `HOLDS_LIFTED` | `$LAMP_COORD_STATE/holds-lifted.tsv` | HOLDs lifted in words |
| `PYTHON` | `python3` | running `markers.py`, `reads2.py`, `qholds.py`, `gen-build-rows.py` |
| `LAMP_COORD_WRITERS` | none | the writers file for `markers.py`, `reads2.py` and `qholds.py` run by hand (the bot passes `--writers`) |

`REPO_DIR`'s `origin` must be `autonomous-ai/lampOS`.
`WORKTREE` and `WORKTREE_NC` must not be `REPO_DIR`, its top level, the repository's main worktree or each other: the bot refuses to start.

## State

State never lives in the repo. It lives in `$LAMP_COORD_STATE`:

- `train-bot.log`: the bot's log. `train-bot.out` and `train-bot.pid`: its output and process id, when started as below.
- `train-bot.told`: what the bot has already said, so it says each thing once (its last 5000 lines are kept).
- `train-bot.said`: fetch failures already logged; a line goes when that fetch works again.
- `pr-trainN.md`: each train PR's body.
- `train-first.txt`: PR numbers that go first, in that order.
- `train-hold.txt`: PR numbers that sit out.
- `train-only.txt`: while it exists, a train carries only the PRs it names.
- `holds-lifted.tsv`: HOLDs lifted in words. See `holds-lifted.tsv.example` for the format.
  Its keys hold account logins, so it stays out of the repo. Copy the first builder's file.
- `private-names.txt`: one regex a line. See `private-names.txt.example`. `build-rows-pr.sh` stops without it.
  `gen-build-rows.py` also leaves out of the Owners column any builder label a rule matches.
  A `builder:*` label can read like a person's name: add a rule for it.
- `writers.txt`: the logins with write access to the repo, fetched from its collaborators.
  Its first line is `# fetched <epoch>`. It is fetched again when an hour old.
- `writers-extra.txt`: logins the operator adds, one a line. Optional.
- `writers-allowed.txt`: the two above together, the file the readers get.
- `queue.jsonl`, `pr-comments.jsonl` (comments and review bodies), `pr-body.jsonl`: the bot's last fetches.
- `asks-last`: the time of `asks-for-me.sh`'s last run.
- `labels.json`, `gen/`: `build-rows-pr.sh`'s work files.

## Running the train bot

Run only one bot. Start, stop or restart it only when no `first/train-*` PR is open:

```sh
gh pr list -R autonomous-ai/lampOS --base lamp-voice --state open --json headRefName \
  --jq '.[]|select(.headRefName|startswith("first/train-"))|.headRefName'
```

Look first with a dry run:

```sh
DRY=1 .github/tools/coordination/train-bot.sh
```

Start:

```sh
S="${LAMP_COORD_STATE:-$HOME/.lamp-coordination}"; mkdir -p "$S"
nohup .github/tools/coordination/train-bot.sh > "$S/train-bot.out" 2>&1 &
echo $! > "$S/train-bot.pid"
```

Stop: `kill "$(cat "$S/train-bot.pid")"` (it stops both lanes). Restart: stop, check no train is open, start.

The bot stops by itself (exit 2) when two crate trains in a row time out, or a crate run fails with no failing test named.
It says so on the Queue (#5385, the last issue in lib.sh's `COORD_QUEUE_ISSUES`).
If the Queue refuses the post (an issue takes at most 2,500 comments), the bot logs it instead.
The no-crates lane stops alone (the file `train-bot.stop-nc` in the state folder) when two of its trains in a row time out or GitHub refuses its merge; delete the file, or restart the bot, to start it again.

## A green train behind lamp-voice

A train that went green while `lamp-voice` moved used to be closed and built again (about 30 min).
Train 597 (#5201) was thrown away for `f5f3dda32`, which changed only `robots/lamp/hardware/test/tools/factory-run.sh`.

Now `behind-unrelated.sh` decides, from the train's base (its merge base with `lamp-voice`).
The train merges as it is, by rebase and pinned to its head, after the same precheck, only when both hold:

- the files `lamp-voice` gained and the files the train changes share none (a rename counts as both paths);
- no gained file is a build input: nothing under `lampOS/crates/`, no `Cargo.*` anywhere, nothing under `.github/workflows/`, and no other file the lampOS ARM64 run takes (`lampOS/**` except the docs' Markdown and `lampOS/builders/`, with the docs `lampos-arm64.yml` takes back in, plus its `.github/scripts/lampos-*` and `.github/tools/lampos-promote/`).

The crates' tests read files in `lampOS/deploy`, `lampOS/skills`, `lampOS/etc` and those docs, so a gain there rebuilds.
Anything else rebuilds as before, and so does any error (fail closed).

A train GitHub already shows as merged is never rebuilt, whatever `merge-when-green.sh` answered: train 761 (#5904) merged and was answered as behind, so its PRs were left open. The bot then closes the carried PRs as after any merge.

Replay a case without changing anything:

```sh
DRY=1 .github/tools/coordination/behind-unrelated.sh <train head> <lamp-voice then>
DRY=1 LV_AT=<lamp-voice then> .github/tools/coordination/merge-when-green.sh <train PR> --behind-unrelated
```

For train 597: `DRY=1 LV_AT=f5f3dda325 .github/tools/coordination/merge-when-green.sh 5201 --behind-unrelated` prints
`dry: merge as is: gained 1 files since 762487b1d (robots/lamp/hardware/test/tools/factory-run.sh), none overlapping the train's 35, ...`.

## Two lanes

Docs-, tools- and other PRs that change no build input used to wait behind 20-minute crate trains (#5212 part 3).
The bot now sends each eligible PR to exactly one lane, by its files (`lib.sh`, `coord_lane`; `lane.sh` shows it):

- **The no-crates lane** takes a PR none of whose files is a build input: nothing under `lampOS/crates/`, no `Cargo.*`, nothing under `.github/workflows/`, and nothing else the ARM64 run or the SDK check takes, any path their `pull_request` paths add included (the same test as "A green train behind lamp-voice" above). Its trains are `first/train-nc-N`. They get no ARM64 or clippy run, so they merge once the checks that do start on them pass, in minutes; a train only of docs no workflow takes merges with none.
- **The crate lane** takes every other PR, mixed ones too, as before: `first/train-N`, one ARM64 run. The SDK (`lampOS/sdk/`) and the docs `lampos-sdk.yml` takes are inputs, so their PRs go here.

The lanes run side by side, one train each, in their own worktrees, and neither takes the other's PRs.
Neither rebuilds the other: each merges with `--behind-unrelated`, pinned to its head.
A no-crates merge moves `lamp-voice` by non-input files only, so a crate train behind it merges as it is unless they share a file.
A no-crates train passes `--no-crates`: a train that itself changes no input merges as it is behind gained inputs too, since none of its checks reads them.
It still rebuilds on a shared file, on a gained workflow, and, when it changes a `lampOS/docs` file, on any gained `lampOS/docs` file (the doc-links check reads them all).
A merge GitHub refuses (the other lane moved `lamp-voice` first) rebuilds.

A failed no-crates train has no failing test to name: alone in it, the PR gets `HOLD train nc-N` (a new READ-OK clears it); with others, the lane carries one PR a train until a train merges (`train-bot.nc-one` in the state folder).

`NOCRATES_LANE=off` turns the lane off: one lane carries every PR, as before.

```sh
DRY=1 .github/tools/coordination/train-bot.sh   # each PR's lane, then what each lane would carry
.github/tools/coordination/lane.sh 5286 5289      # the lane of any PR, eligible or not
```

## Marker rules

These are what `markers.py`, `reads2.py`, `qholds.py` and `train-bot.sh` do.

**Accounts.** Only comments from logins with write access count: `writers.txt` plus `writers-extra.txt`.
A READ-OK, a HOLD, lifting words, a TWO READS comment or a Queue HOLD from any other login is skipped.
The PR body's TWO READS ask (a line that asks it, below) always counts.
If the collaborators can't be fetched and `writers.txt` is an hour old or more, no PR is eligible (fail closed).
A PR whose head is in another repository is never carried.

**Reviews read as comments.** A GitHub review's body counts exactly as a comment does: same writers, same marker rules,
in time order with the comments (its submit time), so a later HOLD beats an earlier READ-OK whichever kind each is.
A review's state (APPROVED, CHANGES_REQUESTED, ...) is never a read. A review with an empty body, not yet submitted, or dismissed counts for nothing.

**Fetches fail closed.** If a PR's head, comments, reviews or body can't be fetched, that PR is not eligible.
If any Queue issue (#4710 or #5385) can't be fetched, no PR is eligible that round.
`still-eligible.sh` rechecks the markers, the Queue HOLDs and TWO READS just before a train merges.

**Markers.**

- A marker opens the comment: its first line is `READ-OK <sha>` or `HOLD <sha>` (bold and backticks allowed; a `#PR` may come before the sha).
  A HOLD with no sha still holds. A READ-OK with no sha makes the PR not eligible.
- A marker later in the first 400 characters counts only with a commit of 9 or more characters right after it.
  It does not count when it is quoted (backticks or quotation marks), in brackets, after a possessive ("the Reviewer-lead's READ-OK …"),
  or named as a noun ("your HOLD …", "the/that/this/their/for …").
- A marker on a Markdown quote line (`> READ-OK …`, on any line, the first too) or inside a fenced code block (```` ``` ```` or `~~~`) never counts.
- The signature is the reader's name: "Sixth, 09:16" (an optional "The"), the name right after the marker's commit,
  the name that opens the next line ("Sixth: …"; a capitalised word or "<word> builder", never a label such as "Tests:", "Why:" or "Fail-first:"),
  or the name that opens the comment with a colon.

**What markers.py prints.**

- The PR is eligible on its newest READ-OK when no HOLD stands.
  A READ-OK counts for the PR's head, or for a head with the same patch-id (a plain rebase). The patch-id is taken with no context lines. This holds for TWO READS too.
- A signed HOLD is keyed on the login plus the signed name. It stands until the same login and name post a later READ-OK.
  Words alone ("this clears my HOLD" in a comment that is not a READ-OK) never lift it.
- An unsigned HOLD stands until that login posts a later READ-OK whose first 400 characters say the HOLD is lifted or cleared
  ("HOLD lifted", "clears my HOLD", "lifts my 16:45 HOLD", "My HOLD on <full commit> is lifted").
  Lifting words that name a commit clear only that login's HOLDs on that commit (#5534).
- An unsigned READ-OK never lifts a signed HOLD, even the only one on its commit: one account may sign for two readers,
  and words that can't say whose lift neither reader's HOLD (#5542).
- Lifting words with "not", "n't" or "never" in them or right before them ("is not lifted", "won't lift my HOLD") lift nothing.
- A HOLD on A carried over a rebase as a HOLD on B, then "my HOLD on B is lifted", leaves A standing (fail closed); `--held` names it.
- The bot's own `HOLD train N` clears on any writer's later READ-OK marker that names a commit. A quoted READ-OK does not clear it.
- Any HOLD closed in words but never cleared by a READ-OK as above is lifted only by a row in `holds-lifted.tsv`.
  A row's key is `<login>/<signed name, lowercase>`, `<login>/unsigned#<N>` (N counts the PR's comments from 0; reviews never count),
  or `<login>/unsigned-review#<submitted_at>` for an unsigned HOLD posted as a review.
  Post the reader's closing comment on the PR when you add the row.

**Queue HOLDs.** A Queue comment of the last 3 days, on #4710 or on #5385 (lib.sh's `COORD_QUEUE_ISSUES`, the one place the Queue's issue numbers live; #4710 hit GitHub's 2,500-comment limit on 5 Nov), whose first line is `HOLD <sha>` holds the PR whose head starts with that sha.
**When the Queue fills** (2,500 comments): open the next Queue issue and append its number to `COORD_QUEUE_ISSUES` in `lib.sh`, last.
Keep the older ones while their last 3 days of HOLDs matter; the bot posts on the last one.
It stands until the same login posts a later READ-OK marker on the PR whose first 400 characters say the HOLD is lifted or cleared
(the same words as above; `**READ-OK**` counts).

**TWO READS.** A PR body, a writer's comment whose first line opens with `TWO READS`, or a writer's `READ ASK` comment (its first line carries `READ ASK <sha>`) asks for two readers when it names `TWO READS`, in capitals, anywhere (7 Nov, #5635: fail closed; #5580 merged on one read with the words in its body's prose). A false match only costs a second read. A writer's `READ ASK` on the Queue asks too, when its first line opens with `READ ASK` (or has it right after a signature's colon, comma or dash) and names the PR, by a commit its head or a same-patch head starts with, or by `#N`, and it names `TWO READS` with no opt-out (7 Nov: #5647 and #5650 were asked only there and merged on one read). A Queue comment that only mentions a READ ASK in its prose asks nothing. 7 Nov, El's read of #5653: put `#N` (or the sha) on the Queue ask's first line; a URL with no `#`, or `#N` on line two, asks nothing. The opt-out is read over the whole comment, so one `ONE READ:` line releases every PR in a multi-PR ask: post one READ ASK per PR when any PR in it is ONE READ. A Queue ask's first line may have `READ ASK` right after a signature's comma (`Sixth, READ ASK to …: #N`) or a dash with a space on each side, em, en or hyphen (`Twelve — READ ASK #N`, `Second - READ ASK #N`; 7 Nov, #5653's P3s); a hyphen inside a word (`pre-READ ASK`) is no signature. Any colon, comma or spaced dash counts as a signature's, so prose like `Faye, 07:28, the hour: READ ASK #5653 was posted, TWO READS.` holds #5653; it fails closed and costs at most a second read. A Queue ask by sha counts when the sha is any commit of the PR (`git rev-list origin/lamp-voice..HEAD`), or an earlier head with the same patch (a plain rebase); that only widens the asks, never which READ-OKs count. The same-patch heads come from the shas on the Queue's asking first lines (`reads2.py --queue-shas`, the regex `queue_asks` reads), so `coord_same_patch` and `reads2.py` take the same asks. Give the full 40-character sha (or `#N`): git can't fetch an abbreviated sha, so a short sha of a head a rebase left behind resolves only on a checkout that already has it, and then asks nothing. When the PR's commits can't be listed, the PR sits out the round (`#N left out: its commits can't be listed`) and `still-eligible.sh` fails: the head alone would miss an ask at an older commit. The bot reads only the last 3 days of the Queue (`coord_queue`), so an older ask is not seen: ask again, or on the PR. A line opening with `ONE READ` (after an optional heading, list or bold mark) followed by `:`, a dash not followed by a letter (`ONE READ-OK` is no opt-out), `(` or the line's end opts out of a mere mention in its own text only: a body's opt-out doesn't clear a READ ASK's mention. A quoted line (`> ONE READ: …`), one indented by four spaces or a tab (a code block), or one inside a ``` or ~~~ fence opts nothing out; a fence closes only on a line of its own character, at least as long, with nothing after it. An HTML comment opts out only when `ONE READ:` opens a line of its own inside it (`<!--`, then `ONE READ: …`, then `-->` on separate lines), though it doesn't show in the rendered body: it is the author's own text. A one-line `<!-- ONE READ: … -->` opts nothing out. An ask stays once made: a later comment's `ONE READ:` doesn't undo it, so edit the ask out instead. It never clears a line that asks: one opening with `TWO READS` followed by `:`, a dash, `(` or the line's end (`## TWO READS`), or `TWO READS:` with readers after it (`Privacy. TWO READS: Reviewer-lead, Tests.`). The bot says an opted-out body in its log once per head (`reads2.py --unasked`).
Then two different people need a READ-OK marker for a commit the head starts with.
A person is a login plus the name it signs, not a login alone.
Several builders share one GitHub account, so counting logins would make a legitimate two-reader PR impossible.
An unsigned READ-OK never adds a second person to a login: it merges into that login's signed reader,
and a login whose READ-OKs on the head are all unsigned is one reader.

**Trains.**

- A PR whose own ARM64 run on its head is red sits out.
- A PR stacked on another open PR sits out until that PR is on `lamp-voice`.
- No merge commits: the bot cherry-picks with `--no-merges` and merges by rebase.
- Each lane runs one train at a time: the no-crates lane waits while a `first/train-nc-*` PR is open, the crate lane while any other `first/train-*` PR is open. With `NOCRATES_LANE=off`, one lane waits while any `first/train-*` PR is open.
- A train of rows PRs alone waits for a code PR, or until the oldest is 3 h old, in the crate lane and with `NOCRATES_LANE=off`. In the no-crates lane it goes at once: no code PR rides there, and a crate train merges with `--behind-unrelated` when lamp-voice gained only files it doesn't touch.

## Promoting a build to stable

Promote only between 07:00 and 21:00 (GMT+7).

1. Run the pre-promote check on a test lamp with that build.
2. Start the promotion of lamp-voice sequence `N`:

   ```sh
   gh workflow run lampos-arm64.yml --repo autonomous-ai/lampOS --ref lamp-voice -f promote_sequence=N
   ```

3. When the run is done, check the stable feed names `N`. Decode the base64 `manifest` field of the
   `lampos-stable` release's `lampos-manifest.json`:

   ```sh
   gh release download lampos-stable -R autonomous-ai/lampOS -p lampos-manifest.json -O - \
     | python3 -c 'import json,base64,sys; m=json.loads(base64.b64decode(json.load(sys.stdin)["manifest"])); print(m["sequence"], m["commit"])'
   ```

4. Write the release notes. Take the current notes, insert the new notes just before
   `### Built but not on yet`, and save them back:

   ```sh
   gh release view lampos-stable -R autonomous-ai/lampOS --json body -q .body > notes.md
   # edit notes.md
   gh release edit lampos-stable -R autonomous-ai/lampOS --notes-file notes.md
   ```

## Build % and quality %

`gen-build-rows.py` computes both from the review page's tables.

- Build % is the mean of the `BUILD` rows' scores (0 to 1), times 100.
  The mood rows are left out and shown on their own line, for later.
- Quality % is the mean of the `PROGRESS` rows' current scores, times 100.

`build-rows-pr.sh` writes them into `lampOS/docs/build-rows.md` and opens a docs-only PR when the file changed.
It stops if the file would hold a name from `private-names.txt` or a note written to the owner.
It also stops if its commit would hold any file but `lampOS/docs/build-rows.md`.
