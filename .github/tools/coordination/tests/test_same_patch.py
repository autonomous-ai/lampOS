# Tests for lib.sh's coord_same_patch with the Queue (7 Nov, El's HOLD on #5653): a Queue READ ASK naming TWO READS on
# an earlier head, then a plain rebase and one READ-OK on the new head, still asks TWO READS. A temporary git repository
# (a bare origin and a work clone on lamp-voice). Needs git and zsh.
# Run from the repo root: python3 -I -m pytest -q .github/tools/coordination/tests
import json, os, shutil, subprocess, sys, tempfile, unittest

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LIB = os.path.join(HERE, "lib.sh")
READS2 = os.path.join(HERE, "reads2.py")


def run(args, cwd, env=None, check=True, input=None):
    return subprocess.run(args, cwd=cwd, env=env, check=check, capture_output=True, text=True, input=input)


class SamePatch(unittest.TestCase):
    """Head A, lamp-voice moves, A rebased as B (same patch); C another patch on lamp-voice."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.origin = os.path.join(self.tmp, "origin.git")
        self.work = os.path.join(self.tmp, "work")
        self.state = os.path.join(self.tmp, "state")
        os.makedirs(self.state)
        self.env = dict(os.environ, REPO_DIR=self.work, TOOLS_DIR=HERE, LAMP_COORD_STATE=self.state)
        run(["git", "init", "-q", "--bare", self.origin], self.tmp)
        run(["git", "init", "-q", "-b", "lamp-voice", self.work], self.tmp)
        self.git("config", "user.email", "t@example.invalid")
        self.git("config", "user.name", "t")
        self.git("remote", "add", "origin", self.origin)
        self.write("a.txt", "one\ntwo\nthree\n"); self.write("b.txt", "x\n")
        self.commit("base")
        self.git("push", "-q", "origin", "lamp-voice")
        self.git("checkout", "-q", "-b", "pr")
        self.write("a.txt", "one\nTWO\nthree\n")
        self.A = self.commit("the change")
        self.git("checkout", "-q", "lamp-voice")
        self.write("b.txt", "y\n")
        self.commit("lamp-voice moves")
        self.git("push", "-q", "origin", "lamp-voice")
        self.git("checkout", "-q", "-b", "other")
        self.write("b.txt", "z\n")
        self.C = self.commit("another change")
        self.git("checkout", "-q", "-b", "pr2", "lamp-voice")
        self.git("cherry-pick", self.A)
        self.B = self.rev("HEAD")
        self.git("fetch", "-q", "origin")
        self.writers = os.path.join(self.tmp, "writers.txt")
        with open(self.writers, "w") as f:
            f.write("writer-a\nwriter-b\n")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def git(self, *a):
        return run(["git", *a], self.work).stdout

    def rev(self, ref):
        return self.git("rev-parse", ref).strip()

    def write(self, f, body):
        with open(os.path.join(self.work, f), "w") as fh:
            fh.write(body)

    def commit(self, msg):
        self.git("add", "-A"); self.git("commit", "-q", "-m", msg)
        return self.rev("HEAD")

    def jsonl(self, name, items):
        p = os.path.join(self.tmp, name)
        with open(p, "w") as f:
            f.write("".join(json.dumps(x) + "\n" for x in items))
        return p

    def same_patch(self, *args):
        r = run(["zsh", "-c", 'source "$TOOLS_DIR/lib.sh"; coord_same_patch "$@"', "zsh", *args], self.work, self.env)
        return r.stdout.split()

    def ask(self, sha):
        return {"login": "writer-a", "body": "READ ASK " + sha[:10] + "\nTWO READS."}

    def test_a_queue_ask_on_an_earlier_same_patch_head_is_offered(self):
        c = self.jsonl("c.jsonl", [])
        q = self.jsonl("q.jsonl", [self.ask(self.A)])
        self.assertEqual(self.same_patch(self.B, c, q), [self.B, self.A])

    def test_a_queue_ask_on_another_patch_is_not(self):
        c = self.jsonl("c.jsonl", [])
        q = self.jsonl("q.jsonl", [self.ask(self.C)])
        self.assertEqual(self.same_patch(self.B, c, q), [self.B])

    def test_an_unknown_queue_sha_is_skipped(self):
        c = self.jsonl("c.jsonl", [])
        q = self.jsonl("q.jsonl", [self.ask("deadbeef0123456789"), self.ask(self.A)])
        self.assertEqual(self.same_patch(self.B, c, q), [self.B, self.A])

    def test_a_signed_queue_ask_on_an_earlier_head_is_offered(self):
        # 7 Nov (#5653's P3s): the Queue's shas come from reads2.py (--queue-shas), so a signed ask with words between
        # READ ASK and its sha survives a plain rebase, as reads2.py's queue_asks reads it.
        c = self.jsonl("c.jsonl", [])
        for first in ("Sixth, READ ASK to **El**: `" + self.A[:10] + "`", "Twelve — READ ASK " + self.A):
            q = self.jsonl("q.jsonl", [{"login": "writer-a", "body": first + "\nTWO READS."}])
            self.assertEqual(self.same_patch(self.B, c, q), [self.B, self.A], first)

    def test_a_queue_sha_off_an_asking_first_line_is_not_offered(self):
        # A sha in prose that mentions a READ ASK, or on line two, is no candidate (reads2.py wouldn't count it).
        c = self.jsonl("c.jsonl", [])
        for body in ("the hour #5653 had a READ ASK " + self.A[:10] + " naming TWO READS",
                     "READ ASK #5653\nREAD ASK " + self.A[:10] + ", TWO READS."):
            q = self.jsonl("q.jsonl", [{"login": "writer-a", "body": body}])
            self.assertEqual(self.same_patch(self.B, c, q), [self.B], body)

    def test_a_sha_in_both_files_is_printed_once(self):
        c = self.jsonl("c.jsonl", [{"login": "writer-b", "body": "READ-OK " + self.A}])
        q = self.jsonl("q.jsonl", [self.ask(self.A)])
        self.assertEqual(self.same_patch(self.B, c, q), [self.B, self.A])

    def test_without_the_queue_nothing_changes(self):
        c = self.jsonl("c.jsonl", [])
        self.assertEqual(self.same_patch(self.B, c), [self.B])
        c = self.jsonl("c.jsonl", [{"login": "writer-b", "body": "READ-OK " + self.A[:10]}])
        self.assertEqual(self.same_patch(self.B, c), [self.B, self.A])

    def test_the_patch_cache_holds_across_calls(self):
        # Output redirected (not $(...)), as the bot calls it, so the cache lives in the calling shell.
        c = self.jsonl("c.jsonl", [])
        q = self.jsonl("q.jsonl", [self.ask(self.A), self.ask("deadbeef0123456789")])
        r = run(["zsh", "-c", 'source "$TOOLS_DIR/lib.sh"; coord_same_patch "$@" > /dev/null; print -r -- ${#coord_patch_cache}',
                 "zsh", self.B, c, q], self.work, self.env)
        self.assertGreaterEqual(int(r.stdout.strip()), 3)   # B, A and the unknown sha

    def test_end_to_end_one_read_on_the_rebased_head_is_short(self):
        body = {"login": "", "body": "A change.", "pr_body": True}
        one = {"login": "writer-b", "body": "READ-OK " + self.B + "\n\nthe security read"}
        c = self.jsonl("c.jsonl", [one])
        q = self.jsonl("q.jsonl", [self.ask(self.A)])
        heads = self.same_patch(self.B, c, q)
        lines = "".join(json.dumps(x) + "\n" for x in (body, one))
        r = run([sys.executable, "-I", READS2, "--writers", self.writers, "--queue", q, "--pr", "5653", *heads],
                self.work, input=lines)
        self.assertEqual(r.stdout.strip(), "short 1")


    def test_end_to_end_an_ask_before_a_fix_on_top_still_asks(self):
        # 7 Nov (the second reviewer's HOLD on #5653): asked at B, then a fix D on top (another patch); the bot's
        # rev-list of the PR's commits reaches reads2.py's --pr-commits, while heads stay [D].
        self.write("a.txt", "one\nTWO\nTHREE\n")
        d = self.commit("a fix")
        body = {"login": "", "body": "A change.", "pr_body": True}
        one = {"login": "writer-b", "body": "READ-OK " + d + "\n\nthe security read"}
        c = self.jsonl("c.jsonl", [one])
        q = self.jsonl("q.jsonl", [self.ask(self.B)])
        heads = self.same_patch(d, c, q)
        self.assertEqual(heads, [d])
        commits = os.path.join(self.tmp, "commits.txt")
        with open(commits, "w") as f:
            f.write(self.git("rev-list", "origin/lamp-voice.." + d))
        lines = "".join(json.dumps(x) + "\n" for x in (body, one))
        def reads(*extra):
            return run([sys.executable, "-I", READS2, "--writers", self.writers, "--queue", q, "--pr", "5653", *extra, *heads],
                       self.work, input=lines).stdout.strip()
        self.assertEqual(reads("--pr-commits", commits), "short 1")
        self.assertEqual(reads(), "none")

if __name__ == "__main__":
    unittest.main()
