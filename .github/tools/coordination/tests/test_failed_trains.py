# Tests for #7181 (16 Nov): a crate train that failed with no PR to name.
#   1. Train 940 (15 Nov 20:17): its only red was a job cancelled at its 30-minute limit by a hung test. With no failed
#      job and no failing test the bot stopped until it was restarted by hand. Now the same PRs are cut again, once,
#      said in the log and on the Queue; a second such train in a row stops the bot as before, and a red with a failed
#      job is as before.
#   2. Trains 948 to 950 (16 Nov 00:33-00:55) each carried the same two PRs and failed to compile in a file neither of
#      them changed, so the bot held nobody and cut the pair again. Now a set that failed twice the same way is not cut
#      a third time: each of its PRs goes alone, one a train; one that fails alone is left out on that head and named on
#      the Queue with the file and the first error line, until its head changes; one that passes merges as usual.
#   3. The first read of #7186: with lamp-voice itself broken, every PR would fail alone and be left out on its head,
#      and a fix to lamp-voice would bring none of them back. Before a PR is left out for failing alone, the bot reads
#      lamp-voice's own push runs at its head: red there, no PR is left out and the bot stops, saying so on the Queue;
#      green, missing or still running, the PR is left out as in 2 (and the log says when there was no run to check).
# The bot's own loop runs here for real (not DRY=1), on test_lanes.py's temporary repository and a stand-in gh that
# opens, merges, closes and comments on PRs in that repository only. Needs git, zsh and jq.
# Run from the repo root: python3 -I -m pytest -q .github/tools/coordination/tests
import json, os, re, signal, subprocess, sys, time, unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import test_red_checks
from test_lanes import ARM, ARM_CHECKS, ARM_GREEN, BOT, LIB, QUEUE, run
from test_red_checks import CRATE, check

# The stand-in gh for a real round, ahead of test_red_checks.py's cases: `pr create` opens a train PR (numbered from
# 101, its head at refs/pull/N/head, its branch on the list of open trains the bot reads to see a lane busy), `pr merge`
# moves lamp-voice to the train's head, `pr close` takes a PR off the open list (and a train off the open trains, as a
# merge does), `pr comment` adds the comment to the PR's comments (so a HOLD the bot posts holds), a run's cancelled
# jobs are listed apart from its failed ones (the two calls differ only in their --jq), and lamp-voice's own push runs
# on a commit (lv-runs.txt) are apart from a train's runs (runs.txt): that call asks for event=push.
LOOP = r"""  *"actions/runs?event=push&head_sha="*) cat "$FAKE_DIR/lv-runs.txt" 2>/dev/null; exit 0 ;;
  "pr create "*) n=$(cat "$FAKE_DIR/created" 2>/dev/null || echo 100); n=$((n+1)); echo $n > "$FAKE_DIR/created"
                 br=$(echo "$*" | sed -E 's/.*--head ([^ ]+).*/\1/'); h=$(git ls-remote origin "refs/heads/$br" | cut -c1-40)
                 git push -q origin "$h:refs/pull/$n/head"; echo "$h" > "$FAKE_DIR/head-$n.txt"
                 echo "$br" > "$FAKE_DIR/branch-$n.txt"; echo "$br" >> "$FAKE_DIR/open.txt"
                 echo "https://github.com/autonomous-ai/lampOS/pull/$n"; exit 0 ;;
  "pr merge "*) a="$*"; n=${a#pr merge }; n=${n%% *}; h=$(echo "$*" | sed -E 's/.*--match-head-commit ([0-9a-f]+).*/\1/')
                git push -q origin "$h:refs/heads/lamp-voice" || exit 1
                grep -v -x -F "$(cat "$FAKE_DIR/branch-$n.txt")" "$FAKE_DIR/open.txt" > "$FAKE_DIR/open.txt.new"; mv "$FAKE_DIR/open.txt.new" "$FAKE_DIR/open.txt"; exit 0 ;;
  "pr close "*) a="$*"; n=${a#pr close }; n=${n%% *}
                for f in prs.txt heads.txt; do grep -v -E "^$n( |\$)" "$FAKE_DIR/$f" > "$FAKE_DIR/$f.new"; mv "$FAKE_DIR/$f.new" "$FAKE_DIR/$f"; done
                if [ -f "$FAKE_DIR/branch-$n.txt" ]; then
                  grep -v -x -F "$(cat "$FAKE_DIR/branch-$n.txt")" "$FAKE_DIR/open.txt" > "$FAKE_DIR/open.txt.new"; mv "$FAKE_DIR/open.txt.new" "$FAKE_DIR/open.txt"
                fi; exit 0 ;;
  "pr comment "*) a="$*"; n=${a#pr comment }; n=${n%% *}
                  printf '%s' "${a#*--body }" | jq -Rsc '{login: "w1", body: ., created_at: "2099-01-01T00:00:00Z"}' >> "$FAKE_DIR/comments-$n.json"
                  sed -E "s/^($n [0-9a-f]+) .*/\\1 $(date -u +%Y-%m-%dT%H:%M:%SZ)/" "$FAKE_DIR/prs.txt" > "$FAKE_DIR/prs.txt.new" && mv "$FAKE_DIR/prs.txt.new" "$FAKE_DIR/prs.txt"; exit 0 ;;
  "pr view "*"--json headRefOid -q"*) a="$*"; n=${a#pr view }; n=${n%% *}; cat "$FAKE_DIR/head-$n.txt" 2>/dev/null || cat "$FAKE_DIR/head.txt"; exit 0 ;;
  *actions/runs/*/jobs*cancelled*) a="$*"; n=${a#*actions/runs/}; n=${n%%/*}; cat "$FAKE_DIR/cancelled-$n.txt" 2>/dev/null; exit 0 ;;
"""
FAKE_GH = test_red_checks.FAKE_GH.replace('case "$*" in\n', 'case "$*" in\n' + LOOP, 1)
OTHER = "lampOS/crates/x/src/other.rs"
# The macOS job of train 940, and a train's checks with it cancelled and nothing else red.
SIM = "lampOS virtual lamp — lamp-sim builds and tests on macOS arm64"
CANCELLED = ARM_CHECKS + [check(SIM, conclusion="CANCELLED")]
# The clippy workflow's one job, as lamp-voice's own push run names it.
CLIPPY = "lampOS — clippy, every crate and target (x86 Linux)"
# A train that failed to compile: its checks, and its failed job's log as GitHub serves it (a time on each line).
RED = [check("build and test", conclusion="FAILURE"), check("clippy")]
TASK = "lampOS/crates/y/src/task.rs"
ERROR = "error[E0004]: non-exhaustive patterns: `Made::Never` not covered"
COMPILE_LOG = ("2026-11-15T17:38:39.9000000Z    Compiling lamp-y v0.1.0\n"
               "2026-11-15T17:38:40.1000000Z %s\n"
               "2026-11-15T17:38:40.1000001Z    --> crates/y/src/task.rs:412:15\n"
               "2026-11-15T17:38:41.0000000Z error: could not compile `lamp-y` (lib) due to 1 previous error\n"
               "2026-11-15T17:38:41.2000000Z ##[error]Process completed with exit code 101.\n" % ERROR)
# The longest a loop may take here before the test gives up on it. With two lanes the bot looks at a lane's train every
# 10 s (#7188; before, 10 s after it was cut and then a minute apart), so a slow machine can take minutes over a few
# trains.
DEADLINE = 600


def then(first, later):
    """True once `later` shows after `first` in the bot's output."""
    return lambda out: first in out and later in out.split(first, 1)[1]


class StandIn(test_red_checks.Red):
    def setUp(self):
        super().setUp()
        with open(os.path.join(self.fake, "gh"), "w") as fh:
            fh.write(FAKE_GH)
        # No credentials for a real gh, should one ever be reached past the stand-in.
        self.env.update(GH_TOKEN="none", GH_CONFIG_DIR=os.path.join(self.tmp, "no-gh"))


class Loop(StandIn):
    """PRs 7 and 8, both ready and both crate PRs; the trains the bot opens are PRs 101, 102, ... in that order."""

    def setUp(self):
        super().setUp()
        os.makedirs(self.state, exist_ok=True)
        # A train PR's own files (a crate file, so its ARM64 checks are waited for), and every head's checks unless a
        # test says otherwise: green, so a train with no checks of its own here merges.
        self.fakefile("files.txt", CRATE + "\n")
        self.fakefile("checks.json", json.dumps({"statusCheckRollup": ARM_GREEN}))
        self.pr(7, [CRATE])
        self.pr(8, [OTHER])
        self.ready(7, 8)

    def ready(self, *numbers):
        """The open PRs, as the bot lists them: each with its head and the time it last changed."""
        heads = []
        for n in numbers:
            with open(os.path.join(self.fake, "head-%d.txt" % n)) as fh:
                heads.append((n, fh.read().strip()))
        self.fakefile("heads.txt", "".join("%d %s\n" % nh for nh in heads))
        self.fakefile("prs.txt", "".join("%d %s 2026-11-16T00:00:00Z\n" % nh for nh in heads))

    def changes(self, n, *files):
        """The files `gh pr diff N --name-only` lists."""
        self.fakefile("files-%d.txt" % n, "".join(f + "\n" for f in files))

    def train(self, k, checks):
        """The checks of the k-th train the bot opens."""
        self.fakefile("checks-%d.json" % (100 + k), json.dumps({"statusCheckRollup": checks}))

    def loop(self, until=None, lanes="on"):
        """Run the bot until it stops by itself, or until `until(its output)` is true and then stop it. Returns its exit
        code (None when stopped here) and what it printed in this run."""
        env = dict(self.env, NOCRATES_LANE=lanes, WORKTREE=os.path.join(self.tmp, "wt"), WORKTREE_NC=os.path.join(self.tmp, "wtnc"),
                   COORD_RED_WAIT_S="0")
        path = os.path.join(self.tmp, "bot.out")
        start = os.path.getsize(path) if os.path.exists(path) else 0

        def printed():
            with open(path) as fh:
                fh.seek(start)
                return fh.read()

        with open(path, "a") as fh:
            bot = subprocess.Popen(["zsh", BOT], cwd=self.work, env=env, stdout=fh, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            end = time.time() + DEADLINE
            while time.time() < end:
                rc = bot.poll()
                out = printed()
                if rc is not None:
                    return rc, out
                if until and until(out):
                    return None, out
                time.sleep(0.2)
            self.fail("the bot neither stopped nor got there in %d s:\n%s" % (DEADLINE, printed()))
        finally:
            # The bot and whatever it left running (a lane's train in the background), all in one process group.
            for sig in (signal.SIGTERM, signal.SIGKILL):
                try:
                    os.killpg(bot.pid, sig)
                # macOS answers EPERM, not ESRCH, for a group whose leader has exited but isn't reaped yet
                # (the Reviewer-lead's P2 on #7729): nothing is left to signal either way.
                except (ProcessLookupError, PermissionError):
                    break
                try:
                    bot.wait(20)
                except subprocess.TimeoutExpired:
                    pass

    def dry(self):
        env = dict(self.env, DRY="1", WORKTREE=os.path.join(self.tmp, "wt"), WORKTREE_NC=os.path.join(self.tmp, "wtnc"))
        r = run(["zsh", BOT], self.work, env=env, check=False)
        return r, [l.split(" ", 1)[1] for l in r.stdout.splitlines()]

    def trains(self, out):
        """The trains cut, in order: "train 24 (#101): 7 8"."""
        return re.findall(r"^\d\d:\d\d:\d\d (train \S+ \(#\d+\): [\d ]+?) on [0-9a-f]+$", out, re.M)

    def queue(self):
        """The bot's posts on the Queue, in order."""
        return [c for c in self.calls().splitlines() if c.startswith("issue comment %d " % QUEUE[-1])]

    def comments(self, n):
        return [c for c in self.calls().splitlines() if c.startswith("pr comment %d " % n)]


class OnlyACancelledJob(Loop):
    def setUp(self):
        super().setUp()
        self.fakefile("runs.txt", "501 .github/workflows/lampos-arm64.yml\n502 .github/workflows/lampos-sim.yml\n")
        self.fakefile("cancelled-502.txt", "9200 %s\n" % SIM)
        self.said = "its only red was a cancelled job (%s): cut again once" % SIM

    def cut_again_once_then_stopped(self, lanes):
        # Train 940: no failed job, no failing test, the macOS job cancelled.
        self.train(1, CANCELLED)
        self.train(2, CANCELLED)
        rc, out = self.loop(lanes=lanes)
        self.assertEqual(self.trains(out), ["train 24 (#101): 7 8", "train 25 (#102): 7 8"], out)
        self.assertEqual(rc, 2, out)
        # Said once, of the first, in the log and on the Queue.
        lines = [l for l in out.splitlines() if self.said in l]
        self.assertEqual(len(lines), 1, out)
        self.assertIn("train 24 (#101)", lines[0])
        posts = [c for c in self.queue() if self.said in c]
        self.assertEqual(len(posts), 1, self.queue())
        self.assertIn("train 24 (#101)", posts[0])
        # The second stops the bot as before, and its lines name the cancelled job.
        second = out.split("train 25 (#102): 7 8", 1)[1]
        self.assertIn("no failing test named (red: %s (cancelled)): stopping for the first builder" % SIM, second)
        self.assertIn("train 25 (#102) failed with no failing test named (red: %s (cancelled)); stopped for the first builder" % SIM,
                      self.queue()[-1])
        # Nobody is held or told anything for a cancelled job.
        self.assertNotIn("pr comment", self.calls())

    def test_a_train_whose_only_red_is_a_cancelled_job_is_cut_again_once_and_the_second_stops_the_bot(self):
        self.cut_again_once_then_stopped("on")

    def test_cut_again_once_and_the_second_stops_the_bot_with_one_lane_too(self):
        self.cut_again_once_then_stopped("off")

    def test_a_red_with_a_failed_job_stops_the_bot_at_once_as_before(self):
        # A failed job that names no test, with a cancelled job beside it: never cut again.
        release = ARM % "release"
        self.fakefile("jobs-501.txt", "9001 %s\n" % release)
        self.fakefile("joblog-9001.txt", "##[error]Process completed with exit code 1.\n")
        self.train(1, [check("build and test", conclusion="FAILURE"), check("clippy"), check(SIM, conclusion="CANCELLED")])
        rc, out = self.loop(lanes="off")
        self.assertEqual(self.trains(out), ["train 24 (#101): 7 8"], out)
        self.assertEqual(rc, 2, out)
        self.assertIn("no failing test named (red: %s): stopping for the first builder" % release, out)
        self.assertNotIn("cut again", out)
        self.assertNotIn("cut again", self.calls())

    def test_a_timed_out_job_is_cut_again_once_too_and_a_merge_starts_the_count_again(self):
        # A job GitHub ends as timed_out is listed with the failed jobs too, and its log is read: no test in it.
        self.fakefile("jobs-502.txt", "9200 %s\n" % SIM)
        self.fakefile("joblog-9200.txt", "##[error]The job has exceeded the maximum execution time of 30m0s\n")
        self.train(1, ARM_CHECKS + [check(SIM, conclusion="TIMED_OUT")])
        # The second train is green: it merges, and both PRs close.
        rc, out = self.loop(until=then("closed #8", "open PRs eligible"), lanes="off")
        self.assertEqual(self.trains(out), ["train 24 (#101): 7 8", "train 25 (#102): 7 8"], out)
        self.assertEqual(rc, None, out)
        self.assertEqual(out.count(self.said), 1, out)
        self.assertNotIn("stopping for the first builder", out)
        # A later train with only a cancelled job is the first again: cut again once, not stopped at once.
        self.git("fetch", "-q", "origin")
        self.pr(9, ["lampOS/crates/x/src/third.rs"], base=self.rev("origin/lamp-voice"))
        self.ready(9)
        self.train(3, CANCELLED)
        self.train(4, CANCELLED)
        rc, out = self.loop(lanes="off")
        self.assertEqual(self.trains(out), ["train 26 (#103): 9", "train 27 (#104): 9"], out)
        self.assertEqual(rc, 2, out)
        self.assertEqual(out.count(self.said), 1, out)


class TheSameFailingSet(Loop):
    def setUp(self):
        super().setUp()
        self.fakefile("runs.txt", "501 .github/workflows/lampos-arm64.yml\n")
        self.fakefile("jobs-501.txt", "9001 %s\n" % (ARM % "build"))
        self.fakefile("joblog-9001.txt", COMPILE_LOG)
        self.error = ERROR.replace("`", "'")

    def left_out(self, n, train):
        """PR n failed alone in `train`: said once on the Queue with the file and the first error line, and to the PR."""
        posts = [c for c in self.queue() if "#%d" % n in c and "left out" in c]
        self.assertEqual(len(posts), 1, self.queue())
        for part in (train, TASK, self.error):
            self.assertIn(part, posts[0])
        told = self.comments(n)
        self.assertEqual(len(told), 1, self.calls())
        self.assertIn(TASK, told[0])
        self.assertIn("the next push to it brings it back", told[0])

    def test_a_set_that_failed_twice_the_same_way_is_not_cut_a_third_time_and_each_pr_goes_alone(self):
        # Trains 948 to 950: neither PR changes the file that fails to compile, so there is nobody to hold.
        self.changes(7, CRATE)
        self.changes(8, OTHER)
        self.train(1, RED)
        self.train(2, RED)
        self.train(4, RED)  # #8 alone; #7 alone, the third train, is green
        rc, out = self.loop(until=lambda o: then("#8 left out: ", "open PRs eligible")(o) or o.count("): 7 8 on ") > 2 or len(self.trains(o)) > 4)
        self.assertEqual(self.trains(out), ["train 24 (#101): 7 8", "train 25 (#102): 7 8", "train 26 (#103): 7", "train 27 (#104): 8"], out)
        self.assertEqual(out.count("failed to compile or lint in: " + TASK), 3, out)
        # #7 passed alone and merged as usual; #8 failed alone and is left out, said in the log and named on the Queue.
        self.assertIn("closed #7", out)
        self.assertIn("train 26 (#103) merged", " ".join(self.queue()))
        self.left_out(8, "train 27 (#104)")
        h8 = self.rev("refs/remotes/pr/8")
        said = [l for l in out.splitlines() if "#8" in l and "left out" in l]
        self.assertEqual(len(said), 2, out)
        self.assertIn(self.error, said[0])
        self.assertIn("#8 left out: alone in a train, it failed to compile or lint at %s; a new head brings it back" % h8[:9], said[1])
        # Nobody was held, and #7 was told nothing.
        self.assertNotIn("HOLD train", self.calls())
        self.assertEqual(self.comments(7), [])
        # It stays out on that head, said once ...
        r, lines = self.dry()
        self.assertEqual(lines, ["dry: crate lane: nothing eligible", "dry: no-crates lane: nothing eligible"], r.stdout + r.stderr)
        self.assertNotIn("#8 left out", r.stderr)
        # ... and a new head brings it back.
        self.git("fetch", "-q", "origin")
        lv = self.rev("origin/lamp-voice")
        self.pr(8, [OTHER], base=lv)
        self.ready(8)
        r, lines = self.dry()
        self.assertIn("dry: crate lane: would carry 8 onto " + lv[:9], lines, r.stdout + r.stderr)

    def test_when_every_pr_changes_the_failing_file_none_is_held_and_each_goes_alone(self):
        # Two PRs on the file: the bot can't tell which to leave out. It used to hold both.
        self.changes(7, CRATE, TASK)
        self.changes(8, OTHER, TASK)
        self.train(1, RED)
        self.train(2, RED)
        self.train(3, RED)  # #7 alone; #8 alone, the fourth train, is green
        rc, out = self.loop(until=lambda o: "closed #8" in o or "held #" in o or len(self.trains(o)) > 4, lanes="off")
        self.assertEqual(self.trains(out), ["train 24 (#101): 7 8", "train 25 (#102): 7 8", "train 26 (#103): 7", "train 27 (#104): 8"], out)
        self.assertNotIn("held #", out)
        self.assertNotIn("HOLD train", self.calls())
        self.left_out(7, "train 26 (#103)")
        self.assertIn("closed #8", out)
        self.assertEqual(self.comments(8), [])

    def test_a_pr_alone_that_fails_twice_in_a_file_it_does_not_change_is_left_out(self):
        # No third train: the second failure of a PR alone is its own.
        self.ready(7)
        self.changes(7, CRATE)
        self.train(1, RED)
        self.train(2, RED)
        rc, out = self.loop(until=lambda o: then("#7 left out: ", "open PRs eligible")(o) or len(self.trains(o)) > 2, lanes="off")
        self.assertEqual(self.trains(out), ["train 24 (#101): 7", "train 25 (#102): 7"], out)
        self.left_out(7, "train 25 (#102)")
        self.assertNotIn("HOLD train", self.calls())
        # lamp-voice's head has no push run here at all: the log says there was none to check.
        self.assertIn("lamp-voice's own run at %s was not there to check (no push run of lampos-clippy.yml, no push run of lampos-arm64.yml)"
                      ": the failure is taken as #7's" % self.rev("origin/lamp-voice")[:9], out)

    # The first read of #7186: lamp-voice itself broken would leave every PR out, one by one, each on its head.
    def lamp_voice(self, runs, failed=None):
        """lamp-voice's own push runs on its head, as the bot reads them: (run, how it ended or "running", workflow)
        each, and by run the failed jobs a train waits for."""
        self.fakefile("lv-runs.txt", "".join("%d %s .github/workflows/%s\n" % r for r in runs))
        for run_id, jobs in (failed or {}).items():
            self.fakefile("jobs-%d.txt" % run_id, "".join(j + "\n" for j in jobs))

    def alone_twice(self):
        """#7 alone, failing twice to compile in a file it does not change: the second time it would be left out."""
        self.ready(7)
        self.changes(7, CRATE)
        self.train(1, RED)
        self.train(2, RED)
        rc, out = self.loop(until=lambda o: then("#7 left out: ", "open PRs eligible")(o) or len(self.trains(o)) > 2, lanes="off")
        self.assertEqual(self.trains(out), ["train 24 (#101): 7", "train 25 (#102): 7"], out)
        return rc, out

    def test_a_pr_failing_alone_while_lamp_voice_itself_is_red_is_not_left_out_and_the_bot_stops(self):
        self.lamp_voice([(701, "failure", "lampos-clippy.yml"), (702, "success", "lampos-arm64.yml")], failed={701: [CLIPPY]})
        rc, out = self.alone_twice()
        lv = self.rev("origin/lamp-voice")[:9]
        # The bot stops, and says why in its log and in its one post on the Queue.
        self.assertEqual(rc, 2, out)
        self.assertTrue(os.path.exists(os.path.join(self.state, "train-bot.stop")))
        self.assertIn("lamp-voice itself is red at %s in lampos-clippy.yml (%s): #7 is not left out; stopping for the first builder" % (lv, CLIPPY), out)
        self.assertEqual(len(self.queue()), 1, self.queue())
        for part in ("train 25 (#102) carried #7 alone", TASK,
                     "lamp-voice itself is red at `%s` in lampos-clippy.yml (%s), so no PR is blamed; stopped for the first builder" % (lv, CLIPPY)):
            self.assertIn(part, self.queue()[0])
        # No PR is blamed: #7 is not left out, told nothing and not held.
        self.assertNotIn("left out", self.queue()[0])
        self.assertNotIn("#7 left out", out)
        self.assertEqual(self.comments(7), [])
        with open(os.path.join(self.state, "train-bot.alone-out")) as fh:
            self.assertEqual(fh.read(), "")

    def test_a_pr_failing_alone_while_lamp_voice_is_green_is_left_out(self):
        self.lamp_voice([(701, "success", "lampos-clippy.yml"), (702, "success", "lampos-arm64.yml")])
        rc, out = self.alone_twice()
        self.assertEqual(rc, None, out)
        self.left_out(7, "train 25 (#102)")
        self.assertIn("#7 left out: alone in a train, it failed to compile or lint at", out)
        self.assertNotIn("lamp-voice itself is red", out)
        self.assertNotIn("not there to check", out)

    def test_with_lamp_voices_own_run_missing_or_still_running_the_pr_is_left_out_and_the_log_says_so(self):
        # Its clippy run still going, and no ARM64 run on that head.
        self.lamp_voice([(701, "running", "lampos-clippy.yml")])
        rc, out = self.alone_twice()
        self.assertEqual(rc, None, out)
        self.left_out(7, "train 25 (#102)")
        self.assertIn("lamp-voice's own run at %s was not there to check (lampos-clippy.yml still running, no push run of lampos-arm64.yml)"
                      ": the failure is taken as #7's" % self.rev("origin/lamp-voice")[:9], out)
        self.assertNotIn("lamp-voice itself is red", out)

    def test_a_pr_that_changes_the_failing_file_is_held_and_the_rest_go_again_as_before(self):
        self.changes(7, CRATE, TASK)
        self.changes(8, OTHER)
        self.train(1, RED)
        rc, out = self.loop(until=lambda o: "closed #8" in o or len(self.trains(o)) > 2, lanes="off")
        self.assertEqual(self.trains(out), ["train 24 (#101): 7 8", "train 25 (#102): 8"], out)
        self.assertIn("  held #7", out)
        self.assertNotIn("held #8", out)
        self.assertEqual(len(self.comments(7)), 1, self.calls())
        self.assertIn("--body HOLD train 24: a compile or lint error in a file this PR changes (%s)." % TASK, self.comments(7)[0])
        self.assertIn("closed #8", out)
        self.assertEqual([c for c in self.queue() if "left out" in c], [])

    def test_a_pr_alone_that_changes_the_failing_file_is_held_at_once_as_before(self):
        self.ready(7)
        self.changes(7, CRATE, TASK)
        self.train(1, RED)
        rc, out = self.loop(until=lambda o: then("held #7", "open PRs eligible")(o) or len(self.trains(o)) > 1, lanes="off")
        self.assertEqual(self.trains(out), ["train 24 (#101): 7"], out)
        self.assertIn("  held #7", out)
        self.assertIn("--body HOLD train 24: a compile or lint error in a file this PR changes (%s)." % TASK, self.comments(7)[0])
        self.assertEqual([c for c in self.queue() if "left out" in c], [])


class Helpers(StandIn):
    def lib(self, script, *args):
        return run(["zsh", "-c", 'REPO=autonomous-ai/lampOS; source "$1"; shift; ' + script, "x", LIB, *args], self.work, env=self.env)

    def test_a_round_under_way_when_a_lane_stops_the_bot_cuts_no_train(self):
        # With two lanes a round can begin once the failed train's PR is closed, a moment before the stop is written
        # (train-bot.stop); it used to cut one more train before the bot stopped.
        os.makedirs(self.state, exist_ok=True)
        fn = ('SP="$2"; LV=x; two=1; alonef=$SP/train-bot.alone; aloneout=$SP/train-bot.alone-out; typeset -A job; '
              'say() { print -r -- "$*"; }; rows_only() { return 1; }; build_train() { print -r -- "built $*"; }; run_train() { :; }; '
              'source "$1"; eval "$(sed -n \'/^lane_round()/,/^}/p\' "$3")"; lane_round crates 0 7 8; print -r -- "rc $?"')
        go = lambda: run(["zsh", "-c", fn, "x", LIB, self.state, BOT], self.work, env=self.env).stdout.splitlines()
        self.assertEqual(go(), ["built crates 7 8", "rc 0"])
        open(os.path.join(self.state, "train-bot.stop"), "w").close()
        self.assertEqual(go(), ["rc 1"])

    def test_the_cancelled_jobs_of_every_run_on_the_head_are_listed(self):
        self.fakefile("runs.txt", "501 .github/workflows/lampos-arm64.yml\n502 .github/workflows/lampos-sim.yml\n")
        self.fakefile("jobs-501.txt", "9001 %s\n" % (ARM % "release"))
        self.fakefile("cancelled-502.txt", "9200 %s\n" % SIM)
        r = self.lib('coord_cancelled_jobs "$1"', "abc")
        self.assertEqual(r.stdout.splitlines(), [".github/workflows/lampos-sim.yml " + SIM])
        # No log is asked for: only the failed jobs' logs are read.
        self.assertNotIn("/logs", self.calls())

    def test_cancelled_and_timed_out_are_the_conclusions_listed(self):
        jobs = {"jobs": [{"id": 1, "name": "a (macOS)", "conclusion": "cancelled"}, {"id": 2, "name": "b", "conclusion": "timed_out"},
                         {"id": 3, "name": "c", "conclusion": "failure"}, {"id": 4, "name": "d", "conclusion": "success"},
                         {"id": 5, "name": "e", "conclusion": "skipped"}, {"id": 6, "name": "f", "conclusion": None}]}
        r = self.lib('jq -r "$coord_cancelled_jobs_jq" <<< "$1"', json.dumps(jobs))
        self.assertEqual(r.stdout.splitlines(), ["1 a (macOS)", "2 b"])

    def first_error(self, log):
        path = os.path.join(self.tmp, "job.log")
        with open(path, "w") as fh:
            fh.write(log)
        return self.lib('coord_first_error "$1"', path).stdout

    def test_the_first_error_line_of_a_log(self):
        # As GitHub serves it: a time on each line; backticks become quotes, so the line can sit in a code span.
        self.assertEqual(self.first_error(COMPILE_LOG), ERROR.replace("`", "'") + "\n")
        # Colours, and a lint error with no code.
        coloured = ("2026-11-15T17:38:40.1Z \x1b[0m\x1b[1m\x1b[38;5;9merror\x1b[0m\x1b[0m\x1b[1m: unused variable: `x`\x1b[0m\n"
                    "2026-11-15T17:38:40.2Z \x1b[0m  \x1b[0m\x1b[0m\x1b[1m\x1b[38;5;12m--> \x1b[0m\x1b[0mcrates/y/src/task.rs:3:9\x1b[0m\n")
        self.assertEqual(self.first_error(coloured), "error: unused variable: 'x'\n")
        # A long line is cut, and a log with no error line gives nothing.
        self.assertEqual(self.first_error("error: " + "x" * 400 + "\n"), "error: " + "x" * 153 + "\n")
        self.assertEqual(self.first_error("warning: unused\n##[error]Process completed with exit code 101.\n"), "")

    # The first read of #7186: lamp-voice's own head, read before a PR is left out for failing alone.
    def test_lamp_voices_own_head_reads_red_ok_or_not_there(self):
        state = lambda sha="abc": self.lib('coord_lv_state "$1"', sha).stdout
        clippy, arm = ".github/workflows/lampos-clippy.yml", ".github/workflows/lampos-arm64.yml"
        # No push run on that commit (or none that could be read): not there, each named.
        self.assertEqual(state(), "none no push run of lampos-clippy.yml, no push run of lampos-arm64.yml\n")
        self.assertIn("actions/runs?event=push&head_sha=abc", self.calls())
        # One still going.
        self.fakefile("lv-runs.txt", "702 running %s\n701 success %s\n" % (arm, clippy))
        self.assertEqual(state(), "none lampos-arm64.yml still running\n")
        # One ended with no answer.
        self.fakefile("lv-runs.txt", "702 success %s\n701 cancelled %s\n" % (arm, clippy))
        self.assertEqual(state(), "none lampos-clippy.yml cancelled\n")
        # Both there and done, no job a train waits for failed: green. Also when the ARM64 run failed in another job
        # only: lamp-voice's head 051928e2be on 16 Nov, its build reused from the train and its publish failed.
        self.fakefile("lv-runs.txt", "702 success %s\n701 success %s\n" % (arm, clippy))
        self.assertEqual(state(), "ok\n")
        self.fakefile("lv-runs.txt", "702 failure %s\n701 success %s\n" % (arm, clippy))
        self.assertEqual(state(), "ok\n")
        # A failed job a train waits for: red, with the workflow and the job. Also while the run's other jobs still run.
        self.fakefile("lv-runs.txt", "702 success %s\n701 failure %s\n" % (arm, clippy))
        self.fakefile("jobs-701.txt", CLIPPY + "\n")
        self.assertEqual(state(), "red lampos-clippy.yml (%s)\n" % CLIPPY)
        self.fakefile("jobs-702.txt", "%s\n%s\n" % (ARM % "2/3", ARM % "3/3"))
        self.fakefile("lv-runs.txt", "702 running %s\n" % arm)
        self.assertEqual(state(), "red lampos-arm64.yml (%s)\n" % (ARM % "2/3"))
        # No commit to ask about: nothing is asked.
        open(os.path.join(self.fake, "calls.log"), "w").close()
        self.assertEqual(state(""), "none lamp-voice's head can't be read\n")
        self.assertEqual(self.calls(), "")

    def test_the_runs_and_jobs_read_for_lamp_voices_own_head(self):
        # The filters themselves, on jq. Runs: the latest push run of the clippy and of the ARM64 workflow; a promotion
        # (workflow_dispatch), a pull request's run and other workflows are not lamp-voice's own push runs.
        wf = ".github/workflows/lampos-%s.yml"
        runs = {"workflow_runs": [
            {"id": 1, "event": "push", "path": wf % "clippy", "status": "completed", "conclusion": "success", "run_number": 10},
            {"id": 2, "event": "push", "path": wf % "clippy", "status": "in_progress", "conclusion": None, "run_number": 11},
            {"id": 3, "event": "workflow_dispatch", "path": wf % "arm64", "status": "completed", "conclusion": "success", "run_number": 50},
            {"id": 4, "event": "push", "path": wf % "arm64", "status": "completed", "conclusion": "failure", "run_number": 49},
            {"id": 5, "event": "push", "path": wf % "sim", "status": "completed", "conclusion": "failure", "run_number": 7},
            {"id": 6, "event": "pull_request", "path": wf % "clippy", "status": "completed", "conclusion": "failure", "run_number": 12}]}
        r = self.lib('jq -r "$coord_lv_runs_jq" <<< "$1"', json.dumps(runs))
        self.assertEqual(r.stdout.splitlines(), ["4 failure " + wf % "arm64", "2 running " + wf % "clippy"])
        for run in runs["workflow_runs"]:
            run.update(status="completed", conclusion="cancelled" if run["id"] == 2 else None)
        r = self.lib('jq -r "$coord_lv_runs_jq" <<< "$1"', json.dumps(runs))
        self.assertEqual(r.stdout.splitlines(), ["4 unknown " + wf % "arm64", "2 cancelled " + wf % "clippy"])
        # Jobs: the failed ones a train waits for. A failed publish is not lamp-voice failing to build or lint.
        jobs = {"jobs": [{"name": ARM % "build", "conclusion": "success"}, {"name": ARM % "2/3", "conclusion": "failure"},
                         {"name": "lampOS — sign and publish the tested lamp-voice artifact", "conclusion": "failure"},
                         {"name": CLIPPY, "conclusion": "failure"}, {"name": ARM % "release", "conclusion": "cancelled"},
                         {"name": ARM % "1/3", "conclusion": None}]}
        r = self.lib('jq -r "$coord_lv_failed_jq" <<< "$1"', json.dumps(jobs))
        self.assertEqual(r.stdout.splitlines(), [ARM % "2/3", CLIPPY])


if __name__ == "__main__":
    unittest.main()
