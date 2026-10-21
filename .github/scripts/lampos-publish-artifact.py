#!/usr/bin/env python3
"""Export current Cargo targets whose executables this build reported, never a cache glob."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

EXAMPLES = {"schedule_check": "lamp-schedule", "replay_cost": "lamp-expression"}
BINARY = re.compile(r"lamp-[a-z0-9-]{1,39}\Z")


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def key(package, target):
    return (package, tuple(target["kind"]), target["name"], target["src_path"])


def inventory(metadata):
    """All current workspace bins, plus two explicitly owned published examples.

    A feature-gated bin is required too: if the build omits it, refuse instead
    of taking an older executable that happens to have the expected name.
    """
    members = metadata["workspace_members"]
    packages = {package["id"]: package for package in metadata["packages"]}
    require(len(packages) == len(metadata["packages"]), "duplicate Cargo package")
    require(len(set(members)) == len(members) and set(members) <= packages.keys(), "incomplete Cargo workspace")
    release = Path(metadata["target_directory"]) / "release"
    examples = Path(metadata["target_directory"]) / "lampos-artifact-examples/release/examples"
    require(release.is_absolute(), "Cargo target directory must be absolute")
    expected, names, paths = {}, set(), set()
    for member in members:
        package = packages[member]
        for target in package["targets"]:
            name = target["name"]
            if target["kind"] == ["bin"]:
                require(BINARY.fullmatch(name), "workspace binary is not a supported lamp-* name")
                path = release / name
            elif target["kind"] == ["example"] and EXAMPLES.get(name) == package["name"]:
                path = examples / name
            else:
                continue
            identity = key(member, target)
            require(identity not in expected and name not in names and path not in paths, "duplicate or conflicting export target")
            expected[identity] = (name, path)
            names.add(name)
            paths.add(path)
    require({"lamp-update", *EXAMPLES} <= names, "missing required updater or example target")
    return expected


def reported_executables(logs, expected):
    found = {}
    for log in logs:
        finished = []
        last = None
        with log.open() as source:
            for line in source:
                message = json.loads(line)
                last = message.get("reason")
                if last == "build-finished":
                    finished.append(message.get("success"))
                if last != "compiler-artifact":
                    continue
                identity = key(message["package_id"], message["target"])
                if identity not in expected:
                    continue
                name, path = expected[identity]
                # Workspace --examples also builds these names into its shared
                # directory. Only the separately built, selected package counts.
                if identity[1] == ("example",) and Path(message.get("executable") or "") != path:
                    shared = path.parents[3] / "release/examples" / name
                    require(Path(message.get("executable") or "") == shared, "compiler artifact has a conflicting example path")
                    continue
                require(identity not in found, "duplicate compiler artifact for export target")
                require(message["profile"]["test"] is False, "test executable is not a release artifact")
                require(type(message.get("fresh")) is bool, "unknown Cargo cache status")
                require(Path(message.get("executable") or "") == path, "compiler artifact has a conflicting executable path")
                require(str(path) in message["filenames"], "compiler artifact does not list its executable")
                # fresh:true is Cargo's validated hit for this current target;
                # an unrelated cached file has no matching artifact record.
                found[identity] = path
        require(finished == [True] and last == "build-finished", "build log is incomplete or unsuccessful")
    missing = set(expected) - found.keys()
    require(not missing, "current targets absent from successful build: " + ", ".join(sorted(expected[item][0] for item in missing)))
    return {expected[identity][0]: path for identity, path in found.items()}


def export(metadata, logs, output):
    expected = inventory(metadata)
    files = reported_executables(logs, expected)
    for path in files.values():
        require(path.is_file() and not path.is_symlink() and path.stat().st_size > 0 and os.access(path, os.X_OK), "current target executable is missing, empty, linked or not executable")
    # Never reuse an artifact folder: stale output must not acquire new sums.
    output.mkdir()
    sums = []
    for name, source in sorted(files.items()):
        before = digest(source)
        destination = output / name
        shutil.copy2(source, destination)
        require(digest(destination) == before, "copied executable checksum mismatch")
        sums.append(f"{before}  {name}\n")
    (output / "SHA256SUMS").write_text("".join(sums))
    require({path.name for path in output.iterdir()} == set(files) | {"SHA256SUMS"}, "artifact inventory changed during export")
    return len(files) - len(EXAMPLES), len(EXAMPLES)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", required=True, type=Path)
    parser.add_argument("--build-log", required=True, action="append", type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    try:
        result = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked", "--offline"],
            cwd=args.workspace, check=True, capture_output=True, text=True, timeout=60,
        )
        binaries, examples = export(json.loads(result.stdout), args.build_log, args.out)
        print(f"Packaged {binaries} current workspace binaries and {examples} selected examples with complete SHA256SUMS")
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"lampOS artifact refused: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
