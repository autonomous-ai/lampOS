# Tests for behind-unrelated.sh and merge-when-green.sh's --behind-unrelated path (#5212 part 2), on a temporary git
# repository with a stand-in `gh` on PATH. Needs git, zsh and jq.
# Run from the repo root: python3 -I -m pytest -q .github/tools/coordination/tests
import os, shutil, subprocess, tempfile, unittest

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BEHIND = os.path.join(HERE, "behind-unrelated.sh")
MWG = os.path.join(HERE, "merge-when-green.sh")
WORKFLOW = """on:
  pull_request:
    paths:
      - 'lampOS/**'
      - '!lampOS/docs/**/*.md'
      - '!lampOS/builders/**'
      - 'lampOS/docs/memory.md'
"""
# lampos-sdk.yml's paths as on lamp-voice: the SDK docs it takes are inputs too (Thirteen's P3 on #5289).
SDK_WORKFLOW = """on:
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
# The stand-in gh: answers the calls merge-when-green.sh and lv-quiet.sh make, and logs every call.
FAKE_GH = r"""#!/bin/sh
echo "$*" >> "$FAKE_DIR/calls.log"
case "$*" in
  "pr diff"*) cat "$FAKE_DIR/files.txt" ;;
  *headRefOid*) cat "$FAKE_DIR/head.txt" ;;
  *statusCheckRollup*) cat "$FAKE_DIR/checks.json" ;;
  "run list"*) echo 0 ;;
  "pr merge"*) exit "${FAKE_MERGE_RC:-0}" ;;
esac
"""
CHECKS = ('{"statusCheckRollup":[{"__typename":"CheckRun","name":"build and test","status":"COMPLETED","conclusion":"SUCCESS"},'
          '{"__typename":"CheckRun","name":"clippy","status":"COMPLETED","conclusion":"SUCCESS"}]}')
TRAIN_FILE = "lampOS/sdk/js/README.md"


def run(args, cwd, env=None, check=True):
    return subprocess.run(args, cwd=cwd, env=env, check=check, capture_output=True, text=True)


class Repo(unittest.TestCase):
    """A lamp-voice with a base, a train (PR 7) on it, and lamp-voice moved on by one commit the test chooses."""

    @classmethod
    def setUpClass(cls):
        # git is slow on a loaded machine: the repositories are made once and copied for each test.
        cls.template = tempfile.mkdtemp()
        t = cls.__new__(cls)
        t.tmp = cls.template
        t.build()
        cls.base, cls.train = t.base, t.train

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.template, ignore_errors=True)

    def setUp(self):
        self.tmp = os.path.join(tempfile.mkdtemp(), "t")
        shutil.copytree(self.template, self.tmp, symlinks=True)
        self.paths()
        self.git("remote", "set-url", "origin", self.origin)

    def paths(self):
        self.origin = os.path.join(self.tmp, "origin.git")
        self.work = os.path.join(self.tmp, "work")
        self.fake = os.path.join(self.tmp, "fake")
        self.env = dict(os.environ, REPO_DIR=self.work, TOOLS_DIR=HERE, FAKE_DIR=self.fake,
                        PATH=self.fake + os.pathsep + os.environ["PATH"])
        self.env.pop("DRY", None)
        self.env.pop("LV_AT", None)

    def build(self):
        self.paths()
        os.makedirs(self.fake)
        run(["git", "init", "-q", "--bare", self.origin], self.tmp)
        run(["git", "init", "-q", "-b", "lamp-voice", self.work], self.tmp)
        self.git("config", "user.email", "t@example.invalid")
        self.git("config", "user.name", "t")
        self.git("remote", "add", "origin", self.origin)
        for f, body in [("lampOS/crates/x/src/lib.rs", "x\n"), ("Cargo.lock", "x\n"), ("lampOS/docs/memory.md", "x\n"),
                        ("lampOS/docs/other.md", "x\n"), (".github/workflows/lampos-arm64.yml", WORKFLOW),
                        (".github/workflows/lampos-sdk.yml", SDK_WORKFLOW),
                        (TRAIN_FILE, "x\n"), ("robots/lamp/hardware/test/tools/factory-run.sh", "x\n")]:
            self.write(f, body)
        self.commit("base")
        self.git("push", "-q", "origin", "lamp-voice")
        self.base = self.rev("HEAD")
        self.write(TRAIN_FILE, "train\n")
        self.commit("train")
        self.train = self.rev("HEAD")
        self.git("push", "-q", "origin", "HEAD:refs/pull/7/head")
        self.git("checkout", "-q", "--detach", self.base)
        with open(os.path.join(self.fake, "gh"), "w") as fh:
            fh.write(FAKE_GH)
        os.chmod(os.path.join(self.fake, "gh"), 0o755)
        for name, body in [("files.txt", TRAIN_FILE + "\n"), ("head.txt", self.train + "\n"), ("checks.json", CHECKS)]:
            with open(os.path.join(self.fake, name), "w") as fh:
                fh.write(body)

    def tearDown(self):
        shutil.rmtree(os.path.dirname(self.tmp), ignore_errors=True)

    def git(self, *a):
        return run(["git", *a], self.work).stdout

    def rev(self, ref):
        return self.git("rev-parse", ref).strip()

    def write(self, f, body):
        p = os.path.join(self.work, f)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as fh:
            fh.write(body)

    def commit(self, msg):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", msg)

    def gain(self, f, push=True):
        """lamp-voice gains a change to f, on top of the train's base."""
        self.write(f, "gained\n")
        self.commit("gain " + f)
        if push:
            self.git("push", "-q", "origin", "HEAD:lamp-voice")
            self.git("fetch", "-q", "origin")
        return self.rev("HEAD")

    def behind(self, lv="HEAD"):
        return run(["zsh", BEHIND, self.train, lv], self.work, env=self.env, check=False)

    def mwg(self, *a, env=None):
        return run(["zsh", MWG, "7", *a], self.work, env=env or self.env, check=False)

    def calls(self):
        p = os.path.join(self.fake, "calls.log")
        return open(p).read() if os.path.exists(p) else ""


class Decision(Repo):
    def test_disjoint_and_no_build_input_merges_as_is(self):
        self.gain("robots/lamp/hardware/test/tools/factory-run.sh", push=False)
        r = self.behind()
        self.assertEqual(r.returncode, 0, r.stdout)
        self.assertTrue(r.stdout.startswith("merge as is: gained 1 files"), r.stdout)

    def test_a_skipped_doc_and_a_builder_log_merge_as_is(self):
        self.gain("lampOS/docs/other.md", push=False)
        self.gain("lampOS/builders/twelve/log.md", push=False)
        r = self.behind()
        self.assertEqual(r.returncode, 0, r.stdout)

    def test_overlap_rebuilds(self):
        self.gain(TRAIN_FILE, push=False)
        r = self.behind()
        self.assertEqual(r.returncode, 1)
        self.assertIn("rebuild:", r.stdout)
        self.assertIn("also in the train: " + TRAIN_FILE, r.stdout)

    def test_a_gained_crate_file_rebuilds(self):
        self.gain("lampOS/crates/x/src/lib.rs", push=False)
        r = self.behind()
        self.assertEqual(r.returncode, 1)
        self.assertIn("lampOS/crates/x/src/lib.rs (crates)", r.stdout)

    def test_cargo_lock_rebuilds(self):
        self.gain("Cargo.lock", push=False)
        r = self.behind()
        self.assertEqual(r.returncode, 1)
        self.assertIn("Cargo.lock (Cargo)", r.stdout)

    def test_a_nested_cargo_toml_rebuilds(self):
        self.gain("tools/other/Cargo.toml", push=False)
        self.assertEqual(self.behind().returncode, 1)

    def test_a_workflow_rebuilds(self):
        self.gain(".github/workflows/other.yml", push=False)
        r = self.behind()
        self.assertEqual(r.returncode, 1)
        self.assertIn("(workflows)", r.stdout)

    def test_a_doc_the_arm64_run_reads_rebuilds(self):
        self.gain("lampOS/docs/memory.md", push=False)
        r = self.behind()
        self.assertEqual(r.returncode, 1)
        self.assertIn("a doc the ARM64 run reads", r.stdout)

    def test_a_doc_the_sdk_check_reads_rebuilds(self):
        # Thirteen's P3 on #5289: lampos-sdk.yml takes lampOS/docs/control* and skill-format*, Markdown included.
        for f in ("lampOS/docs/controls.md", "lampOS/docs/skill-format.md"):
            r = self.behind(self.gain(f, push=False))
            self.assertEqual(r.returncode, 1, r.stdout)
            self.assertIn(f + " (a doc the SDK check reads)", r.stdout)

    def test_another_arm64_input_rebuilds(self):
        self.gain("lampOS/deploy/product/settings/records.json", push=False)
        self.assertEqual(self.behind().returncode, 1)

    def test_not_behind_rebuilds(self):
        r = self.behind(self.base)
        self.assertEqual(r.returncode, 1)
        self.assertIn("not behind", r.stdout)

    def test_an_unreadable_head_rebuilds(self):
        r = run(["zsh", BEHIND, "f" * 40, "HEAD"], self.work, env=self.env, check=False)
        self.assertEqual(r.returncode, 1)
        self.assertIn("can't be read", r.stdout)


class MergeWhenGreen(Repo):
    def test_merges_as_is_pinned_to_its_head(self):
        self.gain("robots/lamp/hardware/test/tools/factory-run.sh")
        r = self.mwg("--behind-unrelated")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("merge as is", r.stdout)
        self.assertIn("pr merge 7 -R autonomous-ai/lampOS --rebase --match-head-commit " + self.train, self.calls())

    def test_overlap_still_rebuilds_without_a_merge(self):
        self.gain(TRAIN_FILE)
        r = self.mwg("--behind-unrelated")
        self.assertEqual(r.returncode, 3)
        self.assertIn("green but behind lamp-voice; not merged", r.stdout)
        self.assertNotIn("pr merge", self.calls())

    def test_without_the_flag_the_old_path_stands(self):
        self.gain("robots/lamp/hardware/test/tools/factory-run.sh")
        r = self.mwg()
        self.assertEqual(r.returncode, 3)
        self.assertEqual(r.stdout.split(" ", 1)[1], "#7 green but behind lamp-voice; not merged\n")
        self.assertNotIn("pr merge", self.calls())

    def test_a_refused_merge_as_is_rebuilds_not_fails(self):
        self.gain("robots/lamp/hardware/test/tools/factory-run.sh")
        r = self.mwg("--behind-unrelated", env=dict(self.env, FAKE_MERGE_RC="1"))
        self.assertEqual(r.returncode, 3)
        self.assertIn("merge as is refused", r.stdout)

    def test_dry_changes_nothing(self):
        self.gain("robots/lamp/hardware/test/tools/factory-run.sh")
        # Its own fetch of the PR's head is local and read-only; take it first so the refs can be compared.
        self.git("fetch", "-q", "origin", "+refs/pull/7/head:refs/remotes/pr/7")
        refs = self.git("for-each-ref")
        remote = run(["git", "for-each-ref"], self.origin).stdout
        r = self.mwg("--behind-unrelated", env=dict(self.env, DRY="1"))
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("dry: merge as is", r.stdout)
        self.assertIn("dry: #7 would merge by rebase at " + self.train[:9] + ", as it is, behind lamp-voice", r.stdout)
        self.assertNotIn("pr merge", self.calls())
        self.assertNotIn("run list", self.calls())  # no lv-quiet wait
        self.assertEqual(run(["git", "for-each-ref"], self.origin).stdout, remote)
        self.assertEqual(self.git("for-each-ref"), refs)

    def test_dry_replays_a_past_lamp_voice(self):
        # LV_AT stands in for lamp-voice's tip: here lamp-voice has since gained a crate file, but the past tip didn't.
        past = self.gain("robots/lamp/hardware/test/tools/factory-run.sh")
        self.gain("lampOS/crates/x/src/lib.rs")
        r = self.mwg("--behind-unrelated", env=dict(self.env, DRY="1", LV_AT=past))
        self.assertEqual(r.returncode, 0, r.stdout)
        self.assertIn("dry: merge as is", r.stdout)
        r = self.mwg("--behind-unrelated", env=dict(self.env, DRY="1"))
        self.assertEqual(r.returncode, 3)
        self.assertIn("dry: rebuild:", r.stdout)


if __name__ == "__main__":
    unittest.main()
