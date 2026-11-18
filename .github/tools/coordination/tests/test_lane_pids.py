# Tests for #7188 (16 Nov): with two lanes the train bot starts each lane's train in the background and keeps its
# process id. `job[$lane]=$!` kept the text "$!" under zsh 5.9, so three things did nothing:
#   1. Lane tracking. No lane ever read as running; a lane was kept to one train only by its open train PR, so it read
#      as free while its merged or failed train was still being done with (its PRs closed, or told why).
#   2. Pacing. The wait between rounds broke at its first look, 10 s in, instead of lasting 3 min while a train runs
#      and ending when a lane's train ends.
#   3. The stop. A TERM ended the bot and left each lane and its merge-when-green.sh running, to merge a train later.
# And with a real process id, three things that must not start: another live process that got a dead lane's process
# id holding the lane (or being signalled by a stop), a stop cutting a lane off as its train is merged or as it closes
# the merged train's PRs, and a train PR left open by a stop being waited for with nothing said.
# The bot's own loop runs here for real (not DRY=1), as in test_failed_trains.py: on the temporary repository, with a
# stand-in gh, and with a stand-in sleep that passes the bot's own waits in a hundredth of the time. Every process a
# test signals is one it started. Needs git, zsh and jq.
# Run from the repo root: python3 -I -m pytest -q .github/tools/coordination/tests
import json, os, re, signal, subprocess, sys, time, unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import test_failed_trains
from test_failed_trains import Loop
from test_lanes import ARM, ARM_CHECKS, BOT, CI_CHECKS, TOOL, run
from test_red_checks import check

# The stand-in gh, ahead of test_failed_trains.py's cases: `pr close N` waits while the file hold-close-N is there (a
# lane held as it closes a merged train's PRs), `pr merge` waits while hold-merge is there (a merge call under way),
# and a no-crates train's own files and checks are files-nc.txt and checks-nc.json, whatever number its PR got.
HELD = r"""a="$*"
case "$a" in
  "pr close "*) n=${a#pr close }; n=${n%% *}; i=0
                while [ -e "$FAKE_DIR/hold-close-$n" ] && [ $i -lt 600 ]; do /bin/sleep 0.1; i=$((i+1)); done ;;
  "pr merge "*) i=0; while [ -e "$FAKE_DIR/hold-merge" ] && [ $i -lt 600 ]; do /bin/sleep 0.1; i=$((i+1)); done ;;
  "pr diff "*|"pr view "*statusCheckRollup*)
                n=$(echo "$a" | sed -E 's/^pr (diff|view) ([0-9]+).*/\2/')
                if grep -q '^first/train-nc-' "$FAKE_DIR/branch-$n.txt" 2>/dev/null; then
                  case "$a" in "pr diff "*) cat "$FAKE_DIR/files-nc.txt"; exit 0 ;; *) cat "$FAKE_DIR/checks-nc.json"; exit 0 ;; esac
                fi ;;
esac
"""
FAKE_GH = test_failed_trains.FAKE_GH.replace('case "$*" in\n', HELD + 'case "$*" in\n', 1)
# The stand-in sleep. The bot's own waits (`sleep 10` between its looks at the lanes, `sleep 30` and `sleep 60` with
# nothing to do) pass in a hundredth of the time and are logged with the gh calls, in order; the file hold-sleep holds
# the wait it is in until the file is gone. Any other sleep (merge-when-green.sh's) is real. A bot still running after
# FAKE_UNTIL (its test was killed before it could end it) ends itself here, with its process group: the one the test
# wrote down in the file `group`, and only from inside it.
FAKE_SLEEP = r"""#!/bin/sh
if [ -n "$FAKE_UNTIL" ] && [ "$(date +%s)" -gt "$FAKE_UNTIL" ]; then
  g=$(cat "$FAKE_DIR/group" 2>/dev/null)
  [ -n "$g" ] && [ "$(ps -o pgid= -p $$ | tr -d ' ')" = "$g" ] && kill -KILL -- "-$g"
fi
case "$1" in
  10|30|60) echo "sleep $1" >> "$FAKE_DIR/calls.log"; i=0
            while [ -e "$FAKE_DIR/hold-sleep" ] && [ $i -lt 1200 ]; do /bin/sleep 0.05; i=$((i+1)); done
            exec /bin/sleep 0.$1 ;;
  *) exec /bin/sleep "$@" ;;
esac
"""
# A train whose checks are still running: its merge-when-green.sh goes on waiting.
RUNNING = [check("build and test", "IN_PROGRESS", None), check("clippy")]
NC_RUNNING = [check(CI_CHECKS[0]["name"], "IN_PROGRESS", None)]
THIRD = "lampOS/crates/x/src/third.rs"
# The looks 10 s apart in the wait between two rounds: 3 min while a lane's train runs, a minute with none.
LOOKS, LOOKS_IDLE = 18, 6


class Running:
    """The bot, running with two lanes in a process group of its own, for the length of a `with` block: whatever ends
    the test, the bot and all it left running are ended before the temporary folder and its stand-in gh go."""

    def __init__(self, case):
        self.case = case
        env = dict(case.env, NOCRATES_LANE="on", WORKTREE=os.path.join(case.tmp, "wt"), WORKTREE_NC=os.path.join(case.tmp, "wtnc"),
                   COORD_RED_WAIT_S="0", MWG_POLL="0.2", FAKE_UNTIL=str(int(time.time()) + 300))
        self.path = os.path.join(case.tmp, "bot.out")
        with open(self.path, "a") as fh:
            self.proc = subprocess.Popen(["zsh", BOT], cwd=case.work, env=env, stdout=fh, stderr=subprocess.STDOUT, start_new_session=True)
        self.pid = self.proc.pid
        case.bots.append(self)
        case.fakefile("group", "%d\n" % self.pid)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.end()

    def out(self):
        with open(self.path) as fh:
            return fh.read()

    def group(self):
        """Every live process in the bot's process group, as (pid, its parent, its command): each one this test started."""
        ps = subprocess.run(["ps", "-axo", "pid=,ppid=,pgid=,stat=,command="], capture_output=True, text=True).stdout
        found = []
        for line in ps.splitlines():
            f = line.split(None, 4)
            if len(f) >= 4 and f[2] == str(self.pid) and not f[3].startswith("Z"):
                found.append((int(f[0]), int(f[1]), f[4] if len(f) > 4 else ""))
        return found

    def lanes(self):
        """The lanes waiting for a train, as (the lane, its merge-when-green.sh): a child of the bot and that child's child."""
        g = self.group()
        return [(l[0], w[0]) for l in g if l[1] == self.pid for w in g if w[1] == l[0] and "merge-when-green.sh" in w[2]]

    def until(self, cond, what, timeout=60):
        """Wait until cond() is true and return it; the test fails, with what the bot printed, when it is not in time."""
        end = time.time() + timeout
        while time.time() < end:
            got = cond()
            if got:
                return got
            time.sleep(0.05)
        self.case.fail("%s: not within %d s\n%s" % (what, timeout, self.out()))

    def stop(self):
        """As an operator stops it (README.md): a TERM to the bot alone. Returns its exit code."""
        os.kill(self.pid, signal.SIGTERM)
        try:
            return self.proc.wait(30)
        except subprocess.TimeoutExpired:
            self.case.fail("the bot did not end within 30 s of its stop\n" + self.out())

    def left(self, timeout=10):
        """What is still running of the bot's process group once it has had `timeout` s to end; [] when all ended."""
        end = time.time() + timeout
        while True:
            g = self.group()
            if not g or time.time() >= end:
                return g
            time.sleep(0.05)

    def end(self):
        # Whatever is left of the process group this test started, and only while it still has a member.
        if self.group():
            try:
                os.killpg(self.pid, signal.SIGKILL)
            # macOS answers EPERM, not ESRCH, for a group whose leader has exited but isn't reaped yet (#7729).
            except (ProcessLookupError, PermissionError):
                pass
        try:
            self.proc.wait(30)
        except subprocess.TimeoutExpired:
            pass


def alive(pid):
    """Is this process, one the test started, still there? (Signal 0 asks; it signals nothing.)"""
    try:
        os.kill(pid, 0)
    except (ProcessLookupError, PermissionError):
        return False
    return True


class TwoLanes(Loop):
    """PRs 7, 8 and 9 for the crate lane and 10 for the no-crates lane; 7 and 8 are ready unless a test says otherwise.
    The trains the bot opens are PRs 101, 102, ... in that order."""

    def setUp(self):
        super().setUp()
        self.bots = []
        # Should a real gh ever be reached past the stand-in, it finds no such host: nothing goes to GitHub.
        self.env.update(GH_HOST="gh.invalid")
        with open(os.path.join(self.fake, "gh"), "w") as fh:
            fh.write(FAKE_GH)
        with open(os.path.join(self.fake, "sleep"), "w") as fh:
            fh.write(FAKE_SLEEP)
        os.chmod(os.path.join(self.fake, "sleep"), 0o755)
        self.fakefile("files-nc.txt", TOOL + "\n")
        self.fakefile("checks-nc.json", json.dumps({"statusCheckRollup": NC_RUNNING}))
        # Made before the bot starts, so nothing but the stand-in's lists changes under it.
        self.pr(9, [THIRD])
        self.pr(10, [TOOL])
        self.ready(7, 8)

    def tearDown(self):
        # Again, should a bot have been started outside a `with` block: before the stand-in gh goes.
        try:
            for bot in self.bots:
                bot.end()
        finally:
            super().tearDown()

    def fakefile(self, name, body):
        # In one step: the bot may be reading the file.
        new = os.path.join(self.fake, name + ".new")
        with open(new, "w") as fh:
            fh.write(body)
        os.replace(new, os.path.join(self.fake, name))

    def hold(self, name):
        self.fakefile(name, "")

    def release(self, name):
        os.unlink(os.path.join(self.fake, name))

    def waits(self):
        """The bot's own waits, from the stand-in's log: for each round begun (its list of the open trains), the looks
        10 s apart it has slept since; the last one may still be growing."""
        looks = []
        for c in self.calls().splitlines():
            if c.startswith("pr list ") and "--limit 50 --json headRefName" in c:
                looks.append(0)
            elif c == "sleep 10" and looks:
                looks[-1] += 1
        return looks

    def stopped_lines(self, out):
        return [l.split(" ", 1)[1] for l in out.splitlines() if re.match(r"\d\d:\d\d:\d\d stopped: ", l)]


class ALaneIsBusyWhileItsTrainRuns(TwoLanes):
    def test_a_lane_is_busy_until_its_train_is_done_with_and_the_other_lane_still_gets_its_train(self):
        # Train 24 merges, and its lane is held as it closes the first of its two PRs: its train PR is no longer open,
        # so only the lane's own process says it is still busy.
        self.hold("hold-close-7")
        with Running(self) as bot:
            bot.until(lambda: "pr close 7 " in self.calls(), "train 24 merged and its lane began closing its PRs")
            # Meanwhile a PR is ready for each lane.
            self.ready(7, 8, 9, 10)
            bot.until(lambda: "train nc-1 (#" in bot.out(), "the no-crates lane's train")
            self.assertEqual(self.trains(bot.out()), ["train 24 (#101): 7 8", "train nc-1 (#102): 10"],
                             "a crate train was cut while train 24 was still closing its PRs: a lane whose train is still running "
                             "must read as busy, and the other lane still gets its train\n" + bot.out())
            # Once train 24 is done with, its lane is free again: the next crate train goes.
            self.release("hold-close-7")
            bot.until(lambda: "train 25 (#" in bot.out(), "the crate lane's next train, once train 24 was done with")
            out = bot.out()
            self.assertEqual(self.trains(out), ["train 24 (#101): 7 8", "train nc-1 (#102): 10", "train 25 (#103): 9"], out)
            self.assertLess(out.index("closed #8"), out.index("train 25 (#103)"), out)

    def test_a_lane_that_stops_the_bot_lets_the_other_lanes_train_finish_first(self):
        # Both lanes' trains run for a round or more, as trains do. Then the crate train fails with no failing test
        # named, which stops the bot, while the no-crates train is still running.
        self.ready(7, 10)
        self.fakefile("runs.txt", "501 .github/workflows/lampos-arm64.yml\n")
        self.fakefile("jobs-501.txt", "9001 %s\n" % (ARM % "release"))
        self.fakefile("joblog-9001.txt", "##[error]Process completed with exit code 1.\n")
        self.train(1, RUNNING)
        with Running(self) as bot:
            bot.until(lambda: len(bot.lanes()) == 2 and len(self.waits()) >= 2, "both lanes waiting for their trains, a round later")
            self.assertEqual(self.trains(bot.out()), ["train 24 (#101): 7", "train nc-1 (#102): 10"], bot.out())
            self.train(1, [check("build and test", conclusion="FAILURE"), check("clippy")])
            stop = os.path.join(self.state, "train-bot.stop")
            bot.until(lambda: os.path.exists(stop), "the crate lane stopped the bot")
            # The bot's own last line and its exit wait for the other lane's train.
            early = ("the bot stopped while the no-crates lane's train was still running, and left it running: it must let the "
                     "other lane's train finish, then stop\n")
            end = time.time() + 4
            while time.time() < end:
                self.assertIsNone(bot.proc.poll(), early + bot.out())
                time.sleep(0.05)
            self.fakefile("checks-nc.json", json.dumps({"statusCheckRollup": CI_CHECKS}))
            bot.until(lambda: bot.proc.poll() is not None, "the bot stopped once the no-crates train had finished")
            out = bot.out()
            self.assertEqual(bot.proc.returncode, 2, out)
            last = re.search(r"^\d\d:\d\d:\d\d stopped for the first builder$", out, re.M)
            self.assertTrue(last and "closed #10" in out[:last.start()], early + out)
            self.assertIn("train nc-1 (#102) merged", " ".join(self.queue()))
            self.assertFalse(bot.left(), "left running after the bot stopped by itself\n" + out)


class Pacing(TwoLanes):
    def test_rounds_are_three_minutes_apart_while_a_train_runs_and_a_trains_end_starts_one_at_once(self):
        self.ready(7)
        self.train(1, RUNNING)
        with Running(self) as bot:
            lane, _ = bot.until(bot.lanes, "the crate lane waiting for train 24")[0]
            # The wait after the round that cut train 24: all of its looks, as the train is still running.
            bot.until(lambda: len(self.waits()) >= 2, "the round after train 24 was cut")
            self.assertEqual(self.waits()[0], LOOKS,
                             "the wait between two rounds while a lane's train runs is %d looks 10 s apart (3 min): the bot's "
                             "process id for the lane must be a real one, or the wait breaks at its first look\n%s" % (LOOKS, bot.out()))
            # The next wait is held at its first look while the train goes green, merges and its lane ends ...
            self.hold("hold-sleep")
            self.train(1, ARM_CHECKS)
            bot.until(lambda: "train 24 (#101) merged" in " ".join(self.queue()) and not alive(lane), "train 24 merged and its lane ended")
            looks = self.waits()[1]
            self.assertLess(looks, LOOKS, bot.out())
            # ... and ends at that look: the rest of the 3 min is not sat out.
            self.release("hold-sleep")
            bot.until(lambda: len(self.waits()) >= 3, "the round after train 24 ended")
            self.assertEqual(self.waits()[1], looks, "the wait must end at the first look after a lane's train ends\n" + bot.out())
            # With no train running, rounds are a minute apart, as before.
            bot.until(lambda: len(self.waits()) >= 4, "the next round")
            self.assertEqual(self.waits()[2], LOOKS_IDLE, bot.out())
            self.assertEqual(self.trains(bot.out()), ["train 24 (#101): 7"], bot.out())


class AStop(TwoLanes):
    def test_a_stop_ends_each_waiting_lane_and_its_merge_when_green(self):
        self.ready(7, 10)
        self.train(1, RUNNING)
        with Running(self) as bot:
            lanes = bot.until(lambda: (lambda l: l if len(l) == 2 else None)(bot.lanes()), "both lanes waiting for their trains")
            self.assertEqual(self.trains(bot.out()), ["train 24 (#101): 7", "train nc-1 (#102): 10"], bot.out())
            self.assertEqual(bot.stop(), 143, bot.out())
            left = bot.left()
            self.assertFalse(left, "a stop must end each lane's train and its merge-when-green.sh, or a stopped bot merges a train "
                             "later; it left these running:\n%s\n%s" % ("\n".join("  %d (parent %d) %s" % p for p in left), bot.out()))
            for lane, waiter in lanes:
                self.assertFalse(alive(lane) or alive(waiter))
            # Each train PR is left open, as it was, and the log names it.
            out = bot.out()
            self.assertEqual(sorted(self.stopped_lines(out)),
                             ["stopped: train 24 (#101) is left open with nothing waiting on it",
                              "stopped: train nc-1 (#102) is left open with nothing waiting on it"], out)
            for verb in ("pr merge", "pr close", "issue comment"):
                self.assertNotIn(verb, self.calls())
            with open(os.path.join(self.fake, "open.txt")) as fh:
                self.assertEqual(fh.read().split(), ["first/train-24", "first/train-nc-1"])

    def test_a_stop_lets_a_lane_that_is_closing_its_merged_trains_prs_finish(self):
        # Before the fix a stop ended nothing, so this lane finished too, with nothing said. A stop that ends lanes must
        # not cut one off after its train merged (its PRs would stay open and the Queue never hear of the merge), and
        # it says that the lane goes on.
        self.hold("hold-close-7")
        with Running(self) as bot:
            bot.until(lambda: "pr close 7 " in self.calls(), "train 24 merged and its lane began closing its PRs")
            closing = [p for p in bot.group() if " pr close 7 " in p[2]]
            self.assertEqual(len(closing), 1, bot.group())
            lane = closing[0][1]
            self.assertEqual(bot.stop(), 143, bot.out())
            self.assertTrue(alive(lane), "the stop ended a lane that was closing its merged train's PRs\n" + bot.out())
            self.release("hold-close-7")
            self.assertFalse(bot.left(30), bot.out())
            out = bot.out()
            for said in ("closed #7", "closed #8"):
                self.assertIn(said, out)
            self.assertIn("train 24 (#101) merged", " ".join(self.queue()))
            self.assertEqual(self.stopped_lines(out), ["stopped: the crate lane is past its wait: it finishes its train and ends by itself"],
                             "a stop must say that a lane goes on to finish its train\n" + out)

    def test_a_stop_lets_a_lane_whose_train_is_being_merged_finish(self):
        # The merge call of train 24 is under way and can't be taken back. Its lane is still waiting for it: ended
        # there, it would leave the train merged with its PRs open and the Queue untold.
        self.hold("hold-merge")
        with Running(self) as bot:
            bot.until(lambda: "pr merge 101 " in self.calls(), "train 24's merge call under way")
            lane, _ = bot.lanes()[0]
            self.assertEqual(bot.stop(), 143, bot.out())
            self.assertTrue(alive(lane), "the stop ended a lane whose train was being merged\n" + bot.out())
            self.release("hold-merge")
            self.assertFalse(bot.left(30), bot.out())
            out = bot.out()
            for said in ("closed #7", "closed #8"):
                self.assertIn(said, out)
            self.assertIn("train 24 (#101) merged", " ".join(self.queue()))
            self.assertEqual(self.stopped_lines(out), ["stopped: the crate lane's train is being merged: it finishes its train and ends by itself"],
                             "a stop must say that a lane goes on to finish its train\n" + out)

    def test_a_train_left_open_by_a_stop_is_said_at_the_next_start(self):
        # What a stop leaves: a train PR open with nothing waiting on it. Its lane waits for it, as for a train built
        # by hand, and the bot says so once; the other lane goes on.
        self.fakefile("open.txt", "first/train-7\n")
        self.ready(7, 10)
        with Running(self) as bot:
            bot.until(lambda: len(self.waits()) >= 3, "three rounds")
            out = bot.out()
            said = [l for l in out.splitlines() if "first/train-7" in l]
            self.assertEqual(len(said), 1, "a lane that waits for a train PR this bot is not running must say so, once\n" + out)
            self.assertIn("train PR first/train-7 was open at the start and this bot is not running it: "
                          "its lane waits until it is merged or closed", said[0])
            self.assertEqual(self.trains(out), ["train nc-1 (#101): 10"], out)


class OwnJobs(test_failed_trains.StandIn):
    """The bot's two helpers for a real process id, on their own: is a lane's train still this shell's own job, and what
    a stop may signal."""

    def zsh(self, script, *args):
        fns = ('typeset -A job; say() { print -r -- "$*"; }; '
               'eval "$(sed -n -e \'/^lane_running()/,/^}/p\' -e \'/^proc_tree()/,/^}/p\' -e \'/^stop_lanes()/,/^}/p\' "$1")"; shift; '
               '(( $+functions[lane_running] && $+functions[stop_lanes] )) || exit 3; ')
        r = run(["zsh", "-c", fns + script, "x", BOT, *args], self.work, env=self.env, check=False)
        self.assertNotEqual(r.returncode, 3, "train-bot.sh has no lane_running or no stop_lanes: it asks `kill -0` and `kill` "
                            "of whatever job[] holds, and before #7188 that was the text \"$!\"")
        return r

    def test_only_the_shells_own_running_job_reads_as_a_lanes_train(self):
        # A process id is used again once its process has ended: a live process that is not the bot's own job (here
        # this test, the shell's parent) must never hold a lane busy.
        r = self.zsh('/bin/sleep 20 & pid=$!; job[crates]=$pid\n'
                     'lane_running crates && print -r -- "its own running job: running"\n'
                     'job[nc]=$PPID; kill -0 $PPID && ! lane_running nc && print -r -- "a live process that is not its job: not running"\n'
                     'job[nc]=\'$!\'; lane_running nc || print -r -- "the text: not running"\n'
                     'unset "job[nc]"; lane_running nc || print -r -- "no train: not running"\n'
                     'kill $pid; wait $pid 2>/dev/null; lane_running crates || print -r -- "its own job, ended: not running"\n')
        self.assertEqual(r.stdout.splitlines(), ["its own running job: running", "a live process that is not its job: not running",
                                                 "the text: not running", "no train: not running", "its own job, ended: not running"],
                         r.stdout + r.stderr)

    def test_a_stop_signals_nothing_that_is_not_the_bots_own_lane(self):
        # A process of this test's own stands for one that got a dead lane's process id. The lane was waiting for its
        # train when it ended (its wait file still names the train), so a stop that went by the number would end it.
        other = subprocess.Popen(["/bin/sleep", "30"])
        self.addCleanup(other.wait)
        self.addCleanup(other.kill)
        os.makedirs(self.state, exist_ok=True)
        with open(os.path.join(self.state, "train-bot.wait-crates"), "w") as fh:
            fh.write("24 (#101)\n")
        r = self.zsh('SP="$2"; job[crates]="$1"; kill -0 "$1" || exit 4; stop_lanes; print -r -- "done"', str(other.pid), self.state)
        self.assertEqual(r.stdout, "done\n", r.stdout + r.stderr)
        time.sleep(0.3)
        self.assertIsNone(other.poll(), "a stop signalled a process that is not the bot's own lane")


if __name__ == "__main__":
    unittest.main()
