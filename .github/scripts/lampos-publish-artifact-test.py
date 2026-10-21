#!/usr/bin/env python3
"""Offline artifact provenance checks, including a real Cargo cache regression."""

import argparse
import copy
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("artifact", Path(__file__).with_name("lampos-publish-artifact.py"))
A = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(A)
CARGO = None


def package(name, targets):
    return {"id": name, "name": name, "targets": [
        {"name": target, "kind": [kind], "src_path": f"/workspace/{name}/{target}.rs"}
        for kind, target in targets
    ]}


def message(package, target, path, fresh=False):
    return {"reason": "compiler-artifact", "package_id": package["id"], "target": target,
            "profile": {"test": False}, "fresh": fresh, "executable": str(path), "filenames": [str(path)]}


def log(path, messages, success=True):
    path.write_text("".join(json.dumps(value) + "\n" for value in [*messages, {"reason": "build-finished", "success": success}]))
    return path


class InventoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="lampos-artifact-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        packages = [package("lamp-update", [("bin", "lamp-update"), ("bin", "lamp-current")]),
                    package("lamp-schedule", [("example", "schedule_check")]),
                    package("lamp-expression", [("example", "replay_cost")]),
                    package("lamp-controls", [("example", "replay_cost")])]
        self.meta = {"workspace_members": [p["id"] for p in packages], "packages": packages,
                     "target_directory": str(self.root / "target")}
        self.expected = A.inventory(self.meta)
        self.messages = []
        for p in packages:
            for t in p["targets"]:
                item = self.expected.get(A.key(p["id"], t))
                if item:
                    name, path = item
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes((p["name"] + ":" + name).encode())
                    path.chmod(0o755)
                    self.messages.append(message(p, t, path))
        self.build = self.root / "build.jsonl"
        self.out = self.root / "out"

    def export(self, messages=None, success=True):
        log(self.build, self.messages if messages is None else messages, success)
        return A.export(self.meta, [self.build], self.out)

    def check_sums(self, output):
        lines = (output / "SHA256SUMS").read_text().splitlines()
        sums = {name: value for value, name in (line.split("  ") for line in lines)}
        self.assertEqual(len(sums), len(lines))
        self.assertEqual(set(sums), {p.name for p in output.iterdir()} - {"SHA256SUMS"})
        self.assertEqual(sums, {p.name: A.digest(p) for p in output.iterdir() if p.name != "SHA256SUMS"})

    def test_unrelated_cached_binary_is_excluded_and_all_sums_match(self):
        retired = self.root / "target/release/lamp-retired"
        retired.write_text("retired cache payload")
        retired.chmod(0o755)
        self.assertEqual(self.export(), (2, 2))
        self.assertNotIn(retired.name, {p.name for p in self.out.iterdir()})
        self.check_sums(self.out)

    def test_cargo_fresh_current_target_is_an_accepted_cache_hit(self):
        for value in self.messages:
            value["fresh"] = True
        self.export()
        self.check_sums(self.out)

    def test_cached_current_filename_without_build_record_refuses(self):
        with self.assertRaisesRegex(ValueError, "absent from successful build"):
            self.export(self.messages[1:])
        self.assertFalse(self.out.exists())

    def test_missing_required_example_record_refuses(self):
        with self.assertRaisesRegex(ValueError, "absent from successful build"):
            self.export(self.messages[:-1])
        self.assertFalse(self.out.exists())

    def test_missing_or_non_executable_or_linked_current_file_refuses(self):
        path = Path(self.messages[0]["executable"])
        data = path.read_bytes()
        for failure in ("missing", "mode", "symlink"):
            with self.subTest(failure=failure):
                path.unlink()
                if failure == "mode":
                    path.write_bytes(data)
                    path.chmod(0o600)
                elif failure == "symlink":
                    path.symlink_to(Path(self.messages[1]["executable"]))
                with self.assertRaisesRegex(ValueError, "current target executable"):
                    self.export()
                if path.is_symlink() or path.exists():
                    path.unlink()
                path.write_bytes(data)
                path.chmod(0o755)

    def test_duplicate_targets_and_conflicting_paths_refuse(self):
        self.meta["packages"][0]["targets"].append(copy.deepcopy(self.meta["packages"][0]["targets"][0]))
        with self.assertRaisesRegex(ValueError, "duplicate or conflicting"):
            self.export()

    def test_duplicate_or_conflicting_build_record_refuses(self):
        for failure in ("duplicate", "path"):
            with self.subTest(failure=failure):
                values = copy.deepcopy(self.messages)
                if failure == "duplicate":
                    values.append(values[0])
                else:
                    values[0]["executable"] = str(self.root / "target/release/lamp-retired")
                with self.assertRaisesRegex(ValueError, "duplicate|conflicting"):
                    self.export(values)

    def test_incomplete_or_unsuccessful_build_refuses(self):
        with self.assertRaisesRegex(ValueError, "incomplete or unsuccessful"):
            self.export(success=False)
        self.build.write_text(json.dumps(self.messages[0]) + "\n")
        with self.assertRaisesRegex(ValueError, "incomplete or unsuccessful"):
            A.export(self.meta, [self.build], self.out)

    def test_shared_example_output_does_not_replace_selected_package(self):
        shared = self.root / "target/release/examples/replay_cost"
        shared.parent.mkdir(parents=True)
        shared.write_text("controls example")
        shared.chmod(0o755)
        extra = []
        for p in self.meta["packages"][-2:]:
            extra.append(message(p, p["targets"][0], shared))
        self.export(extra + self.messages)
        self.assertEqual((self.out / "replay_cost").read_text(), "lamp-expression:replay_cost")
        self.check_sums(self.out)

    def test_shared_example_alone_is_insufficient(self):
        values = copy.deepcopy(self.messages)
        values[-1]["executable"] = str(self.root / "target/release/examples/replay_cost")
        with self.assertRaisesRegex(ValueError, "absent from successful build"):
            self.export(values)

    def test_old_artifact_directory_is_never_reused(self):
        self.out.mkdir()
        stale = self.out / "lamp-retired"
        stale.write_text("keep for diagnosis")
        with self.assertRaises(FileExistsError):
            self.export()
        self.assertEqual(stale.read_text(), "keep for diagnosis")


class CargoRegression(unittest.TestCase):
    def test_renamed_cached_binary_and_shared_example_cannot_enter_current_artifact(self):
        with tempfile.TemporaryDirectory(prefix="lampos-artifact-cargo-") as folder:
            root = Path(folder)
            env = dict(os.environ)
            env.pop("CARGO_TARGET_DIR", None)
            (root / "Cargo.toml").write_text('[workspace]\nmembers=["updater","schedule","expression","controls"]\nresolver="2"\n')
            for directory, name in (("updater", "lamp-update"), ("schedule", "lamp-schedule"),
                                    ("expression", "lamp-expression"), ("controls", "lamp-controls")):
                crate = root / directory
                (crate / "src").mkdir(parents=True)
                (crate / "Cargo.toml").write_text(f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2021"\n')
                (crate / "src/lib.rs").write_text('pub fn fixture() {}\n')
                if directory == "updater":
                    (crate / "src/main.rs").write_text('fn main() {}\n')
                    (crate / "src/bin").mkdir()
                    (crate / "src/bin/lamp-retired.rs").write_text('fn main() { println!("retired"); }\n')
                else:
                    (crate / "examples").mkdir()
                    target = "schedule_check" if directory == "schedule" else "replay_cost"
                    (crate / f"examples/{target}.rs").write_text(f'fn main() {{ println!("{directory}"); }}\n')

            def cargo(*args):
                return subprocess.run([CARGO, *args], cwd=root, env=env, check=True,
                                      capture_output=True, text=True, timeout=120).stdout

            def build(name, *args):
                path = root / name
                path.write_text(cargo("build", "--offline", "--release", "--message-format=json-render-diagnostics", *args))
                return path

            build("old.jsonl", "--workspace", "--bins", "--examples")
            old = root / "target/release/lamp-retired"
            self.assertTrue(old.is_file())
            (root / "updater/src/bin/lamp-retired.rs").rename(root / "updater/src/bin/lamp-current.rs")
            build("current.jsonl", "--workspace", "--bins", "--examples")
            # Run again to prove current-target fresh:true records are accepted.
            workspace_log = build("fresh.jsonl", "--workspace", "--bins", "--examples")
            self.assertTrue(any(value.get("fresh") is True and value.get("target", {}).get("name") == "lamp-current"
                                for value in map(json.loads, workspace_log.read_text().splitlines())))
            selected = [build("schedule.jsonl", "-p", "lamp-schedule", "--example", "schedule_check", "--target-dir", "target/lampos-artifact-examples"),
                        build("expression.jsonl", "-p", "lamp-expression", "--example", "replay_cost", "--target-dir", "target/lampos-artifact-examples")]
            output = root / "out"
            # Exercise the actual exporter CLI, including its metadata command.
            env["PATH"] = str(Path(CARGO).parent) + os.pathsep + env.get("PATH", "")
            args = [sys.executable, "-B", "-I", str(Path(__file__).with_name("lampos-publish-artifact.py")),
                    "--workspace", str(root), "--out", str(output)]
            for path in [workspace_log, *selected]:
                args += ["--build-log", str(path)]
            result = subprocess.run(args, env=env, check=True, capture_output=True, text=True, timeout=120)
            self.assertEqual(result.stdout.strip(), "Packaged 2 current workspace binaries and 2 selected examples with complete SHA256SUMS")
            self.assertTrue(old.is_file(), "the regression must retain the retired cached executable")
            self.assertEqual({p.name for p in output.iterdir()}, {"lamp-update", "lamp-current", "schedule_check", "replay_cost", "SHA256SUMS"})
            self.assertEqual((output / "replay_cost").read_bytes(), (root / "target/lampos-artifact-examples/release/examples/replay_cost").read_bytes())
            sums = dict(line.split("  ")[::-1] for line in (output / "SHA256SUMS").read_text().splitlines())
            self.assertEqual(sums, {p.name: A.digest(p) for p in output.iterdir() if p.name != "SHA256SUMS"})


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo", default=shutil.which("cargo"))
    args = parser.parse_args()
    if not args.cargo:
        parser.error("cargo is required for the real cache regression")
    # Preserve rustup's cargo shim name; resolving its symlink invokes rustup.
    CARGO = str(Path(args.cargo).absolute())
    unittest.main(argv=[__file__], verbosity=2)
