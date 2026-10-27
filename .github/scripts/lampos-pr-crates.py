#!/usr/bin/env python3
"""Which lampOS crates a pull request's ARM64 run tests (the project lead's
proposal, 19 Oct 2026, #1711).

A pull request tests the crates its diff changes, every workspace crate that
depends on them (normal, dev or build dependencies, followed to the end), and
three crates whose tests read other crates' files rather than their code:
lamp-cloud (deploy/product's units), lamp-voice-eval (crates/msgs' topics) and
lamp-update (deploy/product and brain-build.md). Anything shared, or with
readers this can't see, runs the whole workspace: Cargo.lock, any Cargo.toml,
crates/msgs, crates/bus, crates/ipc, deploy/, skills/, the docs tests read,
.config/, this script, the workflow, and every path outside crates/ and the
docs' and role files' Markdown. The push to lamp-voice always runs the whole
workspace.

Run from the repository root on the pull request's merge commit (its first
parent is lamp-voice). Prints the cargo arguments (`--workspace`, or `-p ...`)
and writes them to $GITHUB_OUTPUT as `args`, with why to $GITHUB_STEP_SUMMARY.
The same crates go to $GITHUB_OUTPUT as `filter`, a nextest filter
(`package(=a) | package(=b)`, empty for the whole workspace), which the run
uses: it builds the whole workspace, so dependencies keep one set of features
and come from the cache (lampos-arm64.yml). `--diff A B` takes the diff between
two commits instead, for a local check.
"""
import json
import os
import subprocess
import sys

ALWAYS = ["lamp-cloud", "lamp-voice-eval", "lamp-update"]
SHARED_CRATES = {"msgs", "bus", "ipc"}
# The docs code or tests read (also named in the workflow's paths).
READ_DOCS = {
    "lampOS/docs/brain-build.md",
    "lampOS/docs/control.md",
    "lampOS/docs/endpoint-detection.md",
    "lampOS/docs/endpoint-pilot.md",
    "lampOS/docs/install.md",
    "lampOS/docs/memory.md",
    "lampOS/docs/switches.md",
}


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def changed_files(argv):
    if len(argv) == 3 and argv[0] == "--diff":
        return git("diff", "--name-only", argv[1], argv[2]).split()
    parents = git("rev-list", "--parents", "-n", "1", "HEAD").split()[1:]
    if len(parents) != 2:
        return None  # not a pull request's merge commit
    return git("diff", "--name-only", parents[0], "HEAD").split()


def workspace():
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
        cwd="lampOS", check=True, capture_output=True, text=True).stdout)
    root = os.path.realpath("lampOS")
    by_dir, deps = {}, {}
    names = {p["name"] for p in meta["packages"]}
    for p in meta["packages"]:
        d = os.path.relpath(os.path.dirname(p["manifest_path"]), root)
        by_dir[d] = p["name"]
        deps[p["name"]] = {x["name"] for x in p["dependencies"] if x["name"] in names}
    return by_dir, deps


def whole_reason(path):
    """Why a path runs the whole workspace, or None."""
    if not path.startswith("lampOS/"):
        return "outside lampOS/"
    if path == "lampOS/Cargo.lock" or path.endswith("/Cargo.toml"):
        return "Cargo.lock or a Cargo.toml"
    parts = path.split("/")
    if parts[1] == "crates" and len(parts) > 3:
        return f"crates/{parts[2]}, shared message types" if parts[2] in SHARED_CRATES else None
    if path in READ_DOCS:
        return "a doc tests read"
    if parts[1] in ("docs", "builders") and path.endswith(".md"):
        return None
    return f"{'/'.join(parts[:2])}, read by tests this can't see"


def main(argv):
    files = changed_files(argv)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    def done(args, why, tested=()):
        print(args)
        print(why, file=sys.stderr)
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a") as f:
                f.write(f"args={args}\n")
                f.write(f"filter={' | '.join(f'package(={n})' for n in sorted(tested))}\n")
        if summary:
            with open(summary, "a") as f:
                f.write(f"**Tested:** `{args}`\n\n{why}\n")
    if files is None:
        return done("--workspace", "The whole workspace: HEAD is not a pull request's merge commit.")
    for path in files:
        reason = whole_reason(path)
        if reason:
            return done("--workspace", f"The whole workspace: `{path}` ({reason}).")
    by_dir, deps = workspace()
    changed = set()
    for path in files:
        parts = path.split("/")
        if parts[1] == "crates" and len(parts) > 3:
            name = by_dir.get(f"crates/{parts[2]}")
            if name is None:
                return done("--workspace", f"The whole workspace: `{path}` is in no workspace crate.")
            changed.add(name)
    users = {}
    for name, ds in deps.items():
        for d in ds:
            users.setdefault(d, set()).add(name)
    tested, todo = set(changed), list(changed)
    while todo:
        for u in users.get(todo.pop(), ()):
            if u not in tested:
                tested.add(u)
                todo.append(u)
    tested |= set(ALWAYS)
    args = " ".join(f"-p {n}" for n in sorted(tested))
    why = (f"Changed: {', '.join(sorted(changed)) or 'no crate (docs only)'}. "
           f"With the crates that depend on them and the three that read other crates' files "
           f"({', '.join(ALWAYS)}): {len(tested)} of {len(deps)} crates.")
    return done(args, why, tested)


if __name__ == "__main__":
    main(sys.argv[1:])
