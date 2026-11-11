#!/usr/bin/env python3
"""Offline CDN mirror tests: gcloud, the network and the updater are simulated."""

import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("mirror", Path(__file__).with_name("lampos-mirror.py"))
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)
BUCKET = "test-bucket-1"
SEQUENCE = 4242
RELEASE = f"https://github.com/{M.REPO}/releases/download/lampos-lamp-voice-{SEQUENCE}/"
MODELS = f"https://github.com/{M.REPO}/releases/download/models-2026-10-16/"


def manifest():
    return {
        "schema": 1, "channel": "lamp-voice", "sequence": SEQUENCE, "commit": "a" * 40, "files_url": RELEASE,
        "files": [{"name": "lamp-update", "sha256": "1" * 64, "size": 10}],
        "units": [{"lamp": "lamp-4ace", "unit": "lamp-air.service", "sha256": "2" * 64, "size": 20}],
        "min_updater": 11,
        "models": [
            {"path": "/opt/lampOS/voice/current/a.onnx", "url": MODELS + "a.onnx", "bytes": 5, "sha256": "3" * 64},
            {"path": "/opt/lampOS/voice/current/lib.so", "url": MODELS + "pack.tgz", "bytes": 9, "sha256": "4" * 64,
             "archive": {"bytes": 3, "sha256": "5" * 64, "member": "pack/lib.so"}},
            {"path": "/opt/lampOS/voice/current/other.so", "url": MODELS + "pack.tgz", "bytes": 8, "sha256": "6" * 64,
             "archive": {"bytes": 3, "sha256": "5" * 64, "member": "pack/other.so"}},
        ],
        "settings": [{"path": "/etc/x", "url": "https://example.invalid/not-a-model"}],
    }


class FakeGcloud:
    """The bucket as a dict of object name to bytes; every command is recorded."""

    def __init__(self, objects=None):
        self.objects = dict(objects or {})
        self.calls = []
        self.cache = {}

    def __call__(self, words):
        self.calls.append(list(words))
        prefix = f"gs://{BUCKET}/"
        verb = words[0] if words[0] != "objects" else "describe"
        urls = [word for word in words if word.startswith(prefix)]
        name = urls[0][len(prefix):] if urls else None
        if verb == "describe":
            return (0, b"{}", b"") if name in self.objects else (1, b"", b"ERROR: HTTPError 404: No such object")
        if verb == "cp":
            if urls and words[-1] == urls[0]:
                self.cache[name] = [word for word in words if word.startswith("--cache-control=")]
                if "--no-clobber" in words and name in self.objects:
                    return 0, b"", b""
                self.objects[name] = Path(words[-2]).read_bytes()
                return 0, b"", b""
            if name not in self.objects:
                return 1, b"", b"ERROR: not found"
            Path(words[-1]).write_bytes(self.objects[name])
            return 0, b"", b""
        if verb == "rm":
            self.objects.pop(name, None)
            return 0, b"", b""
        return 1, b"", b"unexpected"

    def writes(self):
        return [call for call in self.calls if call[0] in ("cp", "rm") and call[-1].startswith("gs://")]


class GuardTests(unittest.TestCase):
    def refuses(self, destination, **kwargs):
        with self.assertRaises(M.Refused):
            M.guard(destination, BUCKET, **kwargs)

    def test_a_plain_path_under_the_mirror_folder_passes(self):
        for tail in ("lampos/stable/4242/files/lamp-update", "lampos/stable/lampos-manifest.json", "lampos/_probe/17.txt",
                     "lampos/stable/4242/models/models-2026-10-16/pack.tgz"):
            self.assertEqual(M.guard(f"gs://{BUCKET}/{tail}", BUCKET), f"gs://{BUCKET}/{tail}")

    def test_everything_else_is_refused(self):
        for tail in ("os/ota/metadata.json", "skills/x.zip", "lampos", "lampos/", "lampos//x", "lampos/../os/x", "lampos/stable/../../os/x",
                     "lampos/./x", "Lampos/x", "lampos-evil/x", "/lampos/x", "lampos/x y", "lampos/x?y", "lampos/*", "lampos/x\n", "lampos/.hidden",
                     "website/lampos/x", "lampos/a%2e%2e/b", "lampos/․/x"):
            with self.subTest(tail=tail):
                self.refuses(f"gs://{BUCKET}/{tail}")

    def test_another_bucket_or_scheme_is_refused(self):
        self.refuses("gs://other-bucket-1/lampos/x")
        self.refuses(f"gs://{BUCKET}x/lampos/x")
        self.refuses(f"https://{BUCKET}/lampos/x")
        self.refuses(f"file:///gs://{BUCKET}/lampos/x")
        self.refuses(None)

    def test_a_bad_bucket_name_is_refused(self):
        for bucket in ("", None, "a", "UPPER", "has space", "x/y"):
            with self.assertRaises(M.Refused):
                M.guard(f"gs://{bucket}/lampos/x", bucket)

    def test_the_skills_folder_passes_only_when_asked_for(self):
        self.refuses(f"gs://{BUCKET}/skills/a.zip")
        self.assertTrue(M.guard(f"gs://{BUCKET}/skills/a.zip", BUCKET, prefixes=("lampos", "skills")))

    def test_a_refused_path_never_reaches_gcloud(self):
        fake = FakeGcloud()
        store = M.Store(BUCKET, fake)
        with tempfile.TemporaryDirectory() as folder:
            local = Path(folder) / "f"
            local.write_bytes(b"x")
            for call in (lambda p: store.upload(local, p, M.MUTABLE_CACHE, True), store.exists, store.delete,
                         lambda p: store.download(p, local), lambda p: store.put_immutable(local, p, Path(folder))):
                for path in ("os/ota/metadata.json", "lampos/../os/x", "lampos"):
                    with self.assertRaises(M.Refused):
                        call(path)
        self.assertEqual(fake.calls, [])


class StoreTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.work = Path(self.folder.name)
        self.local = self.work / "f"
        self.local.write_bytes(b"firmware")
        self.path = "lampos/stable/4242/files/lamp-air"

    def tearDown(self):
        self.folder.cleanup()

    def test_a_new_object_is_uploaded_without_clobber_and_cached_for_good(self):
        fake = FakeGcloud()
        self.assertEqual(M.Store(BUCKET, fake).put_immutable(self.local, self.path, self.work), "uploaded")
        self.assertEqual(fake.objects[self.path], b"firmware")
        upload = fake.writes()[0]
        self.assertIn("--no-clobber", upload)
        self.assertIn(f"--cache-control={M.IMMUTABLE_CACHE}", upload)
        self.assertFalse(any("acl" in word for call in fake.calls for word in call))

    def test_an_existing_object_with_the_same_bytes_is_left_alone(self):
        fake = FakeGcloud({self.path: b"firmware"})
        self.assertEqual(M.Store(BUCKET, fake).put_immutable(self.local, self.path, self.work), "kept")
        self.assertEqual(fake.writes(), [])

    def test_an_existing_object_with_other_bytes_fails_and_is_not_overwritten(self):
        fake = FakeGcloud({self.path: b"other"})
        with self.assertRaises(M.Refused):
            M.Store(BUCKET, fake).put_immutable(self.local, self.path, self.work)
        self.assertEqual(fake.objects[self.path], b"other")
        self.assertEqual(fake.writes(), [])

    def test_a_gcloud_failure_is_not_taken_for_a_missing_object_and_hides_the_bucket(self):
        def broken(words):
            return 1, b"", f"ERROR: 403 denied on gs://{BUCKET}/x".encode()
        with self.assertRaises(M.Refused) as raised:
            M.Store(BUCKET, broken).exists(self.path)
        self.assertNotIn(BUCKET, str(raised.exception))

    def test_the_head_is_uploaded_to_be_asked_for_again(self):
        fake = FakeGcloud()
        M.Store(BUCKET, fake).upload(self.local, f"{M.STABLE}/{M.ENVELOPE}", M.MUTABLE_CACHE, clobber=True)
        self.assertIn("--cache-control=no-cache", fake.writes()[0])
        self.assertNotIn("--no-clobber", fake.writes()[0])


class RewriteTests(unittest.TestCase):
    def rewrite(self, value=None):
        raw = M.pretty(value or manifest())
        return raw, *M.rewrite_manifest(raw, SEQUENCE)

    def test_the_files_folder_and_every_model_move_to_the_cdn_and_nothing_else_changes(self):
        raw, new, moved = self.rewrite()
        base = f"https://cdn.autonomous.ai/lampos/stable/{SEQUENCE}/"
        value = json.loads(new)
        self.assertEqual(value["files_url"], base + "files/")
        self.assertEqual([model["url"] for model in value["models"]], [base + "models/models-2026-10-16/a.onnx"] + [base + "models/models-2026-10-16/pack.tgz"] * 2)
        self.assertEqual(M.without_urls(new), M.without_urls(raw))
        self.assertEqual(value["settings"], manifest()["settings"])
        for key in ("sequence", "commit", "channel", "files", "units", "min_updater"):
            self.assertEqual(value[key], manifest()[key])
        self.assertEqual(moved, {MODELS + "a.onnx": "models/models-2026-10-16/a.onnx", MODELS + "pack.tgz": "models/models-2026-10-16/pack.tgz"})
        self.assertTrue(new.endswith(b"\n"))

    def test_a_manifest_without_models_rewrites_the_folder_only(self):
        value = manifest()
        del value["models"]
        _, new, moved = self.rewrite(value)
        self.assertEqual(moved, {})
        self.assertNotIn(b"models", new)

    def test_refusals(self):
        def changed(edit):
            value = copy.deepcopy(manifest())
            edit(value)
            return M.pretty(value)
        cases = {
            "other sequence": changed(lambda m: m.update(sequence=SEQUENCE + 1)),
            "other channel": changed(lambda m: m.update(channel="other")),
            "files elsewhere": changed(lambda m: m.update(files_url="https://example.invalid/x/")),
            "files at another build": changed(lambda m: m.update(files_url=RELEASE.replace("4242", "4241"))),
            "model elsewhere": changed(lambda m: m["models"][0].update(url="https://example.invalid/a.onnx")),
            "model in another repository": changed(lambda m: m["models"][0].update(url=MODELS.replace("lampOS", "other") + "a.onnx")),
            "model traversal": changed(lambda m: m["models"][0].update(url=MODELS + "../a.onnx")),
            "model with a query": changed(lambda m: m["models"][0].update(url=MODELS + "a.onnx?x=1")),
            "compact layout": json.dumps(manifest()).encode(),
            "not json": b"{",
            "duplicate field": b'{"sequence": 1, "sequence": 1}',
        }
        for name, raw in cases.items():
            with self.subTest(name):
                with self.assertRaises(M.Refused):
                    M.rewrite_manifest(raw, SEQUENCE)

    def test_the_inventories_come_from_the_signed_manifest(self):
        value = manifest()
        self.assertEqual(M.expected_files(value), {"lamp-update": {"size": 10, "sha256": "1" * 64}, "lamp-4ace.lamp-air.service": {"size": 20, "sha256": "2" * 64}})
        _, _, moved = self.rewrite()
        models = M.expected_models(value, moved)
        self.assertEqual(models[MODELS + "a.onnx"], {"size": 5, "sha256": "3" * 64})
        self.assertEqual(models[MODELS + "pack.tgz"], {"size": 3, "sha256": "5" * 64})

    def test_one_model_url_with_two_contents_is_refused(self):
        value = manifest()
        value["models"][2]["archive"]["sha256"] = "7" * 64
        _, _, moved = self.rewrite(value)
        with self.assertRaises(M.Refused):
            M.expected_models(value, moved)


class RunTests(unittest.TestCase):
    ENV = {"LAMPOS_CDN_MIRROR": "on", "GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": M.REPO, "GITHUB_EVENT_NAME": "workflow_dispatch",
           "GITHUB_REF": "refs/heads/lamp-voice", "PROMOTE_SEQUENCE": str(SEQUENCE), M.SECRET: "not-a-key", "LAMPOS_UPDATE_PUBLIC_KEY": "AAAA"}

    def mirror(self, **changes):
        env = {**self.ENV, **changes}
        env = {key: value for key, value in env.items() if value is not None}
        return M.Mirror(env, tempfile.gettempdir(), store=M.Store(BUCKET, FakeGcloud()))

    def test_a_complete_context_passes(self):
        mirror = self.mirror()
        mirror.validate_context()
        self.assertEqual(mirror.sequence, SEQUENCE)

    def test_every_missing_piece_of_the_context_refuses(self):
        for change in ({"LAMPOS_CDN_MIRROR": "off"}, {"LAMPOS_CDN_MIRROR": None}, {"GITHUB_ACTIONS": "false"}, {"GITHUB_REPOSITORY": "x/y"},
                       {"GITHUB_EVENT_NAME": "pull_request"}, {"GITHUB_EVENT_NAME": "push"}, {"GITHUB_REF": "refs/heads/main"}, {"GITHUB_REF": "refs/tags/v1"},
                       {"PROMOTE_SEQUENCE": ""}, {"PROMOTE_SEQUENCE": "0"}, {"PROMOTE_SEQUENCE": "1x"}, {M.SECRET: None}, {"LAMPOS_UPDATE_PUBLIC_KEY": None}):
            with self.subTest(change=change):
                with self.assertRaises(M.Refused):
                    self.mirror(**change).validate_context()

    def test_no_bucket_refuses(self):
        for bucket in ("", None):
            with self.assertRaises(M.Refused):
                M.Store(bucket)

    def test_a_newer_stable_in_the_bucket_is_never_replaced(self):
        mirror = self.mirror()
        mirror.validate_context()
        for current in (None, SEQUENCE - 1, SEQUENCE):
            mirror.not_behind(current)
        with self.assertRaises(M.Refused):
            mirror.not_behind(SEQUENCE + 1)


class ProbeTests(unittest.TestCase):
    def test_it_writes_reads_back_and_deletes_only_its_own_file(self):
        fake = FakeGcloud()
        served = {}

        def download(url, path, most, sleep):
            self.assertEqual(url, "https://cdn.autonomous.ai/lampos/_probe/77.txt")
            Path(path).write_bytes(fake.objects["lampos/_probe/77.txt"])
        env = {"GITHUB_REPOSITORY": M.REPO, "GITHUB_REF": "refs/heads/lamp-voice", "GITHUB_RUN_ID": "77"}
        with tempfile.TemporaryDirectory() as folder:
            self.assertEqual(M.probe(env, folder, M.Store(BUCKET, fake), download=download, sleep=lambda _s: None), 0)
        self.assertEqual(fake.objects, {})
        self.assertEqual({call[-1] for call in fake.writes()}, {f"gs://{BUCKET}/lampos/_probe/77.txt"})

    def test_it_refuses_off_lamp_voice_and_for_a_bad_run_id(self):
        for env in ({"GITHUB_REPOSITORY": M.REPO, "GITHUB_REF": "refs/heads/main", "GITHUB_RUN_ID": "77"},
                    {"GITHUB_REPOSITORY": M.REPO, "GITHUB_REF": "refs/heads/lamp-voice", "GITHUB_RUN_ID": "../x"},
                    {"GITHUB_REPOSITORY": "x/y", "GITHUB_REF": "refs/heads/lamp-voice", "GITHUB_RUN_ID": "77"}):
            fake = FakeGcloud()
            with tempfile.TemporaryDirectory() as folder, self.assertRaises(M.Refused):
                M.probe(env, folder, M.Store(BUCKET, fake))
            self.assertEqual(fake.calls, [])

    def test_the_file_is_deleted_even_when_the_cdn_never_serves_it(self):
        fake = FakeGcloud()

        def download(url, path, most, sleep):
            raise M.Refused("download failed")
        env = {"GITHUB_REPOSITORY": M.REPO, "GITHUB_REF": "refs/heads/lamp-voice", "GITHUB_RUN_ID": "78"}
        with tempfile.TemporaryDirectory() as folder, self.assertRaises(M.Refused):
            M.probe(env, folder, M.Store(BUCKET, fake), download=download, sleep=lambda _s: None)
        self.assertEqual(fake.objects, {})


if __name__ == "__main__":
    unittest.main()
