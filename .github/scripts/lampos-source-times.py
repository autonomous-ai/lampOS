#!/usr/bin/env python3
"""Give lampOS's unchanged source files back the times they had when the
cached lampOS/target was built, so cargo rebuilds only what changed (#1806;
the first builder's ask of 21 Oct 2026 to cut the ARM64 run to about ten
minutes).

Cargo rebuilds a workspace crate when one of its files is newer than what was
built from it. A checkout gives every file the time of the checkout, so every
run rebuilt all of lampOS's crates (4.4 to 7.5 minutes before any test) and the
cache saved only the dependencies' time.

The cache now carries lampOS/target/source-times.json: each tracked file's git
blob and the time it had for that run's build. Here, a file whose blob is the
same gets that time back. A changed or new file keeps the checkout's time,
which is newer than anything in the cache, so its crate and every crate built
from it are rebuilt. A file's content decides, never its time alone.

Run from the repository root after the cache is restored and before the first
cargo command. It then writes this checkout's record, which the run's cache
saves for the next run.
"""
import json
import os
import subprocess

RECORD = "lampOS/target/source-times.json"


def tracked():
    """Each tracked file under lampOS/ with the blob of what is on disk now,
    symlinks and submodules left out."""
    out = subprocess.run(["git", "ls-files", "-s", "-z", "--", "lampOS"],
                         check=True, capture_output=True).stdout
    paths = []
    for entry in out.split(b"\0"):
        if not entry:
            continue
        meta, path = entry.split(b"\t", 1)
        if meta.split(b" ")[0] in (b"100644", b"100755"):
            paths.append(path.decode())
    blobs = subprocess.run(["git", "hash-object", "--no-filters", "--stdin-paths"],
                           input="\n".join(paths) + "\n", check=True,
                           capture_output=True, text=True).stdout.split()
    return dict(zip(paths, blobs, strict=True))


def main():
    files = tracked()
    try:
        with open(RECORD) as f:
            kept = json.load(f)
    except (OSError, ValueError):
        kept = {}
    same = 0
    for path, blob in files.items():
        old = kept.get(path)
        if old and old[0] == blob:
            os.utime(path, ns=(old[1], old[1]))
            same += 1
    record = {path: [blob, os.stat(path).st_mtime_ns] for path, blob in files.items()}
    os.makedirs(os.path.dirname(RECORD), exist_ok=True)
    with open(RECORD + ".new", "w") as f:
        json.dump(record, f)
    os.replace(RECORD + ".new", RECORD)
    if not kept:
        print(f"No record in the cache: all {len(files)} source files keep the checkout's time.")
    else:
        print(f"{same} of {len(files)} source files are as the cached build had them and get its times back; "
              f"{len(files) - same} changed or new keep the checkout's time.")


if __name__ == "__main__":
    main()
