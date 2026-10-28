#!/usr/bin/env python3
"""Offline tests of the stable notes' "For developers" part: every gh call is simulated."""

import base64
import datetime
import importlib.util
import json
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("notes", Path(__file__).with_name("lampos-stable-notes.py"))
N = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(N)

NEW, OLD = "c" * 40, "b" * 40
RECEIPT = {"outcome": "promoted", "sequence": 8335, "commit": NEW, "source_run": 77, "source_attempt": 1,
           "promotion_run": 88, "promotion_attempt": 2, "previous_asset": 5, "stable_asset": 6, "envelope_sha256": "0" * 64}
HAND = ("## lampOS build 8335\n\nPromoted by hand.\n\n### Changed\n- **A thing a person notices.**\n\n"
        "### For developers\nAn older developers part.\n")


class FakeGh:
    def __init__(self, body, pulls=None, titles=None, total=None):
        self.body = body
        self.calls = []
        self.patched = None
        merged = "2026-10-28T19:20:19Z"
        self.pulls = pulls if pulls is not None else {
            "1" * 40: [{"number": 4163, "title": "lampOS: merge train 321: #4160, #4116", "base": {"ref": "lamp-voice"}, "merged_at": merged},
                       # Open, and closed unmerged, PRs whose branch holds the commit, as GitHub names them.
                       {"number": 3729, "title": "lampOS: still open", "base": {"ref": "lamp-voice"}, "merged_at": None},
                       {"number": 4146, "title": "lampOS: closed as carried", "base": {"ref": "lamp-voice"}, "merged_at": None}],
            "2" * 40: [{"number": 3934, "title": "lampOS: require protected release authorization", "base": {"ref": "lamp-voice"}, "merged_at": merged}],
            "3" * 40: [{"number": 3934, "title": "lampOS: require protected release authorization", "base": {"ref": "lamp-voice"}, "merged_at": merged},
                       {"number": 9, "title": "elsewhere", "base": {"ref": "main"}, "merged_at": merged}],
        }
        self.titles = titles or {"4160": "lampOS: the channel release stays under 1000 assets", "4116": "lampOS: can names skills.camera"}
        self.total = total

    def __call__(self, words):
        self.calls.append(words)
        endpoint = next(word for word in words if word.startswith("repos/")).removeprefix(f"repos/{N.REPO}/")
        if "application/octet-stream" in " ".join(words):
            manifest = json.dumps({"sequence": 8327, "commit": OLD}).encode()
            return json.dumps({"manifest": base64.b64encode(manifest).decode()}).encode()
        if endpoint.startswith("compare/"):
            assert endpoint == f"compare/{OLD}...{NEW}", endpoint
            shas = list(self.pulls)
            return json.dumps({"commits": [{"sha": sha} for sha in shas], "total_commits": self.total or len(shas)}).encode()
        if endpoint.startswith("commits/"):
            return json.dumps(self.pulls[endpoint.split("/")[1]]).encode()
        if endpoint.startswith("pulls/"):
            return json.dumps({"title": self.titles[endpoint.split("/")[1]]}).encode()
        if endpoint == f"releases/tags/{N.STABLE}":
            return json.dumps({"id": 42, "body": self.body}).encode()
        if endpoint == "releases/42" and "PATCH" in words:
            self.patched = words[words.index("-f") + 1].removeprefix("body=")
            return json.dumps({"id": 42, "body": self.patched}).encode()
        raise AssertionError(f"unexpected call: {words}")


def notes(fake):
    return N.Notes(fake, now=lambda: datetime.datetime(2026, 10, 29, 9, 5))


class StableNotesTests(unittest.TestCase):
    def test_hand_notes_for_this_build_keep_all_but_the_developers_part(self):
        fake = FakeGh(HAND)
        said = notes(fake).run_once(RECEIPT)
        kept, section = fake.patched.split(N.HEADING)
        self.assertEqual(kept, HAND.split(N.HEADING)[0])
        self.assertNotIn("An older developers part", fake.patched)
        self.assertIn("Promoted unchanged from lampos-lamp-voice-8335; source ARM64 run 77, attempt 1; promotion run 88, attempt 2.", section)
        self.assertIn(f"Commit `{NEW[:9]}`, after 8327's `{OLD[:9]}`.", section)
        # The train is opened into what it carried; a PR seen twice is listed once; another base,
        # and a PR never merged whose branch holds a commit, are left out.
        self.assertEqual([line for line in section.splitlines() if line.startswith("- ")], [
            "- #4160 lampOS: the channel release stays under 1000 assets",
            "- #4116 lampOS: can names skills.camera",
            "- #3934 lampOS: require protected release authorization",
        ])
        self.assertEqual(said, "stable notes: 3 pull requests since 8327")

    def test_an_older_builds_notes_get_a_fresh_body_for_the_hand_notes(self):
        fake = FakeGh(HAND.replace("8335", "8327"))
        notes(fake).run_once(RECEIPT)
        self.assertTrue(fake.patched.startswith("## lampOS build 8335\n\nPromoted 29 Oct 2026, 09:05 (GMT+7). The notes in plain words follow.\n\n### For developers\n"))
        self.assertNotIn("A thing a person notices", fake.patched)

    def test_hand_notes_without_a_developers_part_get_one_at_the_end(self):
        fake = FakeGh("## lampOS build 8335\n\nPromoted by hand.\n")
        notes(fake).run_once(RECEIPT)
        self.assertTrue(fake.patched.startswith("## lampOS build 8335\n\nPromoted by hand.\n\n### For developers\n"))

    def test_no_promotion_or_no_previous_stable_writes_nothing(self):
        for receipt in ({**RECEIPT, "outcome": "unchanged"}, {**RECEIPT, "previous_asset": None}):
            with self.subTest(receipt=receipt["outcome"]):
                fake = FakeGh(HAND)
                notes(fake).run_once(receipt)
                self.assertIsNone(fake.patched)
                self.assertEqual(fake.calls, [])

    def test_commits_past_the_compare_page_are_said(self):
        fake = FakeGh(HAND, total=300)
        notes(fake).run_once(RECEIPT)
        self.assertIn("- and 297 more commits past GitHub's compare page", fake.patched)

    def test_a_failed_read_writes_nothing(self):
        fake = FakeGh(HAND)
        def failing(words):
            if any(word.startswith(f"repos/{N.REPO}/compare/") for word in words):
                raise N.Refused("gh api failed (exit 1): HTTP 502")
            return fake(words)
        with self.assertRaises(N.Refused):
            N.Notes(failing).run_once(RECEIPT)
        self.assertIsNone(fake.patched)

    def test_a_patch_not_read_back_is_refused(self):
        fake = FakeGh(HAND)
        original = fake.__call__
        def lossy(words):
            answer = original(words)
            return json.dumps({"id": 42, "body": "something else"}).encode() if "PATCH" in words else answer
        with self.assertRaisesRegex(N.Refused, "not confirmed"):
            N.Notes(lossy, now=lambda: datetime.datetime(2026, 10, 29)).run_once(RECEIPT)


if __name__ == "__main__":
    unittest.main(verbosity=2)
