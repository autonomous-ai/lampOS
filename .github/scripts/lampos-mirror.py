#!/usr/bin/env python3
"""Mirror the promoted stable lampOS release to the CDN, and probe the CDN path.

`mirror` takes the stable envelope the promotion just made, checks it with the
trusted public key, copies the build's files and models to
`lampos/stable/<sequence>/`, points the signed manifest at those copies,
signs it again with the same key (same sequence, commit and channel) and
puts it at `lampos/stable/lampos-manifest.json` last. `probe` writes, reads
back and deletes one small file under `lampos/_probe/`.

Every write goes through `guard`: a destination outside the `lampos/` folder
stops the step before any write. Verification and signing are the publisher's
(`lampos-publish.py`); this file adds no signing code of its own.
"""

import argparse
import base64
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

_SPEC = importlib.util.spec_from_file_location("publisher", Path(__file__).with_name("lampos-publish.py"))
P = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(P)
Refused = P.Refused
require = P.require

REPO = P.REPO
CHANNEL = P.CHANNEL
HEAD = P.HEAD
ENVELOPE = P.ENVELOPE
SECRET = P.SECRET
CDN = "https://cdn.autonomous.ai"
ROOT = "lampos"
STABLE = f"{ROOT}/stable"
PROBE = f"{ROOT}/_probe"
STABLE_URL = f"https://github.com/{REPO}/releases/download/lampos-stable/{ENVELOPE}"
BUCKET = re.compile(r"[a-z0-9][a-z0-9._-]{2,221}\Z")
SEGMENT = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_.-]*\Z")
MODEL_URL = re.compile(
    rf"https://github\.com/{re.escape(REPO)}/releases/download/"
    r"([A-Za-z0-9][A-Za-z0-9_.-]*)/([A-Za-z0-9][A-Za-z0-9_.-]*)\Z"
)
# A versioned object is never rewritten, so it may be cached for good; the
# head manifest is rewritten in place and must be asked for again every time.
IMMUTABLE_CACHE = "public, max-age=31536000, immutable"
MUTABLE_CACHE = "no-cache"
MOST_ENVELOPE = 512 * 1024
MOST_FILE = 2 * 1024 * 1024 * 1024
RETRY_PAUSES = (5, 15, 30)
CDN_PAUSES = (5, 15, 30)


def guard(destination, bucket, prefixes=(ROOT,)):
    """`destination`, when it is `gs://<bucket>/<prefix>/<plain path>` for one
    of `prefixes`; otherwise a refusal, before anything is written."""
    require(isinstance(bucket, str) and BUCKET.fullmatch(bucket), "mirror bucket is not set")
    require(isinstance(destination, str) and destination.startswith(f"gs://{bucket}/"), "destination is outside the mirror bucket")
    parts = destination[len(f"gs://{bucket}/"):].split("/")
    require(len(parts) >= 2 and parts[0] in prefixes, "destination is outside the mirror folder")
    require(all(SEGMENT.fullmatch(part) and part not in (".", "..") for part in parts), "destination is not a plain path")
    return destination


def pretty(manifest):
    """The bytes `lamp-update manifest` writes for a manifest."""
    return (json.dumps(manifest, indent=2, ensure_ascii=False) + "\n").encode()


def rewrite_manifest(raw, sequence):
    """The manifest `raw` with its files folder and its models' URLs on the CDN,
    and where each model is copied from: `(new bytes, {source URL: path})`,
    the path relative to `lampos/stable/<sequence>/`. Nothing else changes."""
    manifest = P.decode_json(raw)
    require(isinstance(manifest, dict) and manifest.get("sequence") == sequence, "manifest is not the selected sequence")
    require(manifest.get("channel") == CHANNEL, "manifest has the wrong channel")
    require(pretty(manifest) == raw, "manifest bytes are not in the generator's layout")
    source = f"https://github.com/{REPO}/releases/download/{HEAD}-{sequence}/"
    require(manifest.get("files_url") == source, "manifest files are not at the build's own release")
    base = f"{CDN}/{STABLE}/{sequence}/"
    manifest["files_url"] = base + "files/"
    models = manifest.get("models", [])
    require(isinstance(models, list), "manifest models are not a list")
    moved = {}
    for model in models:
        require(isinstance(model, dict) and isinstance(model.get("url"), str), "invalid model entry")
        match = MODEL_URL.fullmatch(model["url"])
        require(match is not None, "a model is not at a release of this repository")
        relative = f"models/{match[1]}/{match[2]}"
        moved[model["url"]] = relative
        model["url"] = base + relative
    return pretty(manifest), moved


def without_urls(raw):
    manifest = P.decode_json(raw)
    manifest["files_url"] = ""
    for model in manifest.get("models", []):
        model["url"] = ""
    return manifest


def expected_files(manifest):
    """Every file of the build's folder with the size and hash the manifest signs."""
    found = {}
    for entry in manifest["files"] + manifest["units"]:
        name = entry["name"] if "name" in entry else f"{entry['lamp']}.{entry['unit']}"
        require(isinstance(name, str) and P.NAME.fullmatch(name) and name not in found, "duplicate or unsafe build file")
        found[name] = {"size": entry["size"], "sha256": entry["sha256"]}
    return found


def expected_models(manifest, moved):
    """Each model URL once, with what fetching it returns: the model's own size
    and hash, or its archive's."""
    found = {}
    for model in manifest.get("models", []):
        archive = model.get("archive")
        info = {"size": archive["bytes"] if archive else model["bytes"], "sha256": archive["sha256"] if archive else model["sha256"]}
        require(found.setdefault(model["url"], info) == info, "one model URL is signed with two different contents")
    require(set(found) == set(moved), "model inventory differs")
    return found


def fetch(url, path, most, sleep=time.sleep):
    """`url` into `path`, at most `most` bytes; a read is repeated a few times."""
    require(url.startswith("https://"), "not an https URL")
    request = urllib.request.Request(url, headers={"User-Agent": "lampos-mirror"})
    for pause in (*RETRY_PAUSES, None):
        try:
            total = 0
            with urllib.request.urlopen(request, timeout=120) as answer, open(path, "wb") as output:
                for block in iter(lambda: answer.read(1024 * 1024), b""):
                    total += len(block)
                    require(total <= most, "download is larger than allowed")
                    output.write(block)
            return
        except (urllib.error.URLError, OSError, TimeoutError) as error:
            if pause is None:
                raise Refused(f"download failed: {type(error).__name__}") from error
            print(f"download failed ({type(error).__name__}); again in {pause} s", file=sys.stderr)
            sleep(pause)


def run_gcloud(words):
    try:
        done = subprocess.run(
            ["gcloud", "storage", "--no-user-output-enabled", *words],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=3600, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise Refused("gcloud did not complete") from error
    return done.returncode, done.stdout, done.stderr


class Store:
    """The bucket, reached only through `guard`."""

    def __init__(self, bucket, run=run_gcloud):
        require(isinstance(bucket, str) and BUCKET.fullmatch(bucket), "mirror bucket is not set")
        self.bucket = bucket
        self.run = run

    def target(self, path):
        return guard(f"gs://{self.bucket}/{path}", self.bucket)

    def call(self, *words, path, missing_ok=False):
        # The guard is checked before the command is built, so a refused path
        # never reaches gcloud.
        target = self.target(path)
        code, out, err = self.run([target if word == "{target}" else word for word in words])
        if code == 0:
            return out
        text = err.decode("utf-8", errors="replace")
        if missing_ok and ("404" in text or "No such object" in text or "matched no objects" in text):
            return None
        line = "".join(char for char in (text.splitlines() or ["no stderr"])[0] if char.isprintable())
        line = line.replace(self.bucket, "<bucket>")[:200]
        raise Refused(f"gcloud storage {words[0]} failed (exit {code}): {line}")

    def exists(self, path):
        return self.call("objects", "describe", "{target}", path=path, missing_ok=True) is not None

    def download(self, path, local):
        self.call("cp", "{target}", str(local), path=path)

    def upload(self, local, path, cache, clobber):
        words = ["cp", f"--cache-control={cache}"]
        if not clobber:
            words.append("--no-clobber")
        self.call(*words, str(local), "{target}", path=path)

    def delete(self, path):
        self.call("rm", "{target}", path=path)

    def put_immutable(self, local, path, work):
        """`local` at `path` unless an object is there already: the same bytes
        are left alone, different ones are a failure. Never an overwrite."""
        info = P.file_info(local)
        if self.exists(path):
            held = work / "held-object"
            self.download(path, held)
            require(P.file_info(held) == info, "an object already at an immutable path differs")
            held.unlink()
            return "kept"
        self.upload(local, path, IMMUTABLE_CACHE, clobber=False)
        return "uploaded"


def cdn_url(path):
    return f"{CDN}/{path}"


class Mirror:
    def __init__(self, environ, work, store=None, commands=None, download=fetch, sleep=time.sleep):
        self.env = dict(environ)
        self.work = Path(work)
        self.store = store
        self.commands = commands or P.Commands(environ)
        self.download = download
        self.sleep = sleep
        self.sequence = None
        self.updater = None
        self.public_key = None

    def validate_context(self):
        require(self.env.get("LAMPOS_CDN_MIRROR") == "on", "the CDN mirror is off")
        require(self.env.get("GITHUB_ACTIONS") == "true", "not a GitHub Actions run")
        require(self.env.get("GITHUB_REPOSITORY") == REPO, "wrong repository")
        require(self.env.get("GITHUB_EVENT_NAME") == "workflow_dispatch", "only a manual promotion run may mirror")
        require(self.env.get("GITHUB_REF") == "refs/heads/lamp-voice", "wrong branch")
        require(re.fullmatch(r"[1-9][0-9]*", self.env.get("PROMOTE_SEQUENCE", "")), "invalid promoted sequence")
        self.sequence = int(self.env["PROMOTE_SEQUENCE"])
        require(P.positive(self.sequence), "sequence is out of range")
        require(bool(self.env.get(SECRET)), "real signing key is absent")
        require(bool(self.env.get("LAMPOS_UPDATE_PUBLIC_KEY", "").strip()), "trusted public key is absent")
        require(self.store is not None, "no mirror bucket")

    def command(self, *words):
        return self.commands.run(words)

    def get_updater(self, manifest):
        """The build's own `lamp-update`, from its immutable release. The
        promotion has just checked the envelope against the trusted key and
        every file against it; the updater below checks the signature again."""
        entry = next((file for file in manifest["files"] if file["name"] == "lamp-update"), None)
        require(entry is not None, "the build has no updater")
        updater = self.work / "lamp-update"
        self.download(manifest["files_url"] + "lamp-update", updater, entry["size"], self.sleep)
        require(P.file_info(updater) == {"size": entry["size"], "sha256": entry["sha256"]}, "updater differs from the signed manifest")
        updater.chmod(0o700)
        self.updater = updater
        self.public_key = self.work / "public.key"
        self.public_key.write_text(self.env["LAMPOS_UPDATE_PUBLIC_KEY"].strip() + "\n")

    def publisher(self):
        """The publisher's own envelope check, set up on this updater and key."""
        publisher = P.Publisher(self.work, self.work, {}, self.commands)
        publisher.work = self.work
        publisher.updater = self.updater
        publisher.public_key = self.public_key
        return publisher

    def sign(self, manifest, out):
        key = self.work / "signing.key"
        try:
            with open(key, "x", opener=lambda path, flags: os.open(path, flags, 0o600)) as output:
                output.write(self.env[SECRET])
            derived = self.command(self.updater, "public-key", "--key-file", key).strip()
            require(derived == self.public_key.read_bytes().strip(), "signing key is not the trusted key")
            self.command(self.updater, "sign", "--key-file", key, "--manifest", manifest, "--out", out)
        finally:
            key.unlink(missing_ok=True)

    def check_url(self, channel_url, sequence):
        """`lamp-update check` against a channel folder, as a lamp already on
        the build would run it: the signature and schema are read, nothing
        applied."""
        with tempfile.TemporaryDirectory(prefix="check-", dir=self.work) as folder:
            records = Path(folder) / "root/var/lib/lampos-update"
            records.mkdir(parents=True)
            (records / "state.json").write_text(json.dumps({"installed": {"sequence": sequence, "commit": "0" * 40}, "previous": None, "refused": [], "made": []}))
            answer = self.command(
                self.updater, "check", "--channel-url", channel_url, "--channel", CHANNEL, "--key-file", self.public_key,
                "--root", Path(folder) / "root", "--lamp", "mirror-check", "--apply", "off",
            ).decode().strip()
        require(answer == f"up to date: build {sequence}", "updater did not verify the envelope")

    def head_sequence(self):
        """The sequence of the head manifest in the bucket now, or None."""
        path = f"{STABLE}/{ENVELOPE}"
        if not self.store.exists(path):
            return None
        held = self.work / "current-head.json"
        self.store.download(path, held)
        return self.publisher().verified_manifest(held)["sequence"]

    def not_behind(self, current):
        """The head in the bucket may only be replaced by the same sequence or a later one."""
        require(current is None or current <= self.sequence, "the CDN already has a newer stable")

    def through_cdn(self, path, expected, most):
        """`path` as the CDN serves it, checked against `expected`, asked again
        after a pause while it differs (a cache, or the copy not yet there)."""
        local = self.work / "cdn-object"
        problem = "differs"
        for pause in (*CDN_PAUSES, None):
            try:
                self.download(cdn_url(path), local, most, lambda _seconds: None)
                if P.file_info(local) == expected:
                    local.unlink()
                    return
            except Refused as error:
                problem = str(error)
            if pause is not None:
                self.sleep(pause)
        raise Refused(f"the CDN does not serve {path.rsplit('/', 1)[-1]} as mirrored: {problem}")

    def run(self):
        self.validate_context()
        stable = self.work / "stable-envelope.json"
        self.download(STABLE_URL, stable, MOST_ENVELOPE, self.sleep)
        raw = stable.read_bytes()
        value = P.decode_json(raw)
        require(isinstance(value, dict) and isinstance(value.get("manifest"), str), "envelope has no manifest")
        try:
            original = P.decode_json(base64.b64decode(value["manifest"], validate=True))
        except ValueError as error:
            raise Refused("manifest is not base64") from error
        require(isinstance(original, dict) and original.get("sequence") == self.sequence, "stable is not the promoted sequence")
        for name in ("files", "units"):
            require(isinstance(original.get(name), list) and all(isinstance(entry, dict) for entry in original[name]), "manifest has no asset inventory")
        require(isinstance(original.get("files_url"), str), "manifest has no files folder")
        self.get_updater(original)
        manifest = self.publisher().verified_manifest(stable)
        require(manifest == original, "verified manifest differs from the one read")
        plain = base64.b64decode(value["manifest"], validate=True)
        rewritten, moved = rewrite_manifest(plain, self.sequence)
        files = expected_files(manifest)
        models = expected_models(manifest, moved)

        # Everything the new manifest names, fetched from the build's own
        # release and checked against what was signed.
        stage = self.work / "stage"
        stage.mkdir()
        for name, info in sorted(files.items()):
            self.download(manifest["files_url"] + name, stage / name, info["size"], self.sleep)
            require(P.file_info(stage / name) == info, f"{name} differs from the signed manifest")
        modelstage = self.work / "models"
        modelstage.mkdir()
        sources = {}
        for number, (url, info) in enumerate(sorted(models.items())):
            local = modelstage / str(number)
            self.download(url, local, info["size"], self.sleep)
            require(P.file_info(local) == info, "a model differs from the signed manifest")
            sources[url] = local

        candidate = self.work / "candidate-manifest.json"
        candidate.write_bytes(rewritten)
        envelope = self.work / ENVELOPE
        self.sign(candidate, envelope)
        # The same key signed the same build: only the URLs moved.
        new = self.publisher().verified_manifest(envelope)
        require(without_urls(rewritten) == without_urls(plain), "rewrite changed more than the URLs")
        require(new["sequence"] == self.sequence and new["commit"] == manifest["commit"] and new["channel"] == CHANNEL, "re-signed manifest identity differs")
        require(base64.b64decode(P.decode_json(envelope.read_bytes())["manifest"]) == rewritten, "envelope and manifest bytes differ")

        self.not_behind(self.head_sequence())

        folder = f"{STABLE}/{self.sequence}"
        uploaded = kept = 0
        copies = [(stage / name, f"{folder}/files/{name}", info) for name, info in sorted(files.items())]
        copies += [(sources[url], f"{folder}/{moved[url]}", info) for url, info in sorted(models.items())]
        for local, path, info in copies:
            outcome = self.store.put_immutable(local, path, self.work)
            uploaded += outcome == "uploaded"
            kept += outcome == "kept"
        for local, path, info in copies:
            self.through_cdn(path, info, info["size"])

        # The head last, asked for again every time it is read.
        self.store.upload(envelope, f"{STABLE}/{ENVELOPE}", MUTABLE_CACHE, clobber=True)
        self.through_cdn(f"{STABLE}/{ENVELOPE}", P.file_info(envelope), MOST_ENVELOPE)
        self.check_url(f"{CDN}/{STABLE}/", self.sequence)
        return {"mirrored": self.sequence, "commit": manifest["commit"], "files": len(files), "models": len(models), "uploaded": uploaded, "kept": kept}


def probe(environ, work, store, download=fetch, sleep=time.sleep):
    """Write, read back (gcloud and the CDN) and delete one file under `lampos/_probe/`."""
    require(environ.get("GITHUB_REPOSITORY") == REPO and environ.get("GITHUB_REF") == "refs/heads/lamp-voice", "the probe runs from lamp-voice only")
    run_id = environ.get("GITHUB_RUN_ID", "")
    require(re.fullmatch(r"[1-9][0-9]*", run_id), "invalid run id")
    path = f"{PROBE}/{run_id}.txt"
    local = Path(work) / "probe.txt"
    local.write_text(f"probe {run_id}\n")
    store.upload(local, path, MUTABLE_CACHE, clobber=True)
    try:
        back = Path(work) / "probe-back.txt"
        store.download(path, back)
        require(back.read_bytes() == local.read_bytes(), "gcloud read-back differs")
        print("probe: read back through gcloud")
        info = P.file_info(local)
        for pause in (*CDN_PAUSES, None):
            try:
                download(cdn_url(path), Path(work) / "probe-cdn.txt", 1024, lambda _seconds: None)
                if P.file_info(Path(work) / "probe-cdn.txt") == info:
                    break
            except Refused:
                pass
            if pause is None:
                raise Refused("the CDN does not serve the probe file")
            sleep(pause)
        print("probe: read back through the CDN")
    finally:
        store.delete(path)
    require(not store.exists(path), "probe file was not deleted")
    print("probe: deleted")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("mirror", "probe"))
    args = parser.parse_args()
    try:
        with tempfile.TemporaryDirectory(prefix="lampos-mirror-") as folder:
            store = Store(os.environ.get("LAMPOS_MIRROR_BUCKET", ""))
            if args.command == "probe":
                return probe(os.environ, folder, store)
            print(json.dumps(Mirror(os.environ, folder, store).run(), sort_keys=True))
    except (Refused, OSError, UnicodeError, ValueError, KeyError, TypeError) as error:
        print(f"lampOS CDN mirror refused: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
