# Coordination tools

The first builder's tools for merging, reading asks and publishing the build rows.
They ran from one Mac. Now any operator can run them from a checkout of this repo.
They are zsh and Python scripts. They use `gh`, `git`, and BSD `date` (macOS).

Each script keeps its dated comments. They are the history of each rule.

## The tools

| Tool | What it does |
|---|---|
| `train-bot.sh` | Builds merge trains onto `lamp-voice`, back to back. It picks every eligible PR, cherry-picks its commits, pushes `first/train-N`, opens the train PR, waits for the checks and merges by rebase. `DRY=1` prints what one round would carry and stops. |
| `merge-when-green.sh PR` | Used by the bot. Waits for a PR's checks, then merges it by rebase, pinned to its head, only if it is green and contains `lamp-voice`'s tip. Exit codes: 0 merged, 2 failed, 3 behind `lamp-voice`, 4 refused, 5 not done in 45 min, 6 precheck failed. |
| `still-eligible.sh PR...` | Used by the bot just before a merge. Fails if any carried PR is now held or unread. |
| `lv-quiet.sh` | Used by `merge-when-green.sh`. Waits (at most 45 min) until no `lamp-voice` push build is still running. |
| `markers.py [PR]` | Reads a PR's comments as JSON lines `{login, body}` on stdin. Prints `HELD`, `READ-OK <sha>` or nothing. |
| `reads2.py HEAD_SHA` | Reads the PR body and comments as JSON lines. Prints `none`, `met N` or `short N` for a TWO READS ask. |
| `asks-for-me.sh [HH:MM]` | Prints, in full, every comment on the Queue (#3997, #4710) and the lead's channel (#1711) that names the first builder. With no time it starts 5 min before its own last run. |
| `build-rows-pr.sh` | Regenerates `lampOS/docs/build-rows.md` from the review page. If it changed, it pushes a branch `first/build-rows-MMDD-HHMM` and opens a docs-only PR. |
| `gen-build-rows.py BUILD_PY OUT_MD LABELS_JSON` | The generator `build-rows-pr.sh` calls. |

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
| `PYTHON` | `python3` | running `markers.py`, `reads2.py`, `gen-build-rows.py` |

`REPO_DIR`'s `origin` must be `autonomous-ai/lampOS`.

## State

State never lives in the repo. It lives in `$LAMP_COORD_STATE`:

- `train-bot.log`: the bot's log. `train-bot.out` and `train-bot.pid`: its output and process id, when started as below.
- `train-bot.told`: what the bot has already said, so it says each thing once.
- `pr-trainN.md`: each train PR's body.
- `train-first.txt`: PR numbers that go first, in that order.
- `train-hold.txt`: PR numbers that sit out.
- `train-only.txt`: while it exists, a train carries only the PRs it names.
- `holds-lifted.tsv`: HOLDs lifted in words. See `holds-lifted.tsv.example` for the format.
  Its keys hold account logins, so it stays out of the repo. Copy the first builder's file.
- `private-names.txt`: one regex a line. See `private-names.txt.example`. `build-rows-pr.sh` stops without it.
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
DRY=1 lampOS/tools/coordination/train-bot.sh
```

Start:

```sh
S="${LAMP_COORD_STATE:-$HOME/.lamp-coordination}"; mkdir -p "$S"
nohup lampOS/tools/coordination/train-bot.sh > "$S/train-bot.out" 2>&1 &
echo $! > "$S/train-bot.pid"
```

Stop: `kill "$(cat "$S/train-bot.pid")"`. Restart: stop, check no train is open, start.

The bot stops by itself (exit 2) when two trains in a row time out, or a run fails with no failing test named.
It says so on the Queue (#4710).

## Marker rules

- A reader approves with a comment whose first line is `READ-OK <sha>`. A reader blocks with `HOLD <sha>`.
  A marker later in the first 400 characters counts only with a commit right after it.
  A marker that is quoted, in brackets, or named as a noun ("your HOLD …") does not count.
- A READ-OK counts for the PR's head, or for a head with the same patch-id (a plain rebase).
  The patch-id is taken with no context lines.
- A HOLD is keyed on the account plus the reader's signature ("Sixth, 09:16", "Reviewer-lead:").
  It stands until the same account and signature post a later READ-OK.
- An unsigned HOLD stands until that account posts a READ-OK saying the HOLD is lifted or cleared.
- The bot's own `HOLD train N` clears on any later READ-OK.
- A comment on the Queue (#4710) whose first line is `HOLD <sha>` holds the PR whose head starts with that sha.
  It stands until the same account posts a READ-OK on the PR saying the HOLD is lifted or cleared.
- A HOLD closed in words but never cleared is lifted only by a row in `holds-lifted.tsv`.
  Post the reader's closing comment on the PR when you add the row.
- `TWO READS` anywhere in the PR body, or as the first line of a comment, asks for two readers.
  Then two different readers (account plus signature) need a READ-OK for the head.
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
