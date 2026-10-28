#!/usr/bin/env python3
"""After a stable promotion, fill the `lampos-stable` release body's
"For developers" part with the pull requests merged since the previous
stable (#3976). The notes in plain words above it are written by hand; this
never touches them, and never a build's own one-line release body, which the
promote tool reads as provenance. It is never a gate on a promotion."""

import argparse
import base64
import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import sys

REPO = "autonomous-ai/lampOS"
STABLE = "lampos-stable"
HEADING = "### For developers"
# A train's title names the pull requests it carried.
TRAIN = re.compile(r"merge train \d+: (#\d+(?:, #\d+)*)")
# The compare API lists at most this many commits; more are said, not listed.
MOST_COMMITS = 250
MOST_TITLE = 120


class Refused(Exception):
    pass


def gh(words, environ=None):
    """One `gh` call; its stdout. Only gh gets the token."""
    try:
        result = subprocess.run(["gh", *words], env=environ or os.environ, stdin=subprocess.DEVNULL,
                                capture_output=True, timeout=120, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise Refused(f"gh {words[0]} did not complete") from error
    if result.returncode:
        first = result.stderr.decode("utf-8", errors="replace").splitlines()[:1]
        raise Refused(f"gh {words[0]} failed (exit {result.returncode}): {first[0][:200] if first else 'no stderr'}")
    return result.stdout


class Notes:
    def __init__(self, run=gh, now=None):
        self.run = run
        self.now = now or (lambda: datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=7))))

    def api(self, endpoint, method="GET", fields=None):
        words = ["api", "--method", method]
        for key, value in (fields or {}).items():
            words += ["-f", f"{key}={value}"]
        words.append(f"repos/{REPO}/{endpoint}")
        try:
            return json.loads(self.run(words))
        except ValueError as error:
            raise Refused(f"unreadable GitHub answer for {endpoint}") from error

    def previous(self, asset):
        """The sequence and commit in the envelope this promotion replaced."""
        raw = self.run(["api", f"repos/{REPO}/releases/assets/{asset}", "-H", "Accept: application/octet-stream"])
        try:
            manifest = json.loads(base64.b64decode(json.loads(raw)["manifest"], validate=True))
            return int(manifest["sequence"]), str(manifest["commit"])
        except (ValueError, KeyError, TypeError) as error:
            raise Refused("the previous envelope can't be read") from error

    def merged(self, before, after):
        """The pull requests that landed between two commits, trains opened
        into what they carried, in the order they landed, and how many
        commits were left out past the compare API's page."""
        compare = self.api(f"compare/{before}...{after}")
        commits = compare.get("commits") or []
        left = max(0, int(compare.get("total_commits", len(commits))) - len(commits))
        found = {}
        for commit in commits[:MOST_COMMITS]:
            for pull in self.api(f"commits/{commit['sha']}/pulls"):
                if pull.get("base", {}).get("ref") == "lamp-voice":
                    found.setdefault(pull["number"], pull["title"])
        listed = {}
        for number, title in found.items():
            carried = TRAIN.search(title)
            if not carried:
                listed.setdefault(number, title)
                continue
            for member in re.findall(r"#(\d+)", carried.group(1)):
                if int(member) not in listed:
                    listed[int(member)] = self.api(f"pulls/{member}")["title"]
        return listed, left

    def section(self, receipt, prev_sequence, prev_commit, listed, left):
        lines = [
            HEADING,
            f"Promoted unchanged from lampos-lamp-voice-{receipt['sequence']}; source ARM64 run {receipt['source_run']}, "
            f"attempt {receipt['source_attempt']}; promotion run {receipt['promotion_run']}, attempt {receipt['promotion_attempt']}. "
            f"Commit `{receipt['commit'][:9]}`, after {prev_sequence}'s `{prev_commit[:9]}`.",
            "",
            f"Merged since {prev_sequence}:" if listed else f"No pull request found merged since {prev_sequence}.",
        ]
        lines += [f"- #{number} {title[:MOST_TITLE]}" for number, title in listed.items()]
        if left:
            lines.append(f"- and {left} more commits past GitHub's compare page")
        return "\n".join(lines) + "\n"

    def run_once(self, receipt):
        if receipt.get("outcome") != "promoted" or not receipt.get("previous_asset"):
            return "nothing to fill: no promotion over a previous stable"
        prev_sequence, prev_commit = self.previous(receipt["previous_asset"])
        listed, left = self.merged(prev_commit, receipt["commit"])
        section = self.section(receipt, prev_sequence, prev_commit, listed, left)
        release = self.api(f"releases/tags/{STABLE}")
        body = release.get("body") or ""
        header = f"## lampOS build {receipt['sequence']}"
        if body.splitlines()[:1] == [header]:
            # Written by hand for this build already: only its last part.
            kept = body.split(f"\n{HEADING}", 1)[0].rstrip("\n")
            new = f"{kept}\n\n{section}"
        else:
            # Still an older build's notes: a fresh body the hand notes go into.
            when = self.now().strftime("%d %b %Y, %H:%M")
            new = f"{header}\n\nPromoted {when} (GMT+7). The notes in plain words follow.\n\n{section}"
        value = self.api(f"releases/{release['id']}", method="PATCH", fields={"body": new})
        if value.get("body") != new:
            raise Refused("the stable notes were not confirmed")
        return f"stable notes: {len(listed)} pull requests since {prev_sequence}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", required=True, type=Path)
    args = parser.parse_args()
    try:
        receipt = json.loads(args.receipt.read_text())
        print(Notes().run_once(receipt))
    except (Refused, OSError, ValueError, KeyError) as error:
        print(f"stable notes not written, the promotion stands: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
