# Tests for #6556 B and C (12 Nov, the lead): merge-when-green.sh ends a train's wait at its first red check of any
# kind and merges only with every check green; the train bot reads a failed job's own log (train 833: one shard red
# while the others ran), finds the carried PRs a red non-cargo workflow takes, leaves out a PR whose own head has a red
# check, and reads unchanged PRs from its per-PR cache. On test_lanes.py's temporary repository and stand-in gh.
# Run from the repo root: python3 -I -m pytest -q .github/tools/coordination/tests
import json, os, subprocess, sys, unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import test_lanes
from test_lanes import ARM_CHECKS, BOT, LIB, MWG, TOOL, Repo, run

# The stand-in gh, with the Actions API the failure read asks (runs on a sha, a run's jobs, a job's log) and a file
# list per PR, answered ahead of test_lanes.py's cases.
EXTRA = r"""  *"actions/runs?head_sha="*) cat "$FAKE_DIR/runs.txt" 2>/dev/null; exit 0 ;;
  *actions/runs/*/jobs*) a="$*"; n=${a#*actions/runs/}; n=${n%%/*}; cat "$FAKE_DIR/jobs-$n.txt" 2>/dev/null; exit 0 ;;
  *actions/jobs/*/logs*) a="$*"; n=${a#*actions/jobs/}; n=${n%%/*}; cat "$FAKE_DIR/joblog-$n.txt" 2>/dev/null; exit 0 ;;
  "pr diff "*) n=$(echo "$*" | sed -E 's/^pr diff ([0-9]+).*/\1/'); if [ -f "$FAKE_DIR/files-$n.txt" ]; then cat "$FAKE_DIR/files-$n.txt"; else cat "$FAKE_DIR/files.txt"; fi; exit 0 ;;
"""
FAKE_GH = test_lanes.FAKE_GH.replace('case "$*" in\n', 'case "$*" in\n' + EXTRA, 1)
CRATE = "lampOS/crates/x/src/lib.rs"


def check(name, status="COMPLETED", conclusion="SUCCESS"):
    return {"__typename": "CheckRun", "name": name, "status": status, "conclusion": conclusion}


class Red(Repo):
    def setUp(self):
        super().setUp()
        with open(os.path.join(self.fake, "gh"), "w") as fh:
            fh.write(FAKE_GH)

    def mwg(self, checks):
        h = self.pr(7, [CRATE])
        self.fakefile("files.txt", CRATE + "\n")
        self.fakefile("head.txt", h + "\n")
        self.fakefile("checks.json", json.dumps({"statusCheckRollup": checks}))
        return h, run(["zsh", MWG, "7", "--behind-unrelated"], self.work, env=self.env, check=False)


class MergeOnEveryCheck(Red):
    def test_a_red_sdk_check_beside_green_arm64_never_merges_and_ends_the_wait_at_once(self):
        _, r = self.mwg(ARM_CHECKS + [check("Python 3.12 (pytest)", conclusion="FAILURE"), check("doc links", "IN_PROGRESS", None)])
        self.assertEqual(r.returncode, 2, r.stdout + r.stderr)
        self.assertIn("a check failed: Python 3.12 (pytest): COMPLETED FAILURE", r.stdout)
        self.assertNotIn("pr merge", self.calls())
        # The first look ends it: one statusCheckRollup read for the named checks, one for every check.
        self.assertEqual(self.calls().count("statusCheckRollup"), 2, self.calls())

    def test_all_green_merges(self):
        h, r = self.mwg(ARM_CHECKS + [check("Python 3.12 (pytest)"), check("promote", conclusion="SKIPPED")])
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("pr merge 7 -R autonomous-ai/lampOS --rebase --match-head-commit " + h, self.calls())

    def test_a_doc_check_still_running_waits(self):
        # 90 looks at once (MWG_POLL=0), never merged: exit 5, the 45 min path.
        _, r = self.mwg(ARM_CHECKS + [check("Local documentation files and anchors", "IN_PROGRESS", None)])
        self.assertEqual(r.returncode, 5, r.stdout + r.stderr)
        self.assertNotIn("pr merge", self.calls())


class FailureRead(Red):
    def red(self, sha):
        out = os.path.join(self.tmp, "jobs.log")
        r = run(["zsh", "-c", 'REPO=autonomous-ai/lampOS; source "$1"; coord_red_jobs "$2" "$3"', "x", LIB, sha, out],
                self.work, env=self.env)
        return r.stdout.splitlines(), (open(out).read() if os.path.exists(out) else "")

    def test_a_failed_shard_is_read_while_the_run_goes_on(self):
        # Train 833: shard 2/3 failed, the run still in progress; its job's own log names the crate.
        self.fakefile("runs.txt", "501 .github/workflows/lampos-arm64.yml\n")
        self.fakefile("jobs-501.txt", "9002 lampOS — build and test on Linux ARM64 (2/3)\n")
        self.fakefile("joblog-9002.txt", "        FAIL [   6.202s] ( 543/5515) lamp-body bus_out::tests::the_hand_is_said\nerror: test run failed\n")
        lines, log = self.red("abc")
        self.assertEqual(lines, [".github/workflows/lampos-arm64.yml lampOS — build and test on Linux ARM64 (2/3)"])
        self.assertIn("FAIL [   6.202s] ( 543/5515) lamp-body", log)
        self.assertNotIn("run view", self.calls())
        self.assertIn("actions/jobs/9002/logs", self.calls())

    def test_every_failed_run_on_the_sha_is_read(self):
        self.fakefile("runs.txt", "501 .github/workflows/lampos-arm64.yml\n502 .github/workflows/lampos-sdk.yml\n")
        self.fakefile("jobs-502.txt", "9100 Python 3.12 (pytest)\n")
        self.fakefile("joblog-9100.txt", "FAILED tests/test_client.py::test_local\n")
        lines, log = self.red("abc")
        self.assertEqual(lines, [".github/workflows/lampos-sdk.yml Python 3.12 (pytest)"])
        self.assertIn("test_local", log)

    def test_an_sdk_failure_names_the_prs_that_workflow_takes(self):
        self.fakefile("files-7.txt", "lampOS/sdk/python/client.py\n")
        self.fakefile("files-8.txt", CRATE + "\n")
        self.fakefile("files-9.txt", "lampOS/docs/controls.md\n" + TOOL + "\n")
        r = run(["zsh", "-c", 'REPO=autonomous-ai/lampOS; source "$1"; coord_prs_taken origin/lamp-voice lampos-sdk.yml 7 8 9', "x", LIB],
                self.work, env=self.env)
        self.assertEqual(r.stdout.split(), ["7", "9"])

    def test_an_unread_workflow_takes_nobody(self):
        self.fakefile("files-7.txt", "lampOS/sdk/python/client.py\n")
        r = run(["zsh", "-c", 'REPO=autonomous-ai/lampOS; source "$1"; coord_prs_taken origin/lamp-voice nope.yml 7', "x", LIB],
                self.work, env=self.env)
        self.assertEqual(r.stdout, "")


class Eligible(Red):
    def round(self, prs_lines, **extra):
        self.fakefile("prs.txt", "".join(l + "\n" for l in prs_lines))
        env = dict(self.env, DRY="1", NOCRATES_LANE="off", WORKTREE=os.path.join(self.tmp, "wt"),
                   WORKTREE_NC=os.path.join(self.tmp, "wtnc"), **extra)
        os.makedirs(self.state, exist_ok=True)
        open(os.path.join(self.fake, "calls.log"), "w").close()
        r = run(["zsh", BOT], self.work, env=env, check=False)
        return r, [l.split(" ", 1)[1] for l in r.stdout.splitlines()]

    def two(self):
        h7 = self.pr(7, [CRATE])
        h8 = self.pr(8, ["lampOS/crates/x/src/lib.rs", "Cargo.lock"])
        self.heads()
        return h7, h8

    def heads(self):
        self.fakefile("heads.txt", "".join("%d %s\n" % (n, open(os.path.join(self.fake, "head-%d.txt" % n)).read().strip()) for n in (7, 8)))

    def test_a_red_check_on_a_prs_own_head_sits_it_out(self):
        h7, h8 = self.two()
        self.fakefile("checks-8.json", json.dumps({"statusCheckRollup": [check("build and test"), check("Python 3.12 (pytest)", conclusion="FAILURE"),
                                                                         check("promote", conclusion="SKIPPED")]}))
        r, lines = self.round(["7", "8"])
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("#8 left out: a check on its own head %s is red (Python 3.12 (pytest))" % h8[:9], r.stderr)
        self.assertEqual(lines[-1], "dry: would carry 7 onto " + self.base[:9])

    def test_a_check_still_running_is_not_red(self):
        self.two()
        self.fakefile("checks-8.json", json.dumps({"statusCheckRollup": [check("build and test", "IN_PROGRESS", None)]}))
        r, lines = self.round(["7", "8"])
        self.assertEqual(lines[-1], "dry: would carry 7 8 onto " + self.base[:9])

    def test_unreadable_checks_sit_the_pr_out(self):
        self.two()
        self.fakefile("checks-8.json", "not json")
        r, lines = self.round(["7", "8"])
        self.assertIn("#8 left out: the checks on", r.stderr)
        self.assertEqual(lines[-1], "dry: would carry 7 onto " + self.base[:9])

    def test_a_pr_a_red_workflow_sat_out_stays_out_on_that_head(self):
        h7, h8 = self.two()
        os.makedirs(self.state, exist_ok=True)
        with open(os.path.join(self.state, "train-bot.sitout"), "w") as fh:
            fh.write("8 %s\n" % h8)
        r, lines = self.round(["7", "8"])
        self.assertIn("#8 left out: a train's non-cargo check failed", r.stderr)
        self.assertEqual(lines[-1], "dry: would carry 7 onto " + self.base[:9])

    def test_an_unchanged_pr_is_read_from_the_cache(self):
        h7, h8 = self.two()
        keyed = ["7 %s 2026-11-12T07:00:00Z" % h7, "8 %s 2026-11-12T07:00:00Z" % h8]
        r, lines = self.round(keyed)
        self.assertEqual(lines[-1], "dry: would carry 7 8 onto " + self.base[:9], r.stdout + r.stderr)
        self.assertIn("issues/7/comments", self.calls())
        self.assertIn("eligible: 2 of 2 open PRs eligible in", r.stdout + r.stderr)
        r, lines = self.round(keyed)
        self.assertEqual(lines[-1], "dry: would carry 7 8 onto " + self.base[:9], r.stdout + r.stderr)
        calls = self.calls()
        for asked in ("issues/7/comments", "issues/8/comments", "pulls/7/reviews", "pr view 7 -R autonomous-ai/lampOS --json headRefOid",
                      "pr view 7 -R autonomous-ai/lampOS --json body"):
            self.assertNotIn(asked, calls)
        self.assertIn("(2 read from the cache)", r.stdout + r.stderr)
        # The checks are read fresh each round.
        self.assertIn("pr view 7 -R autonomous-ai/lampOS --json statusCheckRollup", calls)

    def test_a_new_comment_or_a_new_head_reads_the_pr_again(self):
        h7, h8 = self.two()
        self.round(["7 %s 2026-11-12T07:00:00Z" % h7, "8 %s 2026-11-12T07:00:00Z" % h8])
        # #7 has a new comment (updatedAt moved); #8 a new head.
        h8b = self.pr(8, ["lampOS/crates/x/src/other.rs"], base=h8)
        self.heads()
        r, lines = self.round(["7 %s 2026-11-12T07:05:00Z" % h7, "8 %s 2026-11-12T07:00:00Z" % h8b])
        calls = self.calls()
        self.assertIn("issues/7/comments", calls)
        self.assertIn("issues/8/comments", calls)
        self.assertIn("(0 read from the cache)", r.stdout + r.stderr)

    def test_a_held_pr_is_seen_held_from_the_cache_too(self):
        # A HOLD after the READ-OK moves updatedAt, so the PR is read again and held.
        h7, h8 = self.two()
        self.round(["7 %s 2026-11-12T07:00:00Z" % h7, "8 %s 2026-11-12T07:00:00Z" % h8])
        with open(os.path.join(self.fake, "comments-8.json"), "a") as fh:
            fh.write(json.dumps({"login": "w1", "body": "HOLD " + h8, "created_at": "2026-11-12T07:04:00Z"}) + "\n")
        r, lines = self.round(["7 %s 2026-11-12T07:00:00Z" % h7, "8 %s 2026-11-12T07:04:00Z" % h8])
        self.assertEqual(lines[-1], "dry: would carry 7 onto " + self.base[:9], r.stdout + r.stderr)


if __name__ == "__main__":
    unittest.main()
