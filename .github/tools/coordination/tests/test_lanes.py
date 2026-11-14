# Tests for the no-crates lane (#5212 part 3): lib.sh's lane test, lane.sh, behind-unrelated.sh --no-crates,
# merge-when-green.sh on a train with no ARM64 run, and train-bot.sh's DRY=1 round with the lane on and off. A temporary
# git repository and a stand-in `gh` on PATH. Needs git, zsh and jq.
# Run from the repo root: python3 -I -m pytest -q .github/tools/coordination/tests
import json, os, re, shutil, subprocess, tempfile, time, unittest

# The Queue issues, read from lib.sh as the bot reads them, so a new Queue issue needs no test change.
QUEUE = tuple(int(n) for n in re.search(r"^COORD_QUEUE_ISSUES=\(([0-9 ]+)\)", open(os.path.join(os.path.dirname(__file__), "..", "lib.sh")).read(), re.M).group(1).split())

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
  *issues/4710/comments*|*issues/5385/comments*|*issues/6059/comments*|*issues/6809/comments*|*issues/6811/comments*) a="$*"; n=${a#*issues/}; n=${n%%/*}; [ -f "$FAKE_DIR/queue-fail-$n" ] && exit 1; cat "$FAKE_DIR/queue-$n.json" 2>/dev/null; exit 0 ;;
  *issues/*/comments*) a="$*"; n=${a#*issues/}; n=${n%%/*}; cat "$FAKE_DIR/comments-$n.json" ;;
  *"--state all"*) echo 23 ;;
  *"--json headRefName --jq"*) cat "$FAKE_DIR/open.txt" 2>/dev/null ;;
  *"number,isDraft"*) cat "$FAKE_DIR/prs.txt" ;;
  *"number,headRefOid,headRefName"*) cat "$FAKE_DIR/heads.txt" ;;
  *isCrossRepository,headRepositoryOwner*) n=$(echo "$*" | sed -E 's/^pr view ([0-9]+).*/\1/'); echo "false autonomous-ai/lampOS $(cat "$FAKE_DIR/head-$n.txt")" ;;
  *"--json body"*) echo '{"login":"","body":"","pr_body":true}' ;;
  *title,createdAt*) n=$(echo "$*" | sed -E 's/^pr view ([0-9]+).*/\1/'); if [ -f "$FAKE_DIR/title-$n.txt" ]; then cat "$FAKE_DIR/title-$n.txt"; else printf 'lampOS: a change\t2026-11-04T01:00:00Z\n'; fi ;;
  *headRefOid*) cat "$FAKE_DIR/head.txt" ;;
  *statusCheckRollup*) n=$(echo "$*" | sed -E 's/^pr view ([0-9]+).*/\1/'); if [ -f "$FAKE_DIR/checks-$n.json" ]; then cat "$FAKE_DIR/checks-$n.json"; elif [ -f "$FAKE_DIR/checks.json" ]; then cat "$FAKE_DIR/checks.json"; else echo '{"statusCheckRollup":[]}'; fi ;;
  "run list"*) echo 0 ;;
  "pr merge"*) if [ -n "$FAKE_MERGE_5XX" ]; then n=$(cat "$FAKE_DIR/5xx" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$FAKE_DIR/5xx"
                 [ "$n" -le "$FAKE_MERGE_5XX" ] && { [ -n "$FAKE_ON_5XX" ] && sh -c "$FAKE_ON_5XX"; echo "HTTP 502: Bad Gateway (https://api.github.com/graphql)" >&2; exit 1; }
                 [ -n "$FAKE_RETRY_WAS_MERGED" ] && echo MERGED > "$FAKE_DIR/state.txt"; fi
               [ -n "$FAKE_ON_MERGE" ] && git push -q origin "$(git commit-tree -p origin/lamp-voice -m moved origin/lamp-voice^{tree})":refs/heads/lamp-voice; exit "${FAKE_MERGE_RC:-0}" ;;
  *"--json state"*) cat "$FAKE_DIR/state.txt" 2>/dev/null || echo OPEN ;;
esac
"""
ARM_CHECKS = [{"__typename": "CheckRun", "name": "build and test", "status": "COMPLETED", "conclusion": "SUCCESS"},
              {"__typename": "CheckRun", "name": "clippy", "status": "COMPLETED", "conclusion": "SUCCESS"}]
# A PR's own ARM64 run with every job green, as the train bot needs it on a PR the ARM64 run takes (#6556, train 841).
ARM = "lampOS — build and test on Linux ARM64 (%s)"
ARM_GREEN = [{"__typename": "CheckRun", "name": ARM % j, "status": "COMPLETED", "conclusion": "SUCCESS"} for j in ("build", "release", "1/1")]
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

    def pr(self, n, files, base=None, body=None):
        """PR n: one commit changing files (to body, default "pr N") on top of base (default: the template's lamp-voice),
        pushed as its head."""
        self.git("checkout", "-q", "--detach", base or self.base)
        for f in files:
            self.write(f, body or "pr %d\n" % n)
        self.commit("pr %d" % n)
        h = self.rev("HEAD")
        self.git("push", "-q", "-f", "origin", "HEAD:refs/pull/%d/head" % n)
        self.git("fetch", "-q", "origin", "+refs/pull/%d/head:refs/remotes/pr/%d" % (n, n))
        self.fakefile("head-%d.txt" % n, h + "\n")
        # 13 Nov: every PR needs two signed readers (reads2.py --always), so a ready PR here has two. 14 Nov: and its lamp
        # row (rows.py), here a reader's "no row needed".
        self.fakefile("comments-%d.json" % n,
                      json.dumps({"login": "w1", "body": "READ-OK " + h + "\n\nReviewer-lead, 01:00",
                                  "created_at": "2026-11-04T01:00:00Z"}) + "\n"
                      + json.dumps({"login": "w1", "body": "READ-OK " + h + "\n\nHeron, 01:05: no row needed, tests only.",
                                    "created_at": "2026-11-04T01:05:00Z"}) + "\n")
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

    def test_a_path_a_workflow_adds_is_an_input(self):
        # Thirteen's P3 on #5297: a path either workflow's pull_request paths gain outside coord_input's fixed cases is an
        # input, so its PR rides the crate lane, as merge-when-green.sh waits for that run.
        self.git("checkout", "-q", "--detach", self.base)
        self.write(".github/workflows/lampos-arm64.yml", ARM64.replace("      - '.github/tools/lampos-promote/**'\n",
                                                                       "      - '.github/tools/lampos-promote/**'\n      - 'tools/foo/**'\n"))
        self.write(".github/workflows/lampos-sdk.yml", SDK.replace("      - 'lampOS/sdk/**'\n", "      - 'lampOS/sdk/**'\n      - 'sdkbits/**'\n"))
        self.commit("workflows gain paths")
        self.git("push", "-q", "origin", "HEAD:lamp-voice")
        self.git("fetch", "-q", "origin")
        base = self.rev("HEAD")

        def lane(files):
            h = self.pr(9, files, base=base)
            r = run(["zsh", "-c", 'source "$1"; coord_inputs_init origin/lamp-voice; coord_lane "$2"', "x", LIB, h],
                    self.work, env=self.env)
            return r.stdout.strip()

        self.assertEqual(lane(["tools/foo/x.sh"]), "crates: tools/foo/x.sh (an ARM64 input)")
        self.assertEqual(lane(["sdkbits/a.js"]), "crates: sdkbits/a.js (an input the SDK check takes)")
        for files in (["tools/bar/x.sh"], [TOOL], ["system/main.go"], ["lampOS/builders/twelve/log.md"], ["lampOS/docs/other.md"]):
            self.assertEqual(lane(files), "nocrates", files)

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

    # 5 Nov (the PM, train 662): GitHub answered a green train's merge with a 502 and the bot stopped.
    def merges(self):
        return [c for c in self.calls().splitlines() if c.startswith("pr merge")]

    def test_a_5xx_on_the_merge_is_tried_again(self):
        _, r = self.mwg([TOOL], CI_CHECKS, env=dict(self.env, FAKE_MERGE_5XX="2", MWG_RETRY_S="0"))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(len(self.merges()), 3)
        self.assertIn("merge answered with GitHub's 5xx (try 2 of 3)", r.stdout)
        self.assertIn("merged at", r.stdout)

    def test_a_5xx_after_the_merge_went_through_is_a_merge(self):
        self.fakefile("state.txt", "MERGED\n")
        _, r = self.mwg([TOOL], CI_CHECKS, env=dict(self.env, FAKE_MERGE_5XX="9", MWG_RETRY_S="0"))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(len(self.merges()), 1)

    def test_three_5xx_rebuild_and_never_stop_the_bot(self):
        _, r = self.mwg([TOOL], CI_CHECKS, env=dict(self.env, FAKE_MERGE_5XX="9", MWG_RETRY_S="0"))
        self.assertEqual(r.returncode, 7, r.stdout + r.stderr)
        self.assertEqual(len(self.merges()), 3)
        self.assertIn("5xx 3 times; rebuilding", r.stdout)

    def test_a_5xx_then_a_moved_lamp_voice_rebuilds_and_never_merges_on_it(self):
        # The first read's P2: lamp-voice has no branch protection, so nothing else refuses a retry onto a newer tip.
        moved = 'git push -q origin "$(git commit-tree -p origin/lamp-voice -m moved origin/lamp-voice^{tree})":refs/heads/lamp-voice'
        _, r = self.mwg([TOOL], CI_CHECKS, env=dict(self.env, FAKE_MERGE_5XX="1", FAKE_ON_5XX=moved, MWG_RETRY_S="0"))
        self.assertEqual(r.returncode, 3, r.stdout + r.stderr)
        self.assertEqual(len(self.merges()), 1)
        self.assertIn("green but behind lamp-voice; not merged", r.stdout)

    def test_a_5xx_then_a_hold_on_a_carried_pr_does_not_merge(self):
        hold = ('printf \'{"login":"w1","body":"HOLD %s","created_at":"2099-01-01T00:00:00Z"}\\n\' '
                '"$(cat "$FAKE_DIR/head.txt")" >> "$FAKE_DIR/comments-7.json"')
        _, r = self.mwg([TOOL], CI_CHECKS, "--precheck", "7",
                        env=dict(self.env, FAKE_MERGE_5XX="1", FAKE_ON_5XX=hold, MWG_RETRY_S="0"))
        self.assertEqual(r.returncode, 6, r.stdout + r.stderr)
        self.assertEqual(len(self.merges()), 1)

    def test_a_retry_refused_as_already_merged_is_a_merge(self):
        # The first read's P3: GitHub had merged but still said OPEN just after the 5xx.
        _, r = self.mwg([TOOL], CI_CHECKS, env=dict(self.env, FAKE_MERGE_5XX="1", FAKE_RETRY_WAS_MERGED="1",
                                                     FAKE_MERGE_RC="1", MWG_RETRY_S="0"))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(len(self.merges()), 2)


class BotDryRound(Repo):
    def bot(self, before=None, again=False, **extra):
        # again: the same PRs and heads as the last round (a new commit could get a new head).
        if not again:
            self.pr(7, [TOOL])
            self.pr(8, ["lampOS/crates/x/src/lib.rs"])
            self.pr(9, ["lampOS/docs/controls.md"])
        self.fakefile("prs.txt", "7\n8\n9\n")
        self.fakefile("heads.txt", "".join("%d %s\n" % (n, open(os.path.join(self.fake, "head-%d.txt" % n)).read().strip())
                                           for n in (7, 8, 9)))
        self.fakefile("checks-8.json", json.dumps({"statusCheckRollup": ARM_GREEN}))
        if before:
            before()
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

    def rows(self, *ns):
        # PRs ns titled as rows PRs, opened a minute ago (well under the 3 h wait).
        at = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() - 60))
        for n in ns:
            self.fakefile("title-%d.txt" % n, "lampOS: docs/build-rows.md as at 05 Nov 02:00\t%s\n" % at)

    def test_a_rows_pr_alone_in_the_no_crates_lane_goes_at_once(self):
        # Thirteen's P3 on #5297: no code PR rides the no-crates lane, so a rows PR there doesn't wait the 3 h.
        self.rows(7)
        r, lines = self.bot()
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("dry: no-crates lane: would carry 7 onto " + self.base[:9], lines)
        self.assertNotIn("dry: no-crates lane: rows PRs alone, waiting", lines)
        self.assertNothingChanged()

    def test_rows_prs_alone_still_wait_in_the_crate_lane_and_one_lane(self):
        self.rows(7, 8, 9)
        r, lines = self.bot()
        self.assertIn("dry: crate lane: rows PRs alone, waiting", lines)
        self.assertIn("dry: no-crates lane: would carry 7 onto " + self.base[:9], lines)
        r, lines = self.bot(NOCRATES_LANE="off")
        self.assertEqual(lines, ["dry: rows PRs alone, waiting"])
        self.assertNothingChanged()

    def test_lane_off_is_the_old_single_lane(self):
        r, lines = self.bot(NOCRATES_LANE="off")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(lines, ["dry: would carry 7 8 9 onto " + self.base[:9]])
        self.fakefile("open.txt", "first/train-nc-4\n")
        r, lines = self.bot(NOCRATES_LANE="off")
        self.assertEqual(lines[0], "dry: another train is open; a real run would wait for it")
        self.assertNothingChanged()

    # 5 Nov: #4710 hit GitHub's 2,500-comment limit and the Queue goes on in #5385; lib.sh's coord_queue reads both.
    def queue_hold(self, issue, n):
        h = open(os.path.join(self.fake, "head-%d.txt" % n)).read().strip()
        self.fakefile("queue-%d.json" % issue, json.dumps({"login": "w1", "body": "HOLD %s\n\nEngineer, 12:30: lamp check pending" % h[:10],
                                                           "created_at": "2026-11-05T05:30:00Z"}) + "\n")

    def test_a_hold_on_the_new_queue_holds_its_pr(self):
        r, lines = self.bot(before=lambda: self.queue_hold(5385, 8))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("#8 left out: a HOLD on the Queue (2026-11-05T05:30:00Z) names head", r.stderr)
        self.assertIn("dry: crate lane: would carry 9 onto " + self.base[:9], lines)
        self.assertIn("dry: no-crates lane: would carry 7 onto " + self.base[:9], lines)
        self.assertIn("issues/4710/comments", self.calls())
        self.assertIn("issues/5385/comments", self.calls())
        self.assertNothingChanged()

    def test_a_hold_on_the_old_queue_still_holds_its_pr(self):
        r, lines = self.bot(before=lambda: self.queue_hold(4710, 7))
        self.assertIn("#7 left out: a HOLD on the Queue (2026-11-05T05:30:00Z) names head", r.stderr)
        self.assertIn("dry: crate lane: would carry 8 9 onto " + self.base[:9], lines)
        self.assertIn("dry: no-crates lane: nothing eligible", lines)
        self.assertNothingChanged()

    # 7 Nov (#5653's P3s): when the PR's commits can't be listed (git rev-list origin/lamp-voice..HEAD fails), the PR
    # sits out the round, in the bot and in still-eligible.sh, instead of reading Queue asks against its head alone.
    def unlisted(self, n):
        bogus = "f" * 40
        self.fakefile("head-%d.txt" % n, bogus + "\n")
        self.fakefile("comments-%d.json" % n, json.dumps({"login": "w1", "body": "READ-OK " + bogus,
                                                          "created_at": "2026-11-04T01:00:00Z"}) + "\n")

    def test_a_pr_whose_commits_cant_be_listed_sits_out(self):
        r, lines = self.bot(before=lambda: self.unlisted(7))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("#7 left out: its commits can't be listed", r.stderr)
        self.assertIn("dry: crate lane: would carry 8 9 onto " + self.base[:9], lines)
        self.assertIn("dry: no-crates lane: nothing eligible", lines)
        self.assertNothingChanged()

    def test_still_eligible_fails_when_commits_cant_be_listed(self):
        self.pr(8, ["lampOS/crates/x/src/lib.rs"])
        os.makedirs(self.state, exist_ok=True)
        r = run(["zsh", os.path.join(HERE, "still-eligible.sh"), "8"], self.work, env=self.env, check=False)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.unlisted(8)
        r = run(["zsh", os.path.join(HERE, "still-eligible.sh"), "8"], self.work, env=self.env, check=False)
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("#8: its commits can't be listed", r.stdout)

    # 13 Nov (Heron's P3 on #6959): every PR needs two signed readers, in the bot and in still-eligible.sh.
    def one_read(self, n):
        with open(os.path.join(self.fake, "head-%d.txt" % n)) as f:
            h = f.read().strip()
        self.fakefile("comments-%d.json" % n, json.dumps({"login": "w1", "body": "READ-OK " + h + "\n\nReviewer-lead, 01:00",
                                                          "created_at": "2026-11-04T01:00:00Z"}) + "\n")

    def test_a_pr_with_one_signed_read_sits_out(self):
        r, lines = self.bot(before=lambda: self.one_read(7))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("#7 left out: two reads needed, short 1", r.stderr)
        self.assertIn("dry: crate lane: would carry 8 9 onto " + self.base[:9], lines)
        self.assertNothingChanged()

    def test_still_eligible_fails_on_one_signed_read(self):
        self.pr(8, ["lampOS/crates/x/src/lib.rs"])
        os.makedirs(self.state, exist_ok=True)
        r = run(["zsh", os.path.join(HERE, "still-eligible.sh"), "8"], self.work, env=self.env, check=False)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.one_read(8)
        r = run(["zsh", os.path.join(HERE, "still-eligible.sh"), "8"], self.work, env=self.env, check=False)
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("#8: TWO READS short 1", r.stdout)

    # 14 Nov (the lead): a PR's lamp row comes before its train: a ROW line on the PR, or a reader's "no row needed" in a
    # READ-OK on its head (rows.py), in the bot and in still-eligible.sh.
    def no_row(self, n, *more):
        with open(os.path.join(self.fake, "head-%d.txt" % n)) as f:
            h = f.read().strip()
        cs = [{"login": "w1", "body": "READ-OK " + h + "\n\nReviewer-lead, 01:00", "created_at": "2026-11-04T01:00:00Z"},
              {"login": "w1", "body": "READ-OK " + h + "\n\nHeron, 01:05", "created_at": "2026-11-04T01:05:00Z"}]
        cs += [{"login": "w1", "body": b % h, "created_at": "2026-11-04T01:10:00Z"} for b in more]
        self.fakefile("comments-%d.json" % n, "".join(json.dumps(c) + "\n" for c in cs))
        return h

    def test_a_pr_with_two_reads_and_no_row_sits_out(self):
        hs = []
        r, lines = self.bot(before=lambda: hs.append(self.no_row(7)))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn('#7 left out: no ROW line and no reader\'s "no row needed" on head %s' % hs[0][:9], r.stderr)
        self.assertIn("dry: crate lane: would carry 8 9 onto " + self.base[:9], lines)
        self.assertIn("dry: no-crates lane: nothing eligible", lines)
        # Said once per head.
        r, lines = self.bot(again=True)
        self.assertNotIn("#7 left out: no ROW line", r.stderr)
        self.assertNothingChanged()

    def test_a_row_line_carries_it(self):
        for row in ("ROW 7 %s on build 12700: the journal line", "**ROW** 7 %s on build 12700",
                    "Fifth, 09:51 GMT+7: lamp row for #7 on lamp-52e6 at %s"):
            shutil.rmtree(self.state, ignore_errors=True)
            r, lines = self.bot(before=lambda: self.no_row(7, row))
            self.assertIn("dry: no-crates lane: would carry 7 onto " + self.base[:9], lines, row)

    def test_a_quoted_row_or_another_prs_row_does_not(self):
        # 14 Nov (the readers' P3s on #7034): a ROW line names its PR, and a FAIL row is no row.
        for row in ("> ROW 7 %s on build 12700", "lamp row for #70 on lamp-52e6 at %s", "ROW 8 %s on build 12700",
                    "ROW %s on build 12700", "ROW 7 %s on build 12700: FAIL, the journal line",
                    "lamp row for #7 at %s: VOID, the kit was wrong"):
            shutil.rmtree(self.state, ignore_errors=True)
            r, lines = self.bot(before=lambda: self.no_row(7, row))
            self.assertIn("dry: no-crates lane: nothing eligible", lines, row)

    def test_no_row_needed_in_a_read_ok_on_the_head_carries_it(self):
        r, lines = self.bot(before=lambda: self.no_row(7, "READ-OK %s\n\nSixth, 01:10: docs only, no row needed."))
        self.assertIn("dry: no-crates lane: would carry 7 onto " + self.base[:9], lines)
        self.assertNotIn("#7 left out", r.stderr)

    def test_no_row_needed_outside_a_read_ok_does_not(self):
        r, lines = self.bot(before=lambda: self.no_row(7, "Sixth, 01:10: on %s, no row needed."))
        self.assertIn("#7 left out: no ROW line", r.stderr)
        self.assertIn("dry: no-crates lane: nothing eligible", lines)

    def test_still_eligible_fails_with_no_row(self):
        self.pr(8, ["lampOS/crates/x/src/lib.rs"])
        os.makedirs(self.state, exist_ok=True)
        self.no_row(8)
        r = run(["zsh", os.path.join(HERE, "still-eligible.sh"), "8"], self.work, env=self.env, check=False)
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("#8: no lamp row (rows.py: none)", r.stdout)
        self.no_row(8, "ROW 8 %s on build 12700")
        r = run(["zsh", os.path.join(HERE, "still-eligible.sh"), "8"], self.work, env=self.env, check=False)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)

    # 14 Nov (the readers' P3s on #7034): a PR whose diff turns a LAMP_ switch on in a product unit needs a ROW line;
    # a reader's "no row needed" doesn't carry it.
    def switch(self, value, *more):
        def make():
            self.pr(8, ["lampOS/deploy/product/lamp-x.service"], body="[Service]\nEnvironment=LAMP_X=%s\n" % value)
            self.fakefile("heads.txt", "".join("%d %s\n" % (n, open(os.path.join(self.fake, "head-%d.txt" % n)).read().strip())
                                               for n in (7, 8, 9)))
            if more:
                with open(os.path.join(self.fake, "head-8.txt")) as f:
                    h = f.read().strip()
                with open(os.path.join(self.fake, "comments-8.json"), "a") as f:
                    for b in more:
                        f.write(json.dumps({"login": "w1", "body": b % h, "created_at": "2026-11-04T01:10:00Z"}) + "\n")
        return make

    def test_a_switch_on_with_no_row_needed_sits_out(self):
        r, lines = self.bot(before=self.switch("on"))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        h8 = open(os.path.join(self.fake, "head-8.txt")).read().strip()
        self.assertIn("#8 left out: it turns a switch on in the product, and no ROW line names it on head " + h8[:9], r.stderr)
        self.assertIn("dry: crate lane: would carry 9 onto " + self.base[:9], lines)

    def test_a_switch_on_with_its_row_is_carried(self):
        r, lines = self.bot(before=self.switch("on", "ROW #8 %s on build 12700: PASS, the journal line"))
        self.assertIn("dry: crate lane: would carry 8 9 onto " + self.base[:9], lines, r.stderr)

    def test_a_switch_off_carries_on_no_row_needed(self):
        r, lines = self.bot(before=self.switch("off"))
        self.assertIn("dry: crate lane: would carry 8 9 onto " + self.base[:9], lines, r.stderr)

    def test_still_eligible_asks_a_switch_on_for_its_row(self):
        os.makedirs(self.state, exist_ok=True)
        self.pr(7, [TOOL]); self.pr(9, [TOOL])
        self.switch("on")()
        se = lambda: run(["zsh", os.path.join(HERE, "still-eligible.sh"), "8"], self.work, env=self.env, check=False)
        r = se()
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("#8: no lamp row (rows.py: none)", r.stdout)
        self.switch("on", "ROW 8 %s on build 12700")()
        self.assertEqual(se().returncode, 0)

    def switch_on(self, head):
        return run(["zsh", "-c", 'source "$1"; coord_switch_on "$2"', "x", LIB, head], self.work, env=self.env, check=False)

    def test_coord_switch_on_reads_product_units_only(self):
        for files, body, want in ((["lampOS/deploy/product/sub/lamp-y.conf"], "[Service]\nEnvironment=LAMP_Y=on\n", "on"),
                                  (["lampOS/deploy/other/lamp-y.service"], "Environment=LAMP_Y=on\n", "off"),
                                  (["lampOS/deploy/product/lamp-y.service"], "Environment=LAMP_Y=off\n", "off"),
                                  (["lampOS/deploy/product/lamp-y.service"], "# Environment=LAMP_Y=on\n", "off")):
            h = self.pr(8, files, body=body)
            r = self.switch_on(h)
            self.assertEqual((r.returncode, r.stdout.strip()), (0, want), (files, body))

    def test_coord_switch_on_reads_settings_and_any_value_but_off(self):
        # 14 Nov (the Reviewer-lead's P3s on #7041): the settings files count, and so does a switch set to a word.
        env = ["lampOS/deploy/product/settings/x.env"]
        unit = ["lampOS/deploy/product/lamp-y.service"]
        for files, body, want in ((env, "LAMP_X=on\n", "on"), (env, "LAMP_NAMES=checked\n", "on"),
                                  (env, "LAMP_ATTENTION=busy-near\n", "on"), (env, 'LAMP_SHORT_SOUNDS="quiet-room-checked"\n', "on"),
                                  (env, "LAMP_ACTIVITY='place'\n", "on"), (unit, "Environment=LAMP_ACTIVITY=place\n", "on"),
                                  (unit, 'Environment="LAMP_X=on"\n', "on"),
                                  (env, "LAMP_X=off\n", "off"), (env, "LAMP_X=0\n", "off"), (env, "LAMP_X=false\n", "off"),
                                  (env, "LAMP_X=\n", "off"), (env, 'LAMP_X=""\n', "off"), (unit, "Environment=LAMP_X=Off\n", "off"),
                                  (env, "# LAMP_X=on\n", "off"), (env, "#LAMP_X=checked\n", "off"),
                                  (["lampOS/deploy/other/x.env"], "LAMP_X=on\n", "off"),
                                  (["lampOS/deploy/product/lamp-y.service"], "LAMP_X=on\n", "on"),
                                  # 14 Nov (the second reader's P3 on #7041): every assignment on the line.
                                  (unit, "Environment=FOO=1 LAMP_X=on\n", "on"), (unit, 'Environment="A=1" "LAMP_X=on"\n', "on"),
                                  (unit, "Environment=LAMP_A=off LAMP_B=checked\n", "on"),
                                  (unit, "Environment=LAMP_A=off LAMP_B=0\n", "off"), (unit, "Environment=FOO_LAMP_X=on\n", "off"),
                                  (env, "LAMP_A=off # LAMP_X=on\n", "off")):
            h = self.pr(8, files, body=body)
            r = self.switch_on(h)
            self.assertEqual((r.returncode, r.stdout.strip()), (0, want), (files, body))

    def test_coord_switch_on_fails_closed_on_an_unread_diff(self):
        self.assertNotEqual(self.switch_on("f" * 40).returncode, 0)

    # 14 Nov: trains 905-907 left out #7016 and #7022 with no line because train-only.txt didn't name them. Each PR
    # train-hold.txt or train-only.txt drops is said once per head.
    def state_file(self, name, body):
        os.makedirs(self.state, exist_ok=True)
        with open(os.path.join(self.state, name), "w") as f:
            f.write(body)

    def test_the_prs_train_only_and_train_hold_drop_are_said(self):
        r, lines = self.bot(before=lambda: (self.state_file("train-only.txt", "8\n9\n"), self.state_file("train-hold.txt", "9\n")))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("#9 left out: named in train-hold.txt", r.stderr)
        self.assertIn("#7 left out: not named in train-only.txt", r.stderr)
        self.assertNotIn("#8 left out", r.stderr)
        self.assertIn("dry: crate lane: would carry 8 onto " + self.base[:9], lines)
        self.assertIn("dry: no-crates lane: nothing eligible", lines)
        # Said once per head.
        r, lines = self.bot(again=True)
        self.assertNotIn("left out: not named in train-only.txt", r.stderr)
        self.assertNotIn("left out: named in train-hold.txt", r.stderr)
        self.assertNothingChanged()

    def test_either_queue_unread_makes_nothing_eligible(self):
        for issue in QUEUE:
            for f in os.listdir(self.fake):
                if f.startswith("queue-fail-"):
                    os.unlink(os.path.join(self.fake, f))
            self.fakefile("queue-fail-%d" % issue, "")
            shutil.rmtree(self.state, ignore_errors=True)
            r, lines = self.bot()
            self.assertIn("the Queue's (%s) comments can't be fetched: no PR is eligible" % ", ".join("#%d" % n for n in QUEUE), r.stderr)
            self.assertNotIn("would carry", " ".join(lines))
            self.assertNothingChanged()


class CoordQueue(Repo):
    """lib.sh's coord_queue: every Queue issue in one file, or a failure with no file (fail closed)."""

    def queue(self):
        out = os.path.join(self.tmp, "q.jsonl")
        r = run(["zsh", "-c", 'TOOLS_DIR=%s; source %s; coord_queue %s' % (HERE, LIB, out)], self.work, env=self.env, check=False)
        return r.returncode, (open(out).read().splitlines() if os.path.exists(out) else None)

    def test_both_issues_in_one_list(self):
        self.fakefile("queue-4710.json", '{"login":"w1","body":"old","created_at":"2026-11-04T01:00:00Z"}\n')
        self.fakefile("queue-5385.json", '{"login":"w1","body":"new","created_at":"2026-11-05T01:00:00Z"}\n')
        rc, lines = self.queue()
        self.assertEqual(rc, 0)
        self.assertEqual([json.loads(l)["body"] for l in lines], ["old", "new"])

    def test_either_issue_failing_fails_with_no_file(self):
        for issue in QUEUE:
            self.fakefile("queue-fail-%d" % issue, "")
            rc, lines = self.queue()
            self.assertNotEqual(rc, 0)
            self.assertIsNone(lines)
            os.unlink(os.path.join(self.fake, "queue-fail-%d" % issue))

    def test_the_bot_posts_on_the_last_issue(self):
        r = run(["zsh", "-c", 'TOOLS_DIR=%s; source %s; print -r -- $COORD_QUEUE_POST ${(j:,:)COORD_QUEUE_ISSUES}' % (HERE, LIB)], self.work, env=self.env)
        self.assertEqual(r.stdout.split(), [str(QUEUE[-1]), ",".join(map(str, QUEUE))])

    def test_a_refused_queue_post_is_logged_not_dropped(self):
        # The Reviewer-lead's P3 on #5390: when the Queue issue is full, the bot's post fails; it is said, not lost.
        fn = 'say() { print -r -- "said: $*"; }; source %s; eval "$(sed -n \'/^queue_post()/,/^}/p\' %s)"; queue_post "train 9 merged"' % (LIB, BOT)
        ok = run(["zsh", "-c", fn], self.work, env=self.env)
        self.assertEqual(ok.stdout + ok.stderr, "")
        gh_fails = os.path.join(self.tmp, "failgh")
        os.makedirs(gh_fails)
        with open(os.path.join(gh_fails, "gh"), "w") as f:
            f.write("#!/bin/sh\nexit 1\n")
        os.chmod(os.path.join(gh_fails, "gh"), 0o755)
        r = run(["zsh", "-c", fn], self.work, env=dict(self.env, PATH=gh_fails + os.pathsep + self.env["PATH"]))
        self.assertIn("said: the Queue (#%d) refused the bot's post" % QUEUE[-1], r.stdout + r.stderr)
        self.assertIn("train 9 merged", r.stdout + r.stderr)


if __name__ == "__main__":
    unittest.main()
