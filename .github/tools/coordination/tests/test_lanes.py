# Tests for the no-crates lane (#5212 part 3): lib.sh's lane test, lane.sh, behind-unrelated.sh --no-crates,
# merge-when-green.sh on a train with no ARM64 run, and train-bot.sh's DRY=1 round with the lane on and off. A temporary
# git repository and a stand-in `gh` on PATH. Needs git, zsh and jq.
# Run from the repo root: python3 -I -m pytest -q .github/tools/coordination/tests
import json, os, shutil, subprocess, tempfile, unittest

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LIB = os.path.join(HERE, "lib.sh")
BEHIND = os.path.join(HERE, "behind-unrelated.sh")
MWG = os.path.join(HERE, "merge-when-green.sh")
BOT = os.path.join(HERE, "train-bot.sh")
LANE = os.path.join(HERE, "lane.sh")
ARM64 = """on:
  push:
    branches: ['lamp-voice']
    paths: ['lampOS/**']
  pull_request:
    branches: ['lamp-voice']
    paths:
      - 'lampOS/**'
      - '!lampOS/docs/**/*.md'
      - '!lampOS/builders/**'
      # The docs code or tests read.
      - 'lampOS/docs/memory.md'
      - '.github/workflows/lampos-arm64.yml'
      - '.github/scripts/lampos-route*.py'
      - '.github/tools/lampos-promote/**'
  workflow_dispatch:
"""
SDK = """on:
  pull_request:
    branches: [lamp-voice]
    paths:
      - 'lampOS/sdk/**'
      - 'lampOS/docs/control*'
      - 'lampOS/docs/skill-format*'
      - '.github/workflows/lampos-sdk.yml'
  push:
    branches: [lamp-voice]
"""
# The stand-in gh: answers what merge-when-green.sh, lv-quiet.sh and train-bot.sh's DRY=1 round ask, after their --jq,
# from files in $FAKE_DIR; logs every call.
FAKE_GH = r"""#!/bin/sh
echo "$*" >> "$FAKE_DIR/calls.log"
case "$*" in
  "pr diff"*) cat "$FAKE_DIR/files.txt" ;;
  *collaborators*) echo w1 ;;
  *issues/4710/comments*) ;;
  *issues/*/comments*) a="$*"; n=${a#*issues/}; n=${n%%/*}; cat "$FAKE_DIR/comments-$n.json" ;;
  *"--state all"*) echo 23 ;;
  *"--json headRefName --jq"*) cat "$FAKE_DIR/open.txt" 2>/dev/null ;;
  *"number,isDraft"*) cat "$FAKE_DIR/prs.txt" ;;
  *"number,headRefOid,headRefName"*) cat "$FAKE_DIR/heads.txt" ;;
  *isCrossRepository,headRepositoryOwner*) n=$(echo "$*" | sed -E 's/^pr view ([0-9]+).*/\1/'); echo "false autonomous-ai/lampOS $(cat "$FAKE_DIR/head-$n.txt")" ;;
  *"--json body"*) echo '{"login":"","body":"","pr_body":true}' ;;
  *title,createdAt*) printf 'lampOS: a change\t2026-11-04T01:00:00Z\n' ;;
  *headRefOid*) cat "$FAKE_DIR/head.txt" ;;
  *statusCheckRollup*) cat "$FAKE_DIR/checks.json" ;;
  "run list"*) echo 0 ;;
  "pr merge"*) [ -n "$FAKE_ON_MERGE" ] && git push -q origin "$(git commit-tree -p origin/lamp-voice -m moved origin/lamp-voice^{tree})":refs/heads/lamp-voice; exit "${FAKE_MERGE_RC:-0}" ;;
esac
"""
ARM_CHECKS = [{"__typename": "CheckRun", "name": "build and test", "status": "COMPLETED", "conclusion": "SUCCESS"},
              {"__typename": "CheckRun", "name": "clippy", "status": "COMPLETED", "conclusion": "SUCCESS"}]
CI_CHECKS = [{"__typename": "CheckRun", "name": "Go (system) — build · vet · test", "status": "COMPLETED", "conclusion": "SUCCESS"},
             {"__typename": "CheckRun", "name": "Python (hal) — unit tests · syntax", "status": "COMPLETED", "conclusion": "SUCCESS"}]
TOOL = ".github/tools/coordination/x.sh"


def run(args, cwd, env=None, check=True):
    return subprocess.run(args, cwd=cwd, env=env, check=check, capture_output=True, text=True)


class Repo(unittest.TestCase):
    """A lamp-voice holding both workflows and a file in each kind of place; PRs are added per test."""

    @classmethod
    def setUpClass(cls):
        cls.template = tempfile.mkdtemp()
        t = cls.__new__(cls)
        t.tmp = cls.template
        t.build()
        cls.base = t.base

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.template, ignore_errors=True)

    def setUp(self):
        self.tmp = os.path.join(tempfile.mkdtemp(), "t")
        shutil.copytree(self.template, self.tmp, symlinks=True)
        self.paths()
        self.git("remote", "set-url", "origin", self.origin)

    def tearDown(self):
        shutil.rmtree(os.path.dirname(self.tmp), ignore_errors=True)

    def paths(self):
        self.origin = os.path.join(self.tmp, "origin.git")
        self.work = os.path.join(self.tmp, "work")
        self.fake = os.path.join(self.tmp, "fake")
        self.state = os.path.join(self.tmp, "state")
        self.env = dict(os.environ, REPO_DIR=self.work, TOOLS_DIR=HERE, FAKE_DIR=self.fake, LAMP_COORD_STATE=self.state,
                        MWG_POLL="0", PATH=self.fake + os.pathsep + os.environ["PATH"])
        for k in ("DRY", "LV_AT", "NOCRATES_LANE", "WORKTREE", "WORKTREE_NC"):
            self.env.pop(k, None)

    def build(self):
        self.paths()
        os.makedirs(self.fake)
        run(["git", "init", "-q", "--bare", self.origin], self.tmp)
        run(["git", "init", "-q", "-b", "lamp-voice", self.work], self.tmp)
        self.git("config", "user.email", "t@example.invalid")
        self.git("config", "user.name", "t")
        self.git("remote", "add", "origin", self.origin)
        for f, body in [("lampOS/crates/x/src/lib.rs", "x\n"), ("Cargo.lock", "x\n"), ("lampOS/docs/memory.md", "x\n"),
                        ("lampOS/docs/other.md", "x\n"), (".github/workflows/lampos-arm64.yml", ARM64),
                        (".github/workflows/lampos-sdk.yml", SDK), (TOOL, "x\n"), ("system/main.go", "x\n"),
                        ("robots/lamp/hardware/test/tools/factory-run.sh", "x\n")]:
            self.write(f, body)
        self.commit("base")
        self.git("push", "-q", "origin", "lamp-voice")
        self.git("fetch", "-q", "origin")
        self.base = self.rev("HEAD")
        with open(os.path.join(self.fake, "gh"), "w") as fh:
            fh.write(FAKE_GH)
        os.chmod(os.path.join(self.fake, "gh"), 0o755)

    def git(self, *a):
        return run(["git", *a], self.work).stdout

    def rev(self, ref):
        return self.git("rev-parse", ref).strip()

    def write(self, f, body):
        p = os.path.join(self.work, f)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as fh:
            fh.write(body)

    def fakefile(self, name, body):
        with open(os.path.join(self.fake, name), "w") as fh:
            fh.write(body)

    def commit(self, msg):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", msg)

    def pr(self, n, files, base=None):
        """PR n: one commit changing files on top of base (default: the template's lamp-voice), pushed as its head."""
        self.git("checkout", "-q", "--detach", base or self.base)
        for f in files:
            self.write(f, "pr %d\n" % n)
        self.commit("pr %d" % n)
        h = self.rev("HEAD")
        self.git("push", "-q", "-f", "origin", "HEAD:refs/pull/%d/head" % n)
        self.git("fetch", "-q", "origin", "+refs/pull/%d/head:refs/remotes/pr/%d" % (n, n))
        self.fakefile("head-%d.txt" % n, h + "\n")
        self.fakefile("comments-%d.json" % n, json.dumps({"login": "w1", "body": "READ-OK " + h,
                                                          "created_at": "2026-11-04T01:00:00Z"}) + "\n")
        return h

    def gain(self, f):
        """lamp-voice gains a change to f, pushed."""
        self.git("checkout", "-q", "--detach", "origin/lamp-voice")
        self.write(f, "gained\n")
        self.commit("gain " + f)
        self.git("push", "-q", "origin", "HEAD:lamp-voice")
        self.git("fetch", "-q", "origin")
        return self.rev("HEAD")

    def lane(self, files):
        h = self.pr(9, files)
        r = run(["zsh", "-c", 'source "$1"; coord_inputs_init origin/lamp-voice; coord_lane "$2"', "x", LIB, h],
                self.work, env=self.env)
        return r.stdout.strip()

    def calls(self):
        p = os.path.join(self.fake, "calls.log")
        return open(p).read() if os.path.exists(p) else ""


class Lanes(Repo):
    def test_docs_tools_and_other_code_go_to_the_no_crates_lane(self):
        for files in (["lampOS/docs/other.md"], ["lampOS/builders/twelve/log.md"], [TOOL],
                      ["robots/lamp/hardware/test/tools/factory-run.sh"], ["system/main.go"],
                      ["lampOS/docs/other.md", TOOL, "system/main.go"]):
            self.assertEqual(self.lane(files), "nocrates", files)

    def test_any_build_input_goes_to_the_crate_lane(self):
        for files, why in ((["lampOS/crates/x/src/lib.rs"], "crates"), (["Cargo.lock"], "Cargo"),
                           (["tools/other/Cargo.toml"], "Cargo"), ([".github/workflows/ci.yml"], "workflows"),
                           (["lampOS/docs/memory.md"], "a doc the ARM64 run reads"),
                           (["lampOS/deploy/product/x.json"], "an ARM64 input"),
                           (["lampOS/sdk/js/src/testing.js"], "an ARM64 input"),
                           ([".github/scripts/lampos-route2.py"], "an ARM64 input"),
                           ([".github/tools/lampos-promote/run.sh"], "an ARM64 input")):
            self.assertEqual(self.lane(files), "crates: %s (%s)" % (files[0], why), files)

    def test_the_sdk_checks_docs_go_to_the_crate_lane(self):
        # Thirteen's P3 on #5289: lampos-sdk.yml takes lampOS/docs/control* and skill-format*, Markdown included.
        self.assertEqual(self.lane(["lampOS/docs/controls.md"]), "crates: lampOS/docs/controls.md (a doc the SDK check reads)")
        self.assertEqual(self.lane(["lampOS/docs/skill-format.md"]),
                         "crates: lampOS/docs/skill-format.md (a doc the SDK check reads)")

    def test_a_mixed_pr_goes_to_the_crate_lane(self):
        self.assertEqual(self.lane([TOOL, "lampOS/docs/other.md", "lampOS/crates/x/src/lib.rs"]),
                         "crates: lampOS/crates/x/src/lib.rs (crates)")

    def test_unread_workflows_fail_closed(self):
        # Paths read from a lamp-voice without the workflows: every lampOS doc is an input.
        h = self.pr(9, ["lampOS/docs/other.md"])
        self.git("checkout", "-q", "--detach", self.base)
        self.git("rm", "-q", ".github/workflows/lampos-arm64.yml", ".github/workflows/lampos-sdk.yml")
        self.commit("no workflows")
        r = run(["zsh", "-c", 'source "$1"; coord_inputs_init HEAD; coord_lane "$2" HEAD', "x", LIB, h], self.work, env=self.env)
        self.assertTrue(r.stdout.startswith("crates: lampOS/docs/other.md"), r.stdout)

    def test_lane_sh(self):
        self.pr(7, [TOOL])
        self.pr(8, ["lampOS/crates/x/src/lib.rs"])
        r = run(["zsh", LANE, "7", "8"], self.work, env=self.env)
        self.assertEqual(r.stdout, "#7: no-crates lane\n#8: crate lane (lampOS/crates/x/src/lib.rs (crates))\n")


class NeverRebuildEachOther(Repo):
    def behind(self, train, *flags):
        return run(["zsh", BEHIND, *flags, train, "origin/lamp-voice"], self.work, env=self.env, check=False)

    def test_a_no_crates_merge_leaves_a_crate_train_as_is(self):
        train = self.pr(7, ["lampOS/crates/x/src/lib.rs"])
        self.gain(TOOL)
        r = self.behind(train)
        self.assertEqual(r.returncode, 0, r.stdout)
        self.assertTrue(r.stdout.startswith("merge as is:"), r.stdout)

    def test_a_crate_merge_leaves_a_no_crates_train_as_is(self):
        train = self.pr(7, [TOOL, "system/main.go"])
        self.gain("lampOS/crates/x/src/lib.rs")
        self.gain("Cargo.lock")
        r = self.behind(train, "--no-crates")
        self.assertEqual(r.returncode, 0, r.stdout)
        self.assertTrue(r.stdout.startswith("merge as is (no-crates train): gained 2 files"), r.stdout)
        # Without --no-crates (the lane off), part 2's test stands: a gained input rebuilds.
        self.assertEqual(self.behind(train).returncode, 1)

    def test_an_overlap_rebuilds_either_way(self):
        train = self.pr(7, [TOOL])
        self.gain(TOOL)
        r = self.behind(train, "--no-crates")
        self.assertEqual(r.returncode, 1)
        self.assertIn("also in the train: " + TOOL, r.stdout)

    def test_a_gained_workflow_rebuilds_a_no_crates_train(self):
        train = self.pr(7, [TOOL])
        self.gain(".github/workflows/ci.yml")
        r = self.behind(train, "--no-crates")
        self.assertEqual(r.returncode, 1)
        self.assertIn("(workflows)", r.stdout)

    def test_a_docs_train_rebuilds_on_a_gained_doc(self):
        train = self.pr(7, ["lampOS/docs/other.md"])
        self.gain("lampOS/docs/memory.md")
        r = self.behind(train, "--no-crates")
        self.assertEqual(r.returncode, 1)
        self.assertIn("doc-links", r.stdout)

    def test_no_crates_on_a_crate_train_falls_back_to_part_2(self):
        train = self.pr(7, ["lampOS/crates/x/src/lib.rs"])
        self.gain("Cargo.lock")
        r = self.behind(train, "--no-crates")
        self.assertEqual(r.returncode, 1)
        self.assertIn("no-crates test not taken", r.stdout)
        self.assertIn("Cargo.lock (Cargo)", r.stdout)


class MergeNoArm64(Repo):
    def mwg(self, files, checks, *flags, env=None):
        h = self.pr(7, files)
        self.fakefile("files.txt", "".join(f + "\n" for f in files))
        self.fakefile("head.txt", h + "\n")
        self.fakefile("checks.json", json.dumps({"statusCheckRollup": checks}))
        r = run(["zsh", MWG, "7", *flags], self.work, env=env or self.env, check=False)
        return h, r

    def test_a_tools_train_merges_on_its_own_checks_without_arm64(self):
        h, r = self.mwg([TOOL], CI_CHECKS, "--behind-unrelated", "--no-crates")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("has no ARM64 run (no file it takes): its 2 checks passed", r.stdout)
        self.assertIn("pr merge 7 -R autonomous-ai/lampOS --rebase --match-head-commit " + h, self.calls())

    def test_a_docs_train_with_no_checks_merges_at_once(self):
        h, r = self.mwg(["lampOS/docs/other.md", "lampOS/builders/twelve/log.md"], [], "--behind-unrelated", "--no-crates")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("docs the workflows skip: no checks to wait for", r.stdout)
        self.assertIn("--match-head-commit " + h, self.calls())

    def test_an_sdk_doc_waits_for_the_sdk_check(self):
        # controls.md starts lampos-sdk.yml, not ARM64: not "no checks", so a failed SDK check stops it.
        failed = [{"__typename": "CheckRun", "name": "sdk", "status": "COMPLETED", "conclusion": "FAILURE"}]
        _, r = self.mwg(["lampOS/docs/controls.md"], failed, "--behind-unrelated")
        self.assertEqual(r.returncode, 2, r.stdout)
        self.assertNotIn("pr merge", self.calls())

    def test_a_doc_the_arm64_run_takes_waits_for_it(self):
        # The doc list comes from the workflow, not a list in the script: memory.md waits for build and test + clippy,
        # so passing checks without them never merge it (45 min, here 90 looks at once).
        _, r = self.mwg(["lampOS/docs/memory.md"], CI_CHECKS)
        self.assertEqual(r.returncode, 5, r.stdout)
        self.assertNotIn("pr merge", self.calls())

    def test_a_no_crates_train_behind_a_crate_merge_merges_as_is(self):
        h = self.pr(7, [TOOL])
        self.gain("lampOS/crates/x/src/lib.rs")
        self.fakefile("files.txt", TOOL + "\n")
        self.fakefile("head.txt", h + "\n")
        self.fakefile("checks.json", json.dumps({"statusCheckRollup": CI_CHECKS}))
        r = run(["zsh", MWG, "7", "--behind-unrelated", "--no-crates"], self.work, env=self.env, check=False)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("merge as is (no-crates train)", r.stdout)
        self.assertIn("--match-head-commit " + h, self.calls())


    def test_a_refused_merge_after_lamp_voice_moved_rebuilds(self):
        # The other lane merged between the check and this merge: exit 3 (rebuild), not 4 (stops the crate lane).
        h, r = self.mwg([TOOL], CI_CHECKS, "--behind-unrelated", env=dict(self.env, FAKE_MERGE_RC="1",
                                                                           FAKE_ON_MERGE="1"))
        self.assertEqual(r.returncode, 3, r.stdout)
        self.assertIn("lamp-voice moved meanwhile", r.stdout)

    def test_a_refused_merge_otherwise_stays_refused(self):
        _, r = self.mwg([TOOL], CI_CHECKS, "--behind-unrelated", env=dict(self.env, FAKE_MERGE_RC="1"))
        self.assertEqual(r.returncode, 4, r.stdout)


class BotDryRound(Repo):
    def bot(self, **extra):
        self.pr(7, [TOOL])
        self.pr(8, ["lampOS/crates/x/src/lib.rs"])
        self.pr(9, ["lampOS/docs/controls.md"])
        self.fakefile("prs.txt", "7\n8\n9\n")
        self.fakefile("heads.txt", "".join("%d %s\n" % (n, open(os.path.join(self.fake, "head-%d.txt" % n)).read().strip())
                                           for n in (7, 8, 9)))
        env = dict(self.env, DRY="1", WORKTREE=os.path.join(self.tmp, "wt"), WORKTREE_NC=os.path.join(self.tmp, "wtnc"), **extra)
        os.makedirs(self.state, exist_ok=True)
        r = run(["zsh", BOT], self.work, env=env, check=False)
        lines = [l.split(" ", 1)[1] for l in r.stdout.splitlines()]
        return r, lines

    def assertNothingChanged(self):
        for verb in ("pr create", "pr comment", "pr merge", "pr close", "issue comment"):
            self.assertNotIn(verb, self.calls())
        self.assertNotIn("first/train-", run(["git", "for-each-ref"], self.origin).stdout)

    def test_two_lanes_dry(self):
        r, lines = self.bot()
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(lines, ["dry: #7 -> no-crates lane",
                                 "dry: #8 -> crate lane (lampOS/crates/x/src/lib.rs (crates))",
                                 "dry: #9 -> crate lane (lampOS/docs/controls.md (a doc the SDK check reads))",
                                 "dry: crate lane: would carry 8 9 onto " + self.base[:9],
                                 "dry: no-crates lane: would carry 7 onto " + self.base[:9]])
        self.assertNothingChanged()

    def test_each_lane_waits_only_for_its_own_train(self):
        self.fakefile("open.txt", "first/train-nc-4\n")
        r, lines = self.bot()
        self.assertIn("dry: crate lane: would carry 8 9 onto " + self.base[:9], lines)
        self.assertIn("dry: no-crates lane: a train is open or running; a real run would wait for it", lines)
        self.fakefile("open.txt", "first/train-600\n")
        r, lines = self.bot()
        self.assertIn("dry: crate lane: a train is open or running; a real run would wait for it", lines)
        self.assertIn("dry: no-crates lane: would carry 7 onto " + self.base[:9], lines)

    def test_lane_off_is_the_old_single_lane(self):
        r, lines = self.bot(NOCRATES_LANE="off")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(lines, ["dry: would carry 7 8 9 onto " + self.base[:9]])
        self.fakefile("open.txt", "first/train-nc-4\n")
        r, lines = self.bot(NOCRATES_LANE="off")
        self.assertEqual(lines[0], "dry: another train is open; a real run would wait for it")
        self.assertNothingChanged()


if __name__ == "__main__":
    unittest.main()
