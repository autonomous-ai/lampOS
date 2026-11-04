# Coordination tools

The first builder's tools for merging, reading asks and publishing the build rows.
They ran from one Mac. Now any operator can run them from a checkout of this repo.
They are zsh and Python scripts. They use `gh`, `git`, `jq`, and BSD `date` (macOS).
They live in `.github/tools/`, not `lampOS/`, which takes no Python scripts (`lampOS/AGENTS.md`, `lampOS/docs/joining.md`).

Each script keeps its dated comments. They are the history of each rule.

## The tools

| Tool | What it does |
|---|---|
| `train-bot.sh` | Builds merge trains onto `lamp-voice`, back to back. It picks every eligible PR, cherry-picks its commits, pushes `first/train-N`, opens the train PR, waits for the checks and merges by rebase. `DRY=1` prints what one round would carry and stops. |
| `merge-when-green.sh PR [--behind-unrelated] [--precheck PR...]` | Used by the bot. Waits for a PR's checks, then merges it by rebase, pinned to its head, only if it is green and contains `lamp-voice`'s tip. Green: at least two ARM64 build-and-test and clippy checks, every one completed with `SUCCESS`; any other completed conclusion (a null one too) fails it. With `--behind-unrelated` (the bot passes it), a PR behind `lamp-voice` goes on waiting and merges as it is when `behind-unrelated.sh` says so; a merge as is that GitHub refuses exits 3, not 4. With `--precheck`, it runs `still-eligible.sh` on those PRs just before merging. `DRY=1`: waits and decides as usual, but skips `lv-quiet.sh` and the merge and prints what it would do; `LV_AT=<sha>` (dry only) stands in for `lamp-voice`'s tip, to replay a past case. Exit codes: 0 merged, 2 failed, 3 behind `lamp-voice`, 4 refused, 5 not done in 45 min, 6 precheck failed. |
| `behind-unrelated.sh TRAIN_HEAD [LAMP_VOICE]` | Used by `merge-when-green.sh --behind-unrelated`. Read-only. Prints `merge as is: ...` (exit 0) or `rebuild: <reason>` (exit 1); see "A green train behind lamp-voice" below. |
| `still-eligible.sh PR...` | Used before a merge. Fails if any carried PR is now held (on the PR or on the Queue), unread, short of a TWO READS ask, or can't be fetched. |
| `lv-quiet.sh` | Used by `merge-when-green.sh`. Waits (at most 45 min) until no `lamp-voice` push build is still running. |
| `markers.py [--writers FILE] [PR]` | Reads a PR's comments as JSON lines `{login, body}` on stdin. Prints `HELD`, `READ-OK <sha>` or nothing. |
| `reads2.py [--writers FILE] HEAD_SHA [EARLIER_HEAD...]` | Reads the PR body and comments as JSON lines. Prints `none`, `met N` or `short N` for a TWO READS ask. A READ-OK for an earlier head given (the bot gives those with the same patch) counts as one for the head. |
| `qholds.py [--writers FILE] HEAD QUEUE_JSONL` | Reads the PR's comments as JSON lines `{login, body, created_at}` on stdin, and the Queue's from the file. Prints the time of a Queue HOLD that holds this head, or nothing. |
| `lib.sh` | Sourced by `train-bot.sh` and `still-eligible.sh`: the writers list and the fetches, each failing closed. |
| `tests/test_markers.py` | Tests for the three readers. Run `python3 tests/test_markers.py` from this folder. |
| `tests/test_behind_unrelated.py` | Tests for `behind-unrelated.sh` and `merge-when-green.sh --behind-unrelated`, on a temporary repository with a stand-in `gh`. Run all: `python3 -I -m pytest -q .github/tools/coordination/tests` from the repo root. |
| `asks-for-me.sh [HH:MM]` | Prints, in full, every comment on the Queue (#3997, #4710) and the lead's channel (#1711) that names the first builder. With no time it starts 5 min before its own last run. |
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
| `ROWS_WORKTREE` | `$LAMP_COORD_STATE/wt-rows` | the clean worktree the rows branch is made in; added if missing |
| `ROWS_SOURCE` | `$LAMP_COORD_STATE/review-page/build.py` | the review page |
| `PRIVATE_NAMES` | `$LAMP_COORD_STATE/private-names.txt` | names the rows file must never hold |
| `HOLDS_LIFTED` | `$LAMP_COORD_STATE/holds-lifted.tsv` | HOLDs lifted in words |
| `PYTHON` | `python3` | running `markers.py`, `reads2.py`, `qholds.py`, `gen-build-rows.py` |
| `LAMP_COORD_WRITERS` | none | the writers file for `markers.py`, `reads2.py` and `qholds.py` run by hand (the bot passes `--writers`) |

`REPO_DIR`'s `origin` must be `autonomous-ai/lampOS`.
`WORKTREE` must not be `REPO_DIR`, its top level or the repository's main worktree: the bot refuses to start.

## State

State never lives in the repo. It lives in `$LAMP_COORD_STATE`:

- `train-bot.log`: the bot's log. `train-bot.out` and `train-bot.pid`: its output and process id, when started as below.
- `train-bot.told`: what the bot has already said, so it says each thing once.
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
- `queue.jsonl`, `pr-comments.jsonl`, `pr-body.jsonl`: the bot's last fetches.
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

Stop: `kill "$(cat "$S/train-bot.pid")"`. Restart: stop, check no train is open, start.

The bot stops by itself (exit 2) when two trains in a row time out, or a run fails with no failing test named.
It says so on the Queue (#4710).

## A green train behind lamp-voice

A train that went green while `lamp-voice` moved used to be closed and built again (about 30 min).
Train 597 (#5201) was thrown away for `f5f3dda32`, which changed only `robots/lamp/hardware/test/tools/factory-run.sh`.

Now `behind-unrelated.sh` decides, from the train's base (its merge base with `lamp-voice`).
The train merges as it is, by rebase and pinned to its head, after the same precheck, only when both hold:

- the files `lamp-voice` gained and the files the train changes share none (a rename counts as both paths);
- no gained file is a build input: nothing under `lampOS/crates/`, no `Cargo.*` anywhere, nothing under `.github/workflows/`, and no other file the lampOS ARM64 run takes (`lampOS/**` except the docs' Markdown and `lampOS/builders/`, with the docs `lampos-arm64.yml` takes back in, plus its `.github/scripts/lampos-*` and `.github/tools/lampos-promote/`).

The crates' tests read files in `lampOS/deploy`, `lampOS/skills`, `lampOS/etc` and those docs, so a gain there rebuilds.
Anything else rebuilds as before, and so does any error (fail closed).

Replay a case without changing anything:

```sh
DRY=1 .github/tools/coordination/behind-unrelated.sh <train head> <lamp-voice then>
DRY=1 LV_AT=<lamp-voice then> .github/tools/coordination/merge-when-green.sh <train PR> --behind-unrelated
```

For train 597: `DRY=1 LV_AT=f5f3dda325 .github/tools/coordination/merge-when-green.sh 5201 --behind-unrelated` prints
`dry: merge as is: gained 1 files since 762487b1d (robots/lamp/hardware/test/tools/factory-run.sh), none overlapping the train's 35, ...`.

## Marker rules

These are what `markers.py`, `reads2.py`, `qholds.py` and `train-bot.sh` do.

**Accounts.** Only comments from logins with write access count: `writers.txt` plus `writers-extra.txt`.
A READ-OK, a HOLD, lifting words, a TWO READS comment or a Queue HOLD from any other login is skipped.
The PR body's TWO READS ask always counts.
If the collaborators can't be fetched and `writers.txt` is an hour old or more, no PR is eligible (fail closed).
A PR whose head is in another repository is never carried.

**Fetches fail closed.** If a PR's head, comments or body can't be fetched, that PR is not eligible.
If the Queue (#4710) can't be fetched, no PR is eligible that round.
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
  ("HOLD lifted", "clears my HOLD", "lifts my 16:45 HOLD").
- The bot's own `HOLD train N` clears on any writer's later READ-OK marker that names a commit. A quoted READ-OK does not clear it.
- Any HOLD closed in words but never cleared by a READ-OK as above is lifted only by a row in `holds-lifted.tsv`.
  Post the reader's closing comment on the PR when you add the row.

**Queue HOLDs.** A Queue (#4710) comment of the last 3 days whose first line is `HOLD <sha>` holds the PR whose head starts with that sha.
It stands until the same login posts a later READ-OK marker on the PR whose first 400 characters say the HOLD is lifted or cleared
(the same words as above; `**READ-OK**` counts).

**TWO READS.** `TWO READS` anywhere in the PR body, or as the first line of a writer's comment, asks for two readers.
Then two different people need a READ-OK marker for a commit the head starts with.
A person is a login plus the name it signs, not a login alone.
Several builders share one GitHub account, so counting logins would make a legitimate two-reader PR impossible.
An unsigned READ-OK never adds a second person to a login: it merges into that login's signed reader,
and a login whose READ-OKs on the head are all unsigned is one reader.

**Trains.**

- A PR whose own ARM64 run on its head is red sits out.
- A PR stacked on another open PR sits out until that PR is on `lamp-voice`.
- No merge commits: the bot cherry-picks with `--no-merges` and merges by rebase.
- One lane: the bot waits while any `first/train-*` PR is open.
- A train of rows PRs alone waits for a code PR, or until the oldest is 3 h old.

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
