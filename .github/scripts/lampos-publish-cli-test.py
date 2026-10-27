#!/usr/bin/env python3
"""Rehearse the real updater CLI using local files, a test key and simulated gh."""

import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("offline_tests", Path(__file__).with_name("lampos-publish-test.py"))
T = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(T)
P = T.P
SOURCE = Path(__file__).resolve().parents[2]
UPDATER = None


class LocalCommands(T.FakeCommands):
    def __init__(self):
        super().__init__(None)
        env = dict(os.environ)
        for switch in ("LAMP_UPDATE_SELF", "LAMP_UPDATE_MODELS", "LAMP_UPDATE_APPLY", "LAMP_UPDATE_IMAGE"):
            env[switch] = "off"
        self.local = P.Commands(env)
        self.updater_results = []

    def updater(self, words):
        self.updater_calls.append(words)
        if words[1] == "check":
            self.assert_check(dict(zip(words[2::2], words[3::2])))
        run = P.subprocess.run

        def observed_run(*args, **kwargs):
            result = run(*args, **kwargs)
            self.updater_results.append((words[1], result.returncode, result.stdout.decode("utf-8", errors="replace")))
            return result

        with mock.patch.object(P.subprocess, "run", side_effect=observed_run):
            return self.local.run(words)


class CliTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = tempfile.TemporaryDirectory(prefix="lampos-publish-cli-key-")
        cls.addClassCleanup(cls.fixture.cleanup)
        cls.keys = Path(cls.fixture.name)
        cls.local = P.Commands(os.environ)
        # Only this disposable test key is generated. No repository/real key,
        # service, device, network or GitHub command is used by this test.
        der = cls.local.run(["openssl", "genpkey", "-algorithm", "ed25519", "-outform", "DER"])
        cls.private = cls.keys / "test.key"
        cls.private.write_bytes(base64.b64encode(der))
        cls.private.chmod(0o600)
        cls.der = cls.keys / "test.der"
        cls.der.write_bytes(der)
        cls.der.chmod(0o600)
        cls.public = cls.local.run([UPDATER, "public-key", "--key-file", cls.private]).strip()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="lampos-publish-cli-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.artifact = self.root / "artifact"
        self.artifact.mkdir()
        shutil.copyfile(UPDATER, self.artifact / "lamp-update")
        (self.artifact / "lamp-update").chmod(0o600)  # Downloaded artifact loses executable mode.
        for name in sorted((T.RETAINED | T.EXCLUDED) - {"lamp-update"}):
            (self.artifact / name).write_bytes(b"test-only payload, never executed\n")
        (self.artifact / "SHA256SUMS").write_text("".join(f"{P.digest(path)}  {path.name}\n" for path in sorted(self.artifact.iterdir())))
        self.env = {P.ENABLE: "on", P.BOOTSTRAP: "on", P.SECRET: self.private.read_text(), "GH_TOKEN": "simulated-release-authorization", "GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": P.REPO, "GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/heads/lamp-voice", "GITHUB_SHA": T.SHA, "GITHUB_RUN_NUMBER": "10", "GITHUB_RUN_ID": "1234", "GITHUB_RUN_ATTEMPT": "1"}
        self.fake = LocalCommands()

    def publish(self):
        return P.Publisher(self.artifact, SOURCE, self.env, self.fake).run()

    def signed_head(self, changes=None, unchecked=False):
        manifest = json.loads(base64.b64decode(json.loads(T.old_envelope(9))["manifest"]))
        manifest.update(changes or {})
        path = self.root / "manifest.json"
        path.write_text(json.dumps(manifest))
        envelope = self.root / P.ENVELOPE
        if unchecked:
            # The maintained sign CLI deliberately rejects unknown schemas.
            # OpenSSL makes a validly signed newer-schema test fixture only;
            # the publisher never uses this path or its signing implementation.
            signature = self.local.run(["openssl", "pkeyutl", "-sign", "-rawin", "-inkey", self.der, "-keyform", "DER", "-in", path])
            envelope.write_text(json.dumps({"format": "lampos-update/1", "key": hashlib.sha256(base64.b64decode(self.public)).hexdigest()[:16], "manifest": base64.b64encode(path.read_bytes()).decode(), "signature": base64.b64encode(signature).decode()}))
        else:
            self.local.run([UPDATER, "sign", "--key-file", self.private, "--manifest", path, "--out", envelope])
        return envelope.read_bytes()

    def install_head(self, raw):
        release = self.fake.release(P.HEAD, "b" * 40)
        self.fake.add(release, P.ENVELOPE, raw)
        self.fake.before = raw
        self.env[P.BOOTSTRAP] = "off"

    def refused_head(self, raw, reason):
        original = P.tree_info(self.artifact)
        self.install_head(raw)
        with self.assertRaisesRegex(P.Refused, r"lamp-update failed \(exit 1\)"):
            self.publish()
        checks = [(code, stdout) for command, code, stdout in self.fake.updater_results if command == "check"]
        # The new candidate verifies first; the existing head must then reach
        # its own verifier and refuse for this fixture's intended reason.
        self.assertEqual([code for code, _ in checks], [0, 1])
        self.assertIn(reason, checks[-1][1])
        self.assertEqual(self.fake.canonical(), raw)
        self.assertEqual(self.fake.head_mutations, [])
        self.assertNotIn(f"{P.HEAD}-10", self.fake.release_map)
        self.assertEqual(P.tree_info(self.artifact), original)

    def test_real_manifest_sign_check_and_classified_upload_rehearsal(self):
        original = P.tree_info(self.artifact)
        receipt = self.publish()
        self.assertEqual(receipt["sequence"], 10)
        release = self.fake.release_map[receipt["published"]]
        T.assert_release_subset(self, self.artifact, release["assets"])
        self.assertEqual(P.tree_info(self.artifact), original)
        manifest = json.loads(release["assets"]["manifest.json"]["data"])
        self.assertIn("lamp-product-skills", {file["name"] for file in manifest["files"]})
        self.assertTrue(manifest["units"])
        self.assertTrue(any(call[1] == "check" for call in self.fake.updater_calls))

    def test_real_excluded_artifact_tampering_refuses_before_execution(self):
        for name in sorted(T.EXCLUDED):
            with self.subTest(name=name):
                original = (self.artifact / name).read_bytes()
                try:
                    (self.artifact / name).write_bytes(b"tampered excluded payload")
                    before = P.tree_info(self.artifact)
                    with self.assertRaisesRegex(P.Refused, "checksum mismatch"):
                        self.publish()
                    self.assertEqual(self.fake.updater_calls, [])
                    self.assertEqual(P.tree_info(self.artifact), before)
                    self.assertFalse(self.fake.release_map)
                finally:
                    (self.artifact / name).write_bytes(original)

    def test_real_verified_existing_channel_promotes_and_retains_previous(self):
        previous = self.signed_head()
        self.install_head(previous)
        self.publish()
        self.assertNotEqual(self.fake.canonical(), previous)
        self.assertIn(previous, [asset["data"] for asset in self.fake.release_map[P.HEAD]["assets"].values()])

    def test_real_bad_signature_refuses_before_immutable_upload(self):
        envelope = json.loads(self.signed_head())
        signature = bytearray(base64.b64decode(envelope["signature"]))
        signature[0] ^= 1
        envelope["signature"] = base64.b64encode(signature).decode()
        self.refused_head(json.dumps(envelope).encode(), "the signature does not match the manifest")

    def test_real_signed_foreign_channel_refuses_before_immutable_upload(self):
        self.refused_head(self.signed_head({"channel": "foreign-channel"}), "the build is of channel foreign-channel, and this lamp follows lamp-voice")

    def test_real_signed_newer_schema_refuses_before_immutable_upload(self):
        self.refused_head(self.signed_head({"schema": 999}, unchecked=True), "the manifest is schema 999")

    def test_real_signed_newer_updater_floor_refuses_before_immutable_upload(self):
        self.refused_head(self.signed_head({"min_updater": 999}), "needs updater 999 or newer")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--updater", required=True, type=Path)
    arguments = parser.parse_args()
    UPDATER = arguments.updater.resolve(strict=True)
    unittest.main(argv=[__file__], verbosity=2)
