#!/usr/bin/env python3
"""Offline publisher tests: every git, gh and updater command is simulated."""

import base64
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("publisher", Path(__file__).with_name("lampos-publish.py"))
P = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(P)
SHA = "a" * 40
RETAINED = {"lamp-update", "lamp-voice", "lamp-body-measure", "lamp-body-mood"}
EXCLUDED = {"lamp-station", "schedule_check", "replay_cost"}


def fixture_classification():
    return {"_comment": "Only the four lists classify binaries.",
            "core": ["lamp-voice"], "optional": ["lamp-update", "lamp-absent-optional"],
            "provisioning": ["lamp-body-measure", "lamp-body-mood"],
            "development": ["lamp-station"]}


def assert_release_subset(test, artifact, assets):
    for name in RETAINED:
        test.assertEqual(assets[name]["data"], (artifact / name).read_bytes())
    test.assertTrue(EXCLUDED.isdisjoint(assets))
    sums = "".join(f"{P.digest(artifact / name)}  {name}\n" for name in sorted(RETAINED))
    test.assertEqual(assets["SHA256SUMS"]["data"], sums.encode())
    manifest = json.loads(assets["manifest.json"]["data"])
    test.assertEqual({entry["name"] for entry in manifest["files"]}, RETAINED | {"lamp-product-skills"})
    expected = RETAINED | {"lamp-product-skills", "SHA256SUMS", "manifest.json", P.ENVELOPE}
    expected.update(f"{entry['lamp']}.{entry['unit']}" for entry in manifest["units"])
    test.assertEqual(set(assets), expected)


def encoded(manifest):
    return json.dumps({"format": "lampos-update/1", "key": "0" * 16, "manifest": base64.b64encode(json.dumps(manifest).encode()).decode(), "signature": "fixture-signature"}).encode()


def old_envelope(sequence):
    return encoded({"schema": 1, "channel": "lamp-voice", "sequence": sequence, "commit": "b" * 40, "files_url": f"https://github.com/{P.REPO}/releases/download/{P.HEAD}-{sequence}/", "files": [{"name": "lamp-update", "size": 1, "sha256": hashlib.sha256(b"x").hexdigest()}], "units": []})


class FakeCommands:
    """A stateful GitHub release API, with failures before and after mutations."""
    def __init__(self, sequence=9):
        self.calls = []
        self.release_map = {}
        self.refs = set()
        self.next_id = 1
        self.next_asset = 100
        self.fail = None
        self.key_paths = []
        self.key_modes = []
        self.head_mutations = []
        self.updater_calls = []
        self.api_fail = False
        # GitHub's 5xx, by call, how many times more (#4103). A read's comes
        # before it is answered; "create" and "upload" come after the change.
        self.server_errors = {}
        self.deleted = []
        if sequence is not None:
            release = self.release(P.HEAD, "b" * 40)
            self.add(release, P.ENVELOPE, old_envelope(sequence))
        self.before = self.canonical()

    def release(self, tag, sha):
        release = {"id": self.next_id, "tag_name": tag, "target_commitish": sha, "draft": False, "prerelease": True, "assets": {}}
        self.next_id += 1
        self.release_map[tag] = release
        self.refs.add(f"refs/tags/{tag}")
        return release

    def add(self, release, name, data):
        if name in release["assets"]:
            raise P.Refused("fake duplicate asset")
        asset = {"id": self.next_asset, "name": name, "size": len(data), "state": "uploaded", "data": data}
        self.next_asset += 1
        release["assets"][name] = asset
        return asset

    def canonical(self):
        asset = self.release_map.get(P.HEAD, {}).get("assets", {}).get(P.ENVELOPE)
        return asset["data"] if asset else None

    def asset(self, identifier):
        for release in self.release_map.values():
            for asset in release["assets"].values():
                if asset["id"] == identifier:
                    return release, asset
        raise P.Refused("fake asset absent")

    @staticmethod
    def json_result(value):
        def public(item):
            return {key: val for key, val in item.items() if key not in ("data", "assets")}
        if isinstance(value, list):
            value = [[public(item) for item in page] for page in value]
        return json.dumps(value).encode()

    def server_error(self, kind):
        if self.server_errors.get(kind, 0) > 0:
            self.server_errors[kind] -= 1
            raise P.ServerError("gh api failed (exit 1): HTTP 504: We couldn't respond to your request in time")

    def run(self, words, output=None):
        words = [str(word) for word in words]
        self.calls.append(words)
        if words[0] == "git":
            return (SHA + "\n").encode()
        if Path(words[0]).name == "lamp-update":
            return self.updater(words)
        if words[:2] == ["gh", "release"]:
            return self.release_command(words)
        if words[:2] == ["gh", "api"]:
            require = P.require
            if self.api_fail:
                raise P.Refused("fake API auth/network error")
            if "application/octet-stream" in " ".join(words):
                self.server_error("download")
                endpoint = words[2]
                _, asset = self.asset(int(endpoint.rsplit("/", 1)[1]))
                output.write(asset["data"])
                return b""
            endpoint = words[-1]
            prefix = f"repos/{P.REPO}"
            if endpoint == prefix:
                self.server_error("repository")
                return self.json_result({"full_name": P.REPO, "private": False})
            endpoint = endpoint.removeprefix(prefix + "/")
            if "PATCH" in words and endpoint.count("/") == 1 and endpoint.startswith("releases/"):
                identifier = int(endpoint.split("/")[1])
                release = next(release for release in self.release_map.values() if release["id"] == identifier)
                release["body"] = words[words.index("-f") + 1].removeprefix("body=")
                return self.json_result({"id": identifier, "body": release["body"]})
            if "DELETE" in words:
                _, asset = self.asset(int(endpoint.rsplit("/", 1)[1]))
                release = next(release for release in self.release_map.values() if asset["name"] in release["assets"] and release["assets"][asset["name"]] is asset)
                self.deleted.append(asset["name"])
                del release["assets"][asset["name"]]
                return b""
            if "PATCH" not in words:
                self.server_error("releases" if endpoint.startswith("releases?") else "refs" if endpoint.startswith("git/") else "assets")
            if endpoint.startswith("releases?"):
                if self.fail == "release-lookup":
                    raise P.Refused("fake release lookup HTTP 503")
                if self.fail == "malformed-lookup":
                    return b'{"message":"unexpected API object"}'
                return self.json_result([list(self.release_map.values())])
            if endpoint.startswith("git/matching-refs/tags/"):
                tag = endpoint.removeprefix("git/matching-refs/tags/")
                return json.dumps([{"ref": ref} for ref in sorted(self.refs) if ref.startswith("refs/tags/" + tag)]).encode()
            if endpoint.endswith("/assets?per_page=100"):
                identifier = int(endpoint.split("/")[1])
                release = next(release for release in self.release_map.values() if release["id"] == identifier)
                return self.json_result([list(release["assets"].values())])
            if "PATCH" in words:
                identifier = int(endpoint.rsplit("/", 1)[1])
                release, asset = self.asset(identifier)
                name = words[words.index("-f") + 1].removeprefix("name=")
                old_name = asset["name"]
                promoting = old_name.startswith("lampos-manifest-staged-") and name == P.ENVELOPE
                restoring = old_name.startswith("lampos-manifest-previous-") and name == P.ENVELOPE
                if old_name == P.ENVELOPE and self.fail == "rename-old":
                    raise P.Refused("fake old rename failed")
                if promoting and self.fail in ("rename-candidate", "restore"):
                    raise P.Refused("fake promotion rename failed")
                if restoring and self.fail == "restore":
                    raise P.Refused("fake restore failed")
                require(name not in release["assets"], "fake duplicate name")
                del release["assets"][old_name]
                asset["name"] = name
                release["assets"][name] = asset
                self.head_mutations.append((identifier, old_name, name))
                if self.fail == "rename-old-response" and old_name == P.ENVELOPE:
                    self.fail = None
                    raise P.Refused("fake lost response after old rename")
                if self.fail == "rename-new-response" and promoting:
                    self.fail = None
                    raise P.Refused("fake lost response after candidate rename")
                return self.json_result({"id": identifier, "name": name})
        raise AssertionError(f"unexpected simulated command: {words}")

    def updater(self, words):
        self.updater_calls.append(words)
        action = words[1]
        options = dict(zip(words[2::2], words[3::2]))
        if action == "manifest":
            stage = Path(options["--binaries"])
            (stage / "lamp-product-skills").write_bytes(b"skill fixture")
            unit = "lamp-4ace.lamp-voice.service"
            (stage / unit).write_bytes(b"unit fixture")
            if self.fail == "manifest-extra-binary":
                (stage / "lamp-station").write_bytes(b"unexpected development binary")
            entries = [{"name": file.name, **P.file_info(file)} for file in sorted(stage.glob("lamp-*")) if P.BINARY.fullmatch(file.name)]
            manifest = {"schema": 1, "channel": options["--channel"], "sequence": int(options["--sequence"]), "commit": options["--commit"], "files_url": options["--files-url"], "files": entries, "units": [{"lamp": "lamp-4ace", "unit": "lamp-voice.service", **P.file_info(stage / unit)}]}
            if self.fail == "wrong-manifest-sha":
                manifest["commit"] = "c" * 40
            if self.fail == "bad-manifest-hash":
                manifest["files"][0]["sha256"] = "0" * 64
            (stage / "manifest.json").write_text(json.dumps(manifest))
            return b"made\n"
        if action in ("sign", "public-key"):
            key = Path(options["--key-file"])
            self.key_paths.append(key)
            self.key_modes.append(key.stat().st_mode & 0o777)
            if action == "public-key":
                if self.fail == "public-key":
                    raise P.Refused("fake public-key failure")
                return base64.b64encode(bytes(32)) + b"\n"
            if self.fail == "sign":
                raise P.Refused("fake signing failure")
            payload = Path(options["--manifest"]).read_bytes()
            envelope = {"format": "lampos-update/1", "key": "0" * 16, "manifest": base64.b64encode(payload).decode(), "signature": "fixture-signature"}
            Path(options["--out"]).write_text(json.dumps(envelope))
            return b"signed with fixture fingerprint\n"
        if action == "check":
            self.assert_check(options)
            channel = Path(options["--channel-url"].removeprefix("file://"))
            envelope = json.loads((channel / P.ENVELOPE).read_bytes())
            P.require(envelope["signature"] == "fixture-signature", "fake bad signature")
            manifest = json.loads(base64.b64decode(envelope["manifest"]))
            P.require(manifest["channel"] == options["--channel"], "fake wrong channel")
            if self.fail == "check":
                raise P.Refused("fake verifier refusal")
            if self.fail == "check-empty-success":
                return b"nothing published\n"
            if self.fail == "check-wrote":
                (Path(options["--root"]) / "changed").write_text("not read-only")
            if self.fail == "check-mkdir":
                (Path(options["--root"]) / "empty").mkdir()
            if self.fail == "check-symlink":
                (Path(options["--root"]) / "dangling").symlink_to("absent")
            return f"up to date: build {manifest['sequence']}\n".encode()
        raise AssertionError(action)

    @staticmethod
    def assert_check(options):
        assert options["--apply"] == "off"
        assert options["--channel-url"].startswith("file:///")
        assert options["--lamp"] == "publish-check"
        assert Path(options["--root"]).is_dir()

    def release_command(self, words):
        action, tag = words[2:4]
        flags = {"--repo", "--target", "--title", "--notes-file"}
        args = {}
        files = []
        cursor = 4
        while cursor < len(words):
            value = words[cursor]
            if value in flags:
                args[value] = words[cursor + 1]
                cursor += 2
            elif value.startswith("--"):
                assert value in ("--prerelease", "--latest=false")
                cursor += 1
            else:
                files.append(Path(value))
                cursor += 1
        if action == "create":
            assert tag not in self.release_map
            release = self.release(tag, args["--target"])
            for index, file in enumerate(files):
                if self.fail == "create-upload" and tag != P.HEAD and index == 1:
                    raise P.Refused("fake partial immutable upload")
                if tag != P.HEAD and index == 2 and self.server_errors.get("create-partial", 0) > 0:
                    self.server_error("create-partial")
                if tag != P.HEAD and index == 2 and self.server_errors.get("create-open", 0) > 0:
                    # A failed upload: GitHub keeps the name, its asset not `uploaded`.
                    self.add(release, file.name, file.read_bytes()[:3])["state"] = "open"
                    self.server_error("create-open")
                if self.fail == "missing-upload" and tag != P.HEAD and index == 1:
                    continue
                data = file.read_bytes()
                if self.fail == "corrupt-upload" and tag != P.HEAD and index == 1:
                    data = bytes([data[0] ^ 1]) + data[1:]
                self.add(release, file.name, data)
            if tag != P.HEAD:
                self.server_error("create")
            if self.fail == "changed-head" and tag != P.HEAD:
                asset = self.release_map[P.HEAD]["assets"][P.ENVELOPE]
                asset["data"] = old_envelope(11)
                asset["size"] = len(asset["data"])
            return b"created\n"
        assert action == "upload" and "--clobber" not in words
        if self.fail == "stage-upload" and tag == P.HEAD:
            raise P.Refused("fake stage upload failed")
        release = self.release_map[tag]
        for file in files:
            data = file.read_bytes()
            if self.fail == "corrupt-stage" and tag == P.HEAD:
                data = data[:-1] + bytes([data[-1] ^ 1])
            self.add(release, file.name, data)
        self.server_error("upload")
        return b"uploaded\n"


class CommandsTests(unittest.TestCase):
    def test_release_authorization_is_scoped_to_each_gh_child_without_mutating_context(self):
        environ = {
            P.SECRET: "signing-key-fixture", "GH_TOKEN": "release-authorization-fixture",
            "GITHUB_TOKEN": "builtin-authorization-fixture",
            "GH_ENTERPRISE_TOKEN": "enterprise-authorization-fixture",
            "GITHUB_ENTERPRISE_TOKEN": "enterprise-builtin-fixture",
            "PATH": "/mock", "GITHUB_REPOSITORY": P.REPO, "GITHUB_SHA": SHA,
        }
        original = environ.copy()
        context = {key: original[key] for key in ("PATH", "GITHUB_REPOSITORY", "GITHUB_SHA")}
        commands = P.Commands(environ)
        children = []
        def completed(words, **kwargs):
            children.append(kwargs["env"].copy())
            return P.subprocess.CompletedProcess(words, 0, b"simulated output", b"")
        words = (["gh", "api", "simulated"], ["git", "rev-parse", "HEAD"],
                 ["/mock/lamp-update", "check"], ["/mock/gh", "release", "upload", "simulated"])
        with mock.patch.object(P.subprocess, "run", side_effect=completed) as run:
            for command in words:
                self.assertEqual(commands.run(command), b"simulated output")
        self.assertEqual(run.call_count, 4)
        for index, child in enumerate(children):
            with self.subTest(child=index):
                expected = {**context, "GH_TOKEN": original["GH_TOKEN"]} if index in (0, 3) else context
                self.assertEqual(child, expected)
        self.assertEqual(commands.environ, context)
        self.assertEqual(environ, original)
        self.assertTrue({original[key] for key in (P.SECRET, "GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN")} <= set(commands.redactions))

    def failure(self, stderr, environ=None, words=None):
        words = words or ["gh", "api", "argv-must-not-appear"]
        result = P.subprocess.CompletedProcess(words, 17, b"stdout-must-not-appear", stderr)
        logged = io.StringIO()
        with mock.patch.object(P.subprocess, "run", return_value=result) as run, mock.patch.object(P.sys, "stderr", logged):
            with self.assertRaises(P.Refused) as caught:
                P.Commands(environ or {}).run(words)
        run.assert_called_once()
        message = str(caught.exception)
        self.assertEqual(logged.getvalue(), message + "\n")
        self.assertNotIn("stdout-must-not-appear", message)
        self.assertNotIn("argv-must-not-appear", message)
        return message

    def test_gh_failures_use_fixed_labels_without_echoing_argv(self):
        for prefix, label in ((["gh", "api"], "gh api"), (["/mock/gh", "release", "create"], "gh release create"), (["gh", "release", "upload"], "gh release upload"), (["gh", "argv-must-not-appear"], "gh")):
            with self.subTest(label=label):
                words = prefix + ["argv-must-not-appear", "--title", "private-title", "private-local-file"]
                message = self.failure(b"HTTP 403: denied\nsecond-line-secret", words=words)
                self.assertEqual(message, label + " failed (exit 17): HTTP 403: denied")
                for value in ("private-title", "private-local-file", "second-line-secret"):
                    self.assertNotIn(value, message)

    def test_github_5xx_is_a_server_error_and_4xx_is_not(self):
        for stderr in (b"HTTP 504: We couldn't respond to your request in time", b"gh: Server Error (HTTP 500)", b"HTTP 502: Bad Gateway", b"HTTP 503: unavailable"):
            with self.subTest(stderr=stderr):
                result = P.subprocess.CompletedProcess(["gh", "api"], 1, b"", stderr)
                with mock.patch.object(P.subprocess, "run", return_value=result), mock.patch.object(P.sys, "stderr", io.StringIO()):
                    with self.assertRaises(P.ServerError):
                        P.Commands({}).run(["gh", "api", "x"])
        for stderr in (b"HTTP 403: Resource not accessible by integration", b"HTTP 404: Not Found", b"HTTP 422: Validation Failed", b"HTTP 5040", b"stream error: CANCEL"):
            with self.subTest(stderr=stderr):
                result = P.subprocess.CompletedProcess(["gh", "api"], 1, b"", stderr)
                with mock.patch.object(P.subprocess, "run", return_value=result), mock.patch.object(P.sys, "stderr", io.StringIO()):
                    with self.assertRaises(P.Refused) as caught:
                        P.Commands({}).run(["gh", "api", "x"])
                    self.assertNotIsInstance(caught.exception, P.ServerError)

    def test_configured_secrets_and_split_private_key_are_redacted(self):
        environ = {"GH_TOKEN": "q7!", P.SECRET: "-----BEGIN PRIVATE KEY-----\nshort-pem-body\n-----END PRIVATE KEY-----", "SERVICE_PASSWORD": "password-demo", "OTHER_CREDENTIAL_VALUE": "credential-demo"}
        for value in (environ["GH_TOKEN"], *environ[P.SECRET].splitlines(), environ["SERVICE_PASSWORD"], environ["OTHER_CREDENTIAL_VALUE"]):
            with self.subTest(value=value):
                self.assertEqual(self.failure(("denied " + value + " end").encode(), environ), "gh api failed (exit 17): denied <redacted> end")

    def test_redaction_happens_before_diagnostic_truncation(self):
        secret = "short-secret-crossing-boundary"
        prefix = "note " * 62 + "x "
        message = self.failure((prefix + secret + " tail").encode(), {"GH_TOKEN": secret})
        detail = message.split(": ", 1)[1]
        self.assertEqual(detail, (prefix + "<redacted> tail")[:320])
        self.assertEqual(len(detail), 320)
        self.assertNotIn("short-secret", message)

    def test_short_tokens_with_empty_jose_components_are_redacted(self):
        # An unsecured JWT has an empty signature; direct JWE has no wrapped key.
        for token in ("eyJhbGciOiJub25lIn0.eyJzdWIiOiJzIn0.", "eyJhbGciOiJkaXIifQ..a.b.c"):
            with self.subTest(token=token):
                self.assertEqual(self.failure(("denied " + token + " end").encode()), "gh api failed (exit 17): denied <redacted> end")

    def test_credential_shapes_and_assignments_are_redacted(self):
        for secret in ("ghp_demo", "gho_demo", "ghu_demo", "ghs_demo", "ghr_demo", "github_pat_demo", "sk-abcdefghijklmnop", "pk_abcdefghijklmnop", "rk-abcdefghijklmnop", "Ab9_" * 10, "eyJheader.payload.signature", "eyJheader.key.iv.cipher.tag"):
            with self.subTest(secret=secret):
                self.assertEqual(self.failure(("denied " + secret + " end").encode()), "gh api failed (exit 17): denied <redacted> end")
        for field in ("token=small", "access_token: small", "API-key='two words'", 'password="two words"', "secret: small", "key=small"):
            with self.subTest(field=field):
                message = self.failure(("denied " + field).encode())
                self.assertIn("<redacted>", message)
                self.assertNotIn("small", message)
                self.assertNotIn("two words", message)

    def test_authorization_schemes_and_standalone_bearer_are_redacted(self):
        for field in ("Authorization: Bearer small", "authorization=Basic tiny extra-credential", "Authorization: unfamiliar tiny extra-credential", "Bearer small"):
            with self.subTest(field=field):
                message = self.failure(("denied " + field).encode())
                self.assertIn("<redacted>", message)
                for value in ("small", "tiny", "extra-credential"):
                    self.assertNotIn(value, message)

    def test_url_credentials_and_query_values_are_redacted(self):
        message = self.failure(b"GET https://user:pass@example.invalid/a?token=small&api_key=tiny&ok=yes")
        self.assertEqual(message, "gh api failed (exit 17): GET https://<redacted>@example.invalid/a?<redacted>")
        self.assertNotIn("unknown-value", self.failure(b"GET https://example.invalid/a?unknown-field=unknown-value"))

    def test_ansi_controls_and_invalid_utf8_do_not_escape_first_line(self):
        message = self.failure(b"\x1b[31mHTTP\x1b[0m\x00\t 403 \xff denied\nsecond-line-secret")
        self.assertEqual(message, "gh api failed (exit 17): HTTP 403 \ufffd denied")
        self.assertTrue(all(char.isprintable() for char in message))
        self.assertEqual(self.failure(b"HTTP\x1b]0;private-title\x07 denied"), "gh api failed (exit 17): HTTP denied")

    def test_missing_and_empty_first_stderr_lines_are_safe(self):
        for stderr, expected in ((None, "no stderr"), (b"", "no stderr"), (b"\nsecond-line-secret", "empty stderr line"), (b"\x00\t", "empty stderr line")):
            with self.subTest(stderr=stderr):
                self.assertEqual(self.failure(stderr), "gh api failed (exit 17): " + expected)

    def test_success_preserves_stdout_and_child_environment(self):
        environ = {P.SECRET: "private-key-fixture", "GH_TOKEN": "auth-fixture", "PATH": "/mock"}
        original = environ.copy()
        words = ["gh", "api", "private-argv"]
        result = P.subprocess.CompletedProcess(words, 0, b"successful output", b"ignored private stderr")
        logged = io.StringIO()
        with mock.patch.object(P.subprocess, "run", return_value=result) as run, mock.patch.object(P.sys, "stderr", logged):
            self.assertEqual(P.Commands(environ).run(words), b"successful output")
        run.assert_called_once_with(words, env={"GH_TOKEN": "auth-fixture", "PATH": "/mock"}, stdin=P.subprocess.DEVNULL, stdout=P.subprocess.PIPE, stderr=P.subprocess.PIPE, timeout=600, check=False)
        self.assertEqual(environ, original)
        self.assertEqual(logged.getvalue(), "")

    def test_output_file_is_forwarded_without_echoing_output(self):
        output = io.BytesIO()
        def completed(words, **kwargs):
            self.assertIs(kwargs["stdout"], output)
            output.write(b"downloaded bytes")
            return P.subprocess.CompletedProcess(words, 0, None, b"ignored private stderr")
        logged = io.StringIO()
        with mock.patch.object(P.subprocess, "run", side_effect=completed) as run, mock.patch.object(P.sys, "stderr", logged):
            self.assertEqual(P.Commands({}).run(["gh", "api", "download"], output), b"")
        run.assert_called_once()
        self.assertEqual(output.getvalue(), b"downloaded bytes")
        self.assertEqual(logged.getvalue(), "")

    def test_non_gh_failures_remain_generic_and_silent(self):
        for program in ("git", "/mock/lamp-update"):
            with self.subTest(program=program):
                result = P.subprocess.CompletedProcess([], 9, b"private stdout", b"private stderr")
                logged = io.StringIO()
                with mock.patch.object(P.subprocess, "run", return_value=result) as run, mock.patch.object(P.sys, "stderr", logged):
                    with self.assertRaises(P.Refused) as caught:
                        P.Commands({}).run([program, "private argv"])
                run.assert_called_once()
                self.assertEqual(str(caught.exception), Path(program).name + " failed (exit 9)")
                self.assertEqual(logged.getvalue(), "")

    def test_spawn_and_timeout_failures_do_not_echo_exception_details(self):
        words = ["gh", "api", "private argv"]
        for failure in (OSError("private spawn detail"), P.subprocess.TimeoutExpired(words, 600, output=b"private stdout", stderr=b"private stderr")):
            with self.subTest(failure=type(failure).__name__):
                logged = io.StringIO()
                with mock.patch.object(P.subprocess, "run", side_effect=failure) as run, mock.patch.object(P.sys, "stderr", logged):
                    with self.assertRaises(P.Refused) as caught:
                        P.Commands({}).run(words)
                run.assert_called_once()
                self.assertEqual(str(caught.exception), "gh api did not complete")
                self.assertEqual(logged.getvalue(), "")


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="lampos-publish-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.artifact = self.root / "artifact"
        self.artifact.mkdir()
        for name in sorted(RETAINED | EXCLUDED):
            (self.artifact / name).write_bytes((name + " fixture").encode())
        (self.artifact / "lamp-update").chmod(0o600)
        self.checksums()
        self.classification = self.root / "lampOS/deploy/package-binaries.json"
        self.classification.parent.mkdir(parents=True)
        self.classification.write_text(json.dumps(fixture_classification()))
        self.env = {P.ENABLE: "on", P.BOOTSTRAP: "off", P.SECRET: "not-a-real-key", "GH_TOKEN": "release-authorization-fixture", "GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": P.REPO, "GITHUB_EVENT_NAME": "push", "GITHUB_REF": "refs/heads/lamp-voice", "GITHUB_SHA": SHA, "GITHUB_RUN_NUMBER": "10", "GITHUB_RUN_ID": "1234", "GITHUB_RUN_ATTEMPT": "1"}
        self.fake = FakeCommands()

    def checksums(self):
        sums = "".join(f"{P.digest(path)}  {path.name}\n" for path in sorted(self.artifact.iterdir()) if path.name != "SHA256SUMS")
        (self.artifact / "SHA256SUMS").write_text(sums)

    def publish(self):
        publisher = P.Publisher(self.artifact, self.root, self.env, self.fake)
        self.pauses = []
        publisher.sleep = self.pauses.append
        return publisher.run()

    def refused_unchanged(self, pattern=None, canonical=True):
        original = P.tree_info(self.artifact)
        with self.assertRaisesRegex(P.Refused, pattern or ".+"):
            self.publish()
        self.assertEqual(P.tree_info(self.artifact), original)
        if canonical:
            self.assertEqual(self.fake.before, self.fake.canonical())
        self.assertTrue(all(not path.exists() for path in self.fake.key_paths))

    def test_missing_release_authorization_refuses_before_commands_or_signing(self):
        for credential in (None, "", " \t\r\n"):
            with self.subTest(credential=credential):
                self.fake = FakeCommands()
                self.env["GITHUB_TOKEN"] = "ambient-builtin-authorization-fixture"
                if credential is None:
                    self.env.pop("GH_TOKEN", None)
                else:
                    self.env["GH_TOKEN"] = credential
                original = self.env.copy()
                self.refused_unchanged("^LAMPOS_RELEASE_TOKEN is absent from the protected publication environment$")
                self.assertEqual(self.fake.calls, [])
                self.assertEqual(self.fake.updater_calls, [])
                self.assertEqual(self.fake.key_paths, [])
                self.assertEqual(self.env, original)

    def test_existing_channel_advances_only_after_verified_build_and_stage(self):
        original = P.tree_info(self.artifact)
        result = self.publish()
        self.assertEqual(P.tree_info(self.artifact), original)
        self.assertEqual(result["published"], "lampos-lamp-voice-10")
        assets = self.fake.release_map[P.HEAD]["assets"]
        self.assertNotEqual(self.fake.before, self.fake.canonical())
        self.assertIn(self.fake.before, [asset["data"] for name, asset in assets.items() if "previous" in name])
        self.assertEqual(self.fake.key_modes, [0o600, 0o600])
        self.assertTrue(all(not path.exists() for path in self.fake.key_paths))
        self.assertFalse(any("--clobber" in call for call in self.fake.calls))
        released = self.fake.release_map[result["published"]]["assets"]
        assert_release_subset(self, self.artifact, released)

    def test_excluded_bytes_are_verified_before_any_updater_execution(self):
        for name in sorted(EXCLUDED):
            with self.subTest(name=name):
                original = (self.artifact / name).read_bytes()
                try:
                    (self.artifact / name).write_bytes(b"tampered excluded bytes")
                    self.refused_unchanged("checksum mismatch")
                    self.assertEqual(self.fake.updater_calls, [])
                finally:
                    (self.artifact / name).write_bytes(original)

    def test_invalid_classification_never_executes_artifact(self):
        original = fixture_classification()
        cases = [b"not json", b"[]", b'{"core":[],"core":[]}',
                 json.dumps({key: value for key, value in original.items() if key != "optional"}).encode(),
                 json.dumps({**original, "future": []}).encode(),
                 json.dumps({**original, "_comment": []}).encode(),
                 json.dumps({**original, "core": "lamp-voice"}).encode(),
                 json.dumps({**original, "core": ["lamp-voice", "lamp-voice"]}).encode(),
                 json.dumps({**original, "development": ["lamp-update"]}).encode()]
        for name in (None, 1, "../lamp-escape", "lamp-../../escape", "lamp-Bad", "lamp_bad", "", "lamp-" + "x" * 40):
            cases.append(json.dumps({**original, "core": [name]}).encode())
        for raw in cases:
            with self.subTest(raw=raw):
                self.classification.write_bytes(raw)
                self.refused_unchanged()
                self.assertEqual(self.fake.updater_calls, [])
                self.assertFalse(any(call[0] == "gh" for call in self.fake.calls))

    def test_missing_or_linked_classification_never_executes_artifact(self):
        self.classification.unlink()
        self.refused_unchanged("package classification")
        self.assertEqual(self.fake.updater_calls, [])
        other = self.root / "classification.json"
        other.write_text(json.dumps(fixture_classification()))
        self.classification.symlink_to(other)
        self.refused_unchanged("package classification")
        self.assertEqual(self.fake.updater_calls, [])

    def test_comment_cannot_classify_an_unclassified_artifact_binary(self):
        (self.artifact / "lamp-new-part").write_bytes(b"unclassified")
        self.checksums()
        classification = fixture_classification()
        classification["_comment"] = 'A quoted "lamp-new-part" is not membership.'
        self.classification.write_text(json.dumps(classification))
        self.refused_unchanged("unclassified")
        self.assertEqual(self.fake.updater_calls, [])

    def test_classification_cannot_exclude_the_updater(self):
        classification = fixture_classification()
        classification["optional"].remove("lamp-update")
        classification["development"].append("lamp-update")
        self.classification.write_text(json.dumps(classification))
        self.refused_unchanged("excludes the updater")
        self.assertEqual(self.fake.updater_calls, [])

    def test_manifest_cannot_reintroduce_an_excluded_binary(self):
        self.fake.fail = "manifest-extra-binary"
        self.refused_unchanged("manifest binary inventory differs")
        self.assertFalse(any(call[0] == "gh" for call in self.fake.calls))

    def test_initial_channel_requires_explicit_bootstrap(self):
        self.fake = FakeCommands(None)
        self.refused_unchanged("requires")
        self.assertFalse(self.fake.release_map)

    def test_explicit_bootstrap_creates_a_verified_head(self):
        self.fake = FakeCommands(None)
        self.env[P.BOOTSTRAP] = "on"
        self.publish()
        self.assertIsNotNone(self.fake.canonical())

    def test_bad_activation_and_non_push_contexts_run_no_updater(self):
        for field, value in ((P.ENABLE, "ON"), (P.ENABLE, ""), ("GITHUB_ACTIONS", "false"), ("GITHUB_REPOSITORY", "other/repo"), ("GITHUB_EVENT_NAME", "pull_request"), ("GITHUB_EVENT_NAME", "workflow_dispatch"), ("GITHUB_REF", "refs/heads/other"), ("GITHUB_SHA", "c" * 40), ("GITHUB_RUN_NUMBER", "0"), (P.SECRET, "")):
            with self.subTest(field=field, value=value):
                original = self.env[field]
                self.env[field] = value
                self.refused_unchanged()
                self.assertEqual(self.fake.updater_calls, [])
                self.env[field] = original

    def test_missing_checksums_never_executes_artifact(self):
        (self.artifact / "SHA256SUMS").unlink()
        self.refused_unchanged("lacks")
        self.assertEqual(self.fake.updater_calls, [])

    def test_changed_checksum_never_executes_artifact(self):
        (self.artifact / "lamp-update").write_bytes(b"changed")
        self.refused_unchanged("checksum mismatch")
        self.assertEqual(self.fake.updater_calls, [])

    def test_unlisted_file_never_executes_artifact(self):
        (self.artifact / "extra").write_text("extra")
        self.refused_unchanged("inventory")
        self.assertEqual(self.fake.updater_calls, [])

    def test_symlink_never_executes_artifact(self):
        (self.artifact / "link").symlink_to(self.artifact / "lamp-update")
        self.refused_unchanged("regular")
        self.assertEqual(self.fake.updater_calls, [])

    def test_duplicate_checksum_never_executes_artifact(self):
        path = self.artifact / "SHA256SUMS"
        path.write_text(path.read_text() * 2)
        self.refused_unchanged("duplicate")
        self.assertEqual(self.fake.updater_calls, [])

    def test_wrong_manifest_identity_refuses_before_github(self):
        self.fake.fail = "wrong-manifest-sha"
        self.refused_unchanged("identity")
        self.assertFalse(any(call[0] == "gh" for call in self.fake.calls))

    def test_manifest_hash_mismatch_refuses_before_github(self):
        self.fake.fail = "bad-manifest-hash"
        self.refused_unchanged("hash")
        self.assertFalse(any(call[0] == "gh" for call in self.fake.calls))

    def test_public_key_failure_removes_private_key(self):
        self.fake.fail = "public-key"
        self.refused_unchanged()

    def test_sign_failure_removes_private_key(self):
        self.fake.fail = "sign"
        self.refused_unchanged()

    def test_verifier_failure_and_success_without_proof_refuse(self):
        for failure in ("check", "check-empty-success", "check-wrote", "check-mkdir", "check-symlink"):
            with self.subTest(failure=failure):
                self.fake = FakeCommands()
                self.fake.fail = failure
                self.refused_unchanged()
                self.assertFalse(any(call[0] == "gh" for call in self.fake.calls))

    def test_api_auth_or_network_error_is_not_absence(self):
        self.env[P.BOOTSTRAP] = "on"
        self.fake.api_fail = True
        self.refused_unchanged("API")
        self.assertEqual(self.fake.head_mutations, [])

    def test_bad_existing_signature_is_not_bootstrap(self):
        self.env[P.BOOTSTRAP] = "on"
        asset = self.fake.release_map[P.HEAD]["assets"][P.ENVELOPE]
        value = json.loads(asset["data"])
        value["signature"] = "wrong-signature"
        asset["data"] = json.dumps(value).encode()
        asset["size"] = len(asset["data"])
        self.fake.before = self.fake.canonical()
        self.refused_unchanged("signature")
        self.assertEqual(self.fake.head_mutations, [])

    def test_failed_or_malformed_channel_lookup_is_not_bootstrap(self):
        for failure in ("release-lookup", "malformed-lookup"):
            with self.subTest(failure=failure):
                self.fake = FakeCommands()
                self.fake.fail = failure
                self.env[P.BOOTSTRAP] = "on"
                self.refused_unchanged()
                self.assertEqual(self.fake.head_mutations, [])
                self.assertNotIn("lampos-lamp-voice-10", self.fake.release_map)

    def test_existing_channel_without_envelope_is_not_bootstrap(self):
        self.env[P.BOOTSTRAP] = "on"
        del self.fake.release_map[P.HEAD]["assets"][P.ENVELOPE]
        self.fake.before = None
        self.refused_unchanged("owner repair")

    def test_existing_immutable_tag_without_release_refuses(self):
        self.fake.refs.add("refs/tags/lampos-lamp-voice-10")
        self.refused_unchanged("tag already")
        self.assertEqual(self.fake.head_mutations, [])

    def test_existing_immutable_release_that_is_not_this_build_refuses(self):
        self.fake.release("lampos-lamp-voice-10", "c" * 40)
        self.refused_unchanged("immutable release identity differs")
        self.assertEqual(self.fake.head_mutations, [])
        self.assertNotIn("body", self.fake.release_map["lampos-lamp-voice-10"])

    def test_github_5xx_on_each_read_is_read_again_then_goes_on(self):
        for kind in ("repository", "releases", "refs", "assets", "download"):
            with self.subTest(kind=kind):
                self.fake = FakeCommands()
                self.fake.server_errors[kind] = 1
                result = self.publish()
                self.assertEqual(self.pauses, [5])
                self.assertEqual(self.fake.server_errors[kind], 0)
                self.assertFalse(result["resumed"])
                self.assertEqual(self.fake.canonical(), self.fake.release_map["lampos-lamp-voice-10"]["assets"][P.ENVELOPE]["data"])

    def test_a_read_still_failing_after_three_pauses_refuses(self):
        self.fake.server_errors["releases"] = 99
        self.refused_unchanged("HTTP 504")
        self.assertEqual(self.pauses, [5, 15, 30])
        self.assertEqual(self.fake.head_mutations, [])

    def test_a_4xx_is_never_read_again(self):
        self.fake.api_fail = True
        self.refused_unchanged("auth/network")
        self.assertEqual(self.pauses, [])

    def test_a_5xx_after_the_release_or_stage_was_made_goes_on_without_repeating_it(self):
        for kind in ("create", "upload"):
            with self.subTest(kind=kind):
                self.fake = FakeCommands()
                self.fake.server_errors[kind] = 1
                result = self.publish()
                self.assertEqual(self.pauses, [])
                self.assertEqual(self.fake.server_errors[kind], 0)
                self.assertEqual(result["published"], "lampos-lamp-voice-10")
                self.assertEqual(sum(call[:3] == ["gh", "release", kind] for call in self.fake.calls), 1)
                self.assertEqual(self.fake.canonical(), self.fake.release_map["lampos-lamp-voice-10"]["assets"][P.ENVELOPE]["data"])

    def test_rerun_over_its_own_identical_release_finishes_and_names_its_attempt(self):
        self.fake.fail = "rename-candidate"
        self.refused_unchanged("previous canonical envelope restored")
        self.assertIn("lampos-lamp-voice-10", self.fake.release_map)
        self.fake.fail = None
        self.env["GITHUB_RUN_ATTEMPT"] = "2"
        result = self.publish()
        self.assertTrue(result["resumed"])
        self.assertEqual(sum(call[:3] == ["gh", "release", "create"] for call in self.fake.calls), 1)
        release = self.fake.release_map["lampos-lamp-voice-10"]
        self.assertEqual(self.fake.canonical(), release["assets"][P.ENVELOPE]["data"])
        self.assertIn("attempt 2, sequence 10.", release["body"])
        assert_release_subset(self, self.artifact, release["assets"])

    def test_rerun_after_the_channel_switched_finishes_without_touching_it(self):
        self.publish()
        mutations = list(self.fake.head_mutations)
        head = self.fake.canonical()
        self.env["GITHUB_RUN_ATTEMPT"] = "2"
        result = self.publish()
        self.assertTrue(result["resumed"])
        self.assertEqual(self.fake.head_mutations, mutations)
        self.assertEqual(self.fake.canonical(), head)
        self.assertIn("attempt 2,", self.fake.release_map["lampos-lamp-voice-10"]["body"])

    def test_rerun_over_a_switched_head_that_is_not_this_build_leaves_the_notes(self):
        self.publish()
        # A head at sequence 10, validly signed, that is not this build's.
        asset = self.fake.release_map[P.HEAD]["assets"][P.ENVELOPE]
        asset["data"] = old_envelope(10)
        asset["size"] = len(asset["data"])
        self.env["GITHUB_RUN_ATTEMPT"] = "2"
        with self.assertRaisesRegex(P.Refused, "channel head at this sequence is not this build"):
            self.publish()
        # Refused before the notes were touched: no PATCH of the release.
        self.assertNotIn("body", self.fake.release_map["lampos-lamp-voice-10"])
        self.assertFalse(any("PATCH" in call and any(word.startswith("body=") for word in call) for call in self.fake.calls))

    def test_a_release_missing_assets_gets_only_those_after_the_rest_are_checked(self):
        # Silently short, cut short by a 5xx inside `gh release create`, or
        # cut short by a refusal and then run again (Reviewer-two's P3 on #4121).
        for case in ("missing-upload", "create-partial", "rerun"):
            with self.subTest(case=case):
                self.fake = FakeCommands()
                self.env["GITHUB_RUN_ATTEMPT"] = "1"
                if case == "missing-upload":
                    self.fake.fail = "missing-upload"
                elif case == "create-partial":
                    self.fake.server_errors["create-partial"] = 1
                else:
                    self.fake.fail = "create-upload"
                    self.refused_unchanged("partial immutable upload")
                    self.fake.fail = None
                    self.env["GITHUB_RUN_ATTEMPT"] = "2"
                result = self.publish()
                release = self.fake.release_map["lampos-lamp-voice-10"]
                assert_release_subset(self, self.artifact, release["assets"])
                self.assertEqual(self.fake.canonical(), release["assets"][P.ENVELOPE]["data"])
                uploads = [call for call in self.fake.calls if call[:4] == ["gh", "release", "upload", "lampos-lamp-voice-10"]]
                self.assertEqual(len(uploads), 1)
                self.assertEqual(result["resumed"], case == "rerun")

    def test_an_upload_github_left_unfinished_is_deleted_and_uploaded_again(self):
        # In this run (a 504 mid-upload) and on a re-run (Fourteen's P3 on #4121).
        for rerun in (False, True):
            with self.subTest(rerun=rerun):
                self.fake = FakeCommands()
                self.env["GITHUB_RUN_ATTEMPT"] = "1"
                self.fake.server_errors["create-open"] = 1
                if rerun:
                    original = self.fake.release_command
                    def refusing(words, original=original):
                        try:
                            return original(words)
                        except P.ServerError as error:
                            raise P.Refused(str(error).replace("504", "403"))
                    self.fake.release_command = refusing
                    self.refused_unchanged()
                    self.fake.release_command = original
                    self.env["GITHUB_RUN_ATTEMPT"] = "2"
                result = self.publish()
                release = self.fake.release_map["lampos-lamp-voice-10"]
                self.assertEqual(len(self.fake.deleted), 1)
                self.assertTrue(all(asset["state"] == "uploaded" for asset in release["assets"].values()))
                assert_release_subset(self, self.artifact, release["assets"])
                self.assertEqual(self.fake.canonical(), release["assets"][P.ENVELOPE]["data"])
                self.assertEqual(result["resumed"], rerun)

    def test_an_unfinished_asset_that_is_not_this_builds_is_refused_and_kept(self):
        self.fake.release("lampos-lamp-voice-10", SHA)
        self.fake.add(self.fake.release_map["lampos-lamp-voice-10"], "lamp-stranger", b"x")["state"] = "open"
        self.refused_unchanged("uploaded asset inventory differs")
        self.assertEqual(self.fake.deleted, [])

    def test_elsewhere_an_unfinished_asset_is_still_uncertain(self):
        self.fake.add(self.fake.release_map[P.HEAD], "lampos-manifest-staged-9.json", b"x")["state"] = "open"
        self.refused_unchanged("uncertain release assets")
        self.assertEqual(self.fake.deleted, [])

    def backups(self, count, staged=()):
        # Older promotions' backups, as a channel holds them: sequences below
        # this run's (500).
        self.env["GITHUB_RUN_NUMBER"] = "500"
        self.fake = FakeCommands(499)
        head = self.fake.release_map[P.HEAD]
        for sequence in range(1, count + 1):
            self.fake.add(head, f"lampos-manifest-previous-{sequence}-{9000 + sequence}.json", b"old head")
        for sequence in staged:
            self.fake.add(head, f"lampos-manifest-staged-{sequence}.json", b"left stage")
        return head

    def test_a_verified_switch_keeps_the_newest_backups_and_deletes_the_rest(self):
        # GitHub's cap is 1000 assets on one release (#4103's channel).
        head = self.backups(30, staged=(3, 7, 512))
        result = self.publish()
        names = set(head["assets"])
        kept = sorted(int(P.BACKUP.fullmatch(name)[1]) for name in names if P.BACKUP.fullmatch(name))
        self.assertEqual(len(kept), P.KEEP_BACKUPS)
        self.assertEqual(kept[-1], 500, "this switch's own backup is kept")
        self.assertEqual(kept[0], 12)
        # Stages of older sequences go; a newer run's (512) stays.
        self.assertEqual(sorted(name for name in names if P.STAGED.fullmatch(name)), ["lampos-manifest-staged-512.json"])
        self.assertEqual(self.fake.canonical(), self.fake.release_map[result["published"]]["assets"][P.ENVELOPE]["data"])
        self.assertTrue(all(not name.startswith(("lampos-manifest-previous-1-", "lampos-manifest-staged-3")) for name in names))

    def test_a_cleanup_that_fails_leaves_the_build_published(self):
        self.backups(30)
        original = self.fake.run
        def refusing(words, output=None):
            if "DELETE" in [str(word) for word in words]:
                raise P.Refused("gh api failed (exit 1): HTTP 403: delete refused")
            return original(words, output)
        self.fake.run = refusing
        logged = io.StringIO()
        with mock.patch.object(P.sys, "stderr", logged):
            result = self.publish()
        self.assertEqual(result["published"], "lampos-lamp-voice-500")
        self.assertEqual(self.fake.canonical(), self.fake.release_map["lampos-lamp-voice-500"]["assets"][P.ENVELOPE]["data"])
        self.assertIn("channel cleanup stopped, the build is published", logged.getvalue())

    def test_one_run_deletes_at_most_its_bound_oldest_first(self):
        head = self.backups(300)
        self.publish()
        left = sorted(int(P.BACKUP.fullmatch(name)[1]) for name in head["assets"] if P.BACKUP.fullmatch(name))
        # 300 older ones and this switch's: 250 deleted, the oldest first.
        self.assertEqual(len(left), 301 - P.MOST_PRUNED)
        self.assertEqual(left[0], P.MOST_PRUNED + 1)
        self.assertEqual(left[-1], 500)

    def test_a_failed_switch_deletes_nothing(self):
        head = self.backups(30)
        self.fake.fail = "rename-candidate"
        self.refused_unchanged("previous canonical envelope restored")
        self.assertEqual(self.fake.deleted, [])
        self.assertEqual(sum(bool(P.BACKUP.fullmatch(name)) for name in head["assets"]), 30)

    def test_a_short_release_with_a_changed_asset_is_not_completed(self):
        self.fake.fail = "create-upload"
        self.refused_unchanged("partial immutable upload")
        self.fake.fail = None
        asset = next(iter(self.fake.release_map["lampos-lamp-voice-10"]["assets"].values()))
        asset["data"] = b"X" + asset["data"][1:]
        self.env["GITHUB_RUN_ATTEMPT"] = "2"
        self.refused_unchanged("uploaded asset hash differs")
        self.assertFalse(any(call[:4] == ["gh", "release", "upload", "lampos-lamp-voice-10"] for call in self.fake.calls))

    def test_rerun_over_a_release_that_differs_refuses(self):
        self.fake.fail = "rename-candidate"
        self.refused_unchanged("previous canonical envelope restored")
        self.fake.fail = None
        asset = self.fake.release_map["lampos-lamp-voice-10"]["assets"]["lamp-voice"]
        asset["data"] = b"X" + asset["data"][1:]
        self.env["GITHUB_RUN_ATTEMPT"] = "2"
        self.refused_unchanged("uploaded asset hash differs")
        self.assertNotIn("body", self.fake.release_map["lampos-lamp-voice-10"])

    def test_stale_or_repeated_sequence_never_mutates_head(self):
        for sequence in (10, 11):
            with self.subTest(sequence=sequence):
                self.fake = FakeCommands(sequence)
                self.refused_unchanged("stale or repeated")
                self.assertEqual(self.fake.head_mutations, [])
                self.assertNotIn("lampos-lamp-voice-10", self.fake.release_map)

    def test_failed_or_corrupt_build_upload_never_mutates_head(self):
        for failure in ("create-upload", "corrupt-upload"):
            with self.subTest(failure=failure):
                self.fake = FakeCommands()
                self.fake.fail = failure
                self.refused_unchanged()
                self.assertEqual(self.fake.head_mutations, [])

    def test_failed_or_corrupt_staging_never_mutates_head(self):
        for failure in ("stage-upload", "corrupt-stage"):
            with self.subTest(failure=failure):
                self.fake = FakeCommands()
                self.fake.fail = failure
                self.refused_unchanged()
                self.assertEqual(self.fake.head_mutations, [])

    def test_channel_changed_during_upload_is_left_untouched(self):
        self.fake.fail = "changed-head"
        self.refused_unchanged("channel changed", canonical=False)
        self.assertEqual(self.fake.canonical(), old_envelope(11))
        self.assertEqual(self.fake.head_mutations, [])

    def test_candidate_rename_failure_restores_previous_head(self):
        self.fake.fail = "rename-candidate"
        self.refused_unchanged("previous canonical envelope restored")
        self.assertTrue(self.fake.head_mutations)

    def test_old_rename_failure_preserves_previous_head(self):
        self.fake.fail = "rename-old"
        self.refused_unchanged("previous canonical envelope restored")
        self.assertEqual(self.fake.head_mutations, [])

    def test_lost_response_after_either_rename_restores_previous_head(self):
        for failure in ("rename-old-response", "rename-new-response"):
            with self.subTest(failure=failure):
                self.fake = FakeCommands()
                self.fake.fail = failure
                self.refused_unchanged("previous canonical envelope restored")

    def test_failed_restoration_reports_retained_asset_ids(self):
        self.fake.fail = "restore"
        self.refused_unchanged("restoration also failed; owner must inspect", canonical=False)
        self.assertIsNone(self.fake.canonical())
        self.assertIn(self.fake.before, [asset["data"] for asset in self.fake.release_map[P.HEAD]["assets"].values()])

    def test_commands_never_pass_private_key_environment_to_children(self):
        commands = P.Commands(self.env)
        self.assertNotIn(P.SECRET, commands.environ)
        self.assertEqual(commands.environ["GITHUB_REPOSITORY"], P.REPO)

    def test_promotion_wrapper_keeps_immediate_safe_gh_diagnostic_visible(self):
        original = self.fake.run
        failed = False
        def simulated(words, output=None):
            nonlocal failed
            if not failed and tuple(words[:2]) == ("gh", "api") and "PATCH" in words:
                failed = True
                return P.Commands({"GH_TOKEN": "auth-fixture"}).run(words, output)
            return original(words, output)
        result = P.subprocess.CompletedProcess([], 22, b"private stdout", b"HTTP 403: auth-fixture denied\nprivate second line")
        logged = io.StringIO()
        with mock.patch.object(self.fake, "run", side_effect=simulated), mock.patch.object(P.subprocess, "run", return_value=result) as run, mock.patch.object(P.sys, "stderr", logged):
            self.refused_unchanged("previous canonical envelope restored")
        run.assert_called_once()
        self.assertTrue(failed)
        self.assertEqual(logged.getvalue(), "gh api failed (exit 22): HTTP 403: <redacted> denied\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)
