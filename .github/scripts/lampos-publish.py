#!/usr/bin/env python3
"""Publish the tested lampOS artifact. Signing and verification stay in lamp-update."""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile

REPO = "autonomous-ai/lampOS"
CHANNEL = "lamp-voice"
HEAD = "lampos-lamp-voice"
ENVELOPE = "lampos-manifest.json"
ENABLE = "LAMPOS_UPDATE_PUBLISH"
BOOTSTRAP = "LAMPOS_UPDATE_BOOTSTRAP"
SECRET = "LAMPOS_UPDATE_KEY"
AUTH_TOKENS = frozenset({"GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"})
NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]*\Z")
BINARY = re.compile(r"lamp-[a-z0-9-]{1,39}\Z")
DIAGNOSTIC_LIMIT = 320
ANSI = re.compile(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|$))")
KEY_SHAPED = re.compile(
    r"eyJ[A-Za-z0-9_-]*(?:\.[A-Za-z0-9_-]*){2,4}|"
    r"github_pat_[A-Za-z0-9_]+|gh[pousr]_[A-Za-z0-9_]+|"
    r"(?:sk|pk|rk)[-_][A-Za-z0-9_-]{12,}|[A-Za-z0-9_+/=-]{32,}"
)


class Refused(Exception):
    pass


def require(ok, why):
    if not ok:
        raise Refused(why)


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest() if hasattr(hashlib, "file_digest") else _digest(source)


def _digest(source):
    hashed = hashlib.sha256()
    for block in iter(lambda: source.read(1024 * 1024), b""):
        hashed.update(block)
    return hashed.hexdigest()


def object_pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON field")
        result[key] = value
    return result


def decode_json(raw):
    try:
        return json.loads(raw, object_pairs_hook=object_pairs)
    except (ValueError, UnicodeError) as error:
        raise Refused("unreadable JSON") from error


def positive(value):
    return type(value) is int and 0 < value <= 2**64 - 1


def file_info(path):
    require(path.is_file() and not path.is_symlink(), "asset is not a regular file")
    return {"size": path.stat().st_size, "sha256": digest(path)}


def tree_info(root):
    result = {}
    for path in [root, *root.rglob("*")]:
        mode = path.lstat().st_mode
        info = {"mode": mode}
        if stat.S_ISLNK(mode):
            info["target"] = os.readlink(path)
        elif stat.S_ISREG(mode):
            info.update(file_info(path))
        else:
            require(stat.S_ISDIR(mode), "unexpected verification root entry")
        result[str(path.relative_to(root))] = info
    return result


class Commands:
    def __init__(self, environ):
        self.environ = {key: value for key, value in environ.items() if key != SECRET and key not in AUTH_TOKENS}
        self.gh_token = environ.get("GH_TOKEN")
        self.redactions = sorted({
            part for key, value in environ.items()
            if re.search(r"(?:^|_)(?:TOKEN|KEY|SECRET|PASSWORD|CREDENTIAL)(?:_|$)", key.upper())
            and isinstance(value, str)
            for part in value.splitlines() if part
        }, key=len, reverse=True)

    @staticmethod
    def name(words):
        program = Path(str(words[0])).name
        # Only fixed subcommand names are safe to echo, never arbitrary argv.
        if program == "gh" and len(words) > 1:
            if words[1] == "api":
                return "gh api"
            if len(words) > 2 and words[1] == "release" and words[2] in ("create", "upload"):
                return f"gh release {words[2]}"
        return program

    def diagnostic(self, stderr):
        lines = stderr.decode("utf-8", errors="replace").splitlines() if stderr else []
        if not lines:
            return "no stderr"
        line = "".join(char for char in ANSI.sub("", lines[0]) if char.isprintable())
        # Redact before truncating, including short configured secret values.
        for value in self.redactions:
            line = line.replace(value, "<redacted>")
        line = re.sub(r"(?i)(https?://)[^/\s@]+@", r"\1<redacted>@", line)
        line = re.sub(r"(?i)(https?://[^\s?]+)\?\S+", r"\1?<redacted>", line)
        line = re.sub(r"(?i)(\bauthorization\s*[:=]\s*).*", r"\1<redacted>", line)
        line = re.sub(r"(?i)(\b(?:bearer|basic)\s+)[^\s,;]+", r"\1<redacted>", line)
        line = re.sub(
            r"(?i)(\b(?:authorization|(?:access[_-]?)?token|(?:api[_-]?)?key|secret|password)\s*[:=]\s*)"
            r"(?:bearer\s+|token\s+)?(?:\"[^\"]*\"|'[^']*'|[^\s,;&]+)",
            r"\1<redacted>", line,
        )
        line = KEY_SHAPED.sub("<redacted>", line)
        return line[:DIAGNOSTIC_LIMIT] or "empty stderr line"

    def run(self, words, output=None):
        # Only gh receives release authorization; git and the verified updater
        # need neither that credential nor the signing key in their environment.
        environ = self.environ.copy()
        if Path(str(words[0])).name == "gh" and self.gh_token is not None:
            environ["GH_TOKEN"] = self.gh_token
        try:
            result = subprocess.run(
                [str(word) for word in words], env=environ, stdin=subprocess.DEVNULL,
                stdout=output if output else subprocess.PIPE, stderr=subprocess.PIPE,
                timeout=600, check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise Refused(f"{self.name(words)} did not complete") from error
        if result.returncode:
            message = f"{self.name(words)} failed (exit {result.returncode})"
            if Path(str(words[0])).name == "gh":
                message += f": {self.diagnostic(result.stderr)}"
                # Promotion recovery can wrap the exception; retain its cause now.
                print(message, file=sys.stderr)
            raise Refused(message)
        return result.stdout if output is None else b""


class Publisher:
    def __init__(self, artifact, source, environ, commands=None):
        self.artifact = Path(artifact).resolve()
        self.source = Path(source).resolve()
        self.env = dict(environ)
        self.commands = commands or Commands(environ)
        self.sequence = None
        self.sha = None
        self.updater = None
        self.public_key = None
        self.work = None

    def command(self, *words, output=None):
        return self.commands.run(words, output=output)

    def api(self, endpoint, method="GET", fields=None, pages=False):
        words = ["gh", "api", "--method", method]
        if pages:
            words += ["--paginate", "--slurp"]
        for key, value in (fields or {}).items():
            words += ["-f", f"{key}={value}"]
        words.append(f"repos/{REPO}/{endpoint}")
        result = decode_json(self.command(*words))
        if pages:
            require(isinstance(result, list) and all(isinstance(page, list) for page in result), "uncertain GitHub page inventory")
            return [entry for page in result for entry in page]
        return result

    def validate_context(self):
        require(self.env.get(ENABLE) == "on", "publication is disabled")
        require(self.env.get("GITHUB_ACTIONS") == "true", "not a GitHub Actions run")
        require(self.env.get("GITHUB_REPOSITORY") == REPO, "wrong repository")
        require(self.env.get("GITHUB_EVENT_NAME") == "push", "only a push artifact may publish")
        require(self.env.get("GITHUB_REF") == "refs/heads/lamp-voice", "wrong branch")
        self.sha = self.env.get("GITHUB_SHA", "")
        require(re.fullmatch(r"[0-9a-f]{40}", self.sha), "invalid checkout SHA")
        for name in ("GITHUB_RUN_NUMBER", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"):
            require(re.fullmatch(r"[1-9][0-9]*", self.env.get(name, "")), f"invalid {name}")
        self.sequence = int(self.env["GITHUB_RUN_NUMBER"])
        require(positive(self.sequence), "sequence is out of range")
        require(self.env.get(BOOTSTRAP, "") in ("", "off", "on"), "bootstrap must be on or off")
        authorization = self.env.get("GH_TOKEN")
        require(isinstance(authorization, str) and bool(authorization.strip()),
                "LAMPOS_RELEASE_TOKEN is absent from the protected publication environment")
        require(bool(self.env.get(SECRET)), "real signing key is absent")
        actual = self.command("git", "-C", self.source, "rev-parse", "HEAD").decode().strip()
        require(actual == self.sha, "checkout differs from this run's SHA")

    def artifact_inventory(self):
        require(self.artifact.is_dir(), "artifact folder is absent")
        found = {path.name: file_info(path) for path in self.artifact.iterdir()}
        require("SHA256SUMS" in found and "lamp-update" in found, "artifact lacks SHA256SUMS or lamp-update")
        sums = {}
        for line in (self.artifact / "SHA256SUMS").read_text().splitlines():
            match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9][A-Za-z0-9_.-]*)", line)
            require(match is not None, "malformed SHA256SUMS")
            hashed, name = match.groups()
            require(name not in sums and name != "SHA256SUMS", "duplicate or self-referential checksum")
            require(BINARY.fullmatch(name) or name in ("schedule_check", "replay_cost"), "unexpected artifact file")
            sums[name] = hashed
        require(set(sums) == set(found) - {"SHA256SUMS"}, "artifact checksum inventory differs")
        require(all(found[name]["sha256"] == hashed for name, hashed in sums.items()), "artifact checksum mismatch")
        return found

    def release_binaries(self, original):
        # One classification, shared with the lamp-os packager. Read the actual
        # JSON fields, not the older package check's grep over comments/names.
        path = self.source / "lampOS/deploy/package-binaries.json"
        require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 256 * 1024, "missing or unsafe package classification")
        classification = decode_json(path.read_bytes())
        groups = {"core", "optional", "provisioning", "development"}
        require(isinstance(classification, dict) and groups <= classification.keys()
                and classification.keys() <= groups | {"_comment"}, "unknown or missing package classification group")
        require("_comment" not in classification or isinstance(classification["_comment"], str), "invalid package classification comment")
        membership = {}
        for group in sorted(groups):
            names = classification[group]
            require(isinstance(names, list), "package classification group is not a list")
            for name in names:
                require(isinstance(name, str) and BINARY.fullmatch(name), "unsafe package binary name")
                require(name not in membership, "duplicate package binary classification")
                membership[name] = group
        binaries = {name for name in original if BINARY.fullmatch(name)}
        require(binaries <= membership.keys(), "unclassified artifact binary")
        # Absent optional tools need not be built. The CI exporter separately
        # requires every current Cargo binary; this contract classifies those
        # actually present, and keeps provisioning tools used beside Body.
        retained = {name: original[name] for name in sorted(binaries)
                    if membership[name] != "development"}
        require("lamp-update" in retained, "package classification excludes the updater")
        return retained

    def verified_manifest(self, envelope):
        require(envelope.is_file() and envelope.stat().st_size <= 512 * 1024, "missing or oversized envelope")
        raw = envelope.read_bytes()
        value = decode_json(raw)
        require(isinstance(value, dict) and isinstance(value.get("manifest"), str), "envelope has no manifest")
        try:
            payload = base64.b64decode(value["manifest"], validate=True)
        except ValueError as error:
            raise Refused("manifest is not base64") from error
        require(len(payload) <= 256 * 1024, "manifest is too large")
        manifest = decode_json(payload)
        require(isinstance(manifest, dict) and positive(manifest.get("sequence")), "invalid manifest sequence")
        sequence = manifest["sequence"]
        # The decoded sequence is only a hint for a disposable read-only root. check
        # verifies signature, schema and channel BEFORE this equality shortcut. It
        # then returns without examining units or fetching any manifest-named URL.
        with tempfile.TemporaryDirectory(prefix="verify-", dir=self.work) as folder:
            folder = Path(folder)
            channel = folder / "channel"
            channel.mkdir()
            (channel / ENVELOPE).write_bytes(raw)
            root = folder / "root"
            records = root / "var/lib/lampos-update"
            records.mkdir(parents=True)
            state = records / "state.json"
            state.write_text(json.dumps({"installed": {"sequence": sequence, "commit": "0" * 40}, "previous": None, "refused": [], "made": []}))
            before = tree_info(root)
            answer = self.command(
                self.updater, "check", "--channel-url", channel.as_uri() + "/", "--channel", CHANNEL,
                "--key-file", self.public_key, "--root", root, "--lamp", "publish-check", "--apply", "off",
            ).decode().strip()
            require(answer == f"up to date: build {sequence}", "updater did not verify the envelope")
            after = tree_info(root)
            require(before == after, "read-only verification changed its root")
        require(manifest.get("channel") == CHANNEL, "verified manifest has wrong channel")
        return manifest

    def prepare(self, retained):
        stage = self.work / "build"
        stage.mkdir()
        for name, expected in retained.items():
            shutil.copy2(self.artifact / name, stage / name)
            require(file_info(stage / name) == expected, "copied artifact checksum mismatch")
        # The original CI inventory remains intact, including excluded bytes.
        # This release checksum list covers exactly its retained binary subset.
        (stage / "SHA256SUMS").write_text("".join(
            f"{retained[name]['sha256']}  {name}\n" for name in sorted(retained)))
        copied = {**retained, "SHA256SUMS": file_info(stage / "SHA256SUMS")}
        self.updater = stage / "lamp-update"
        # Artifact upload/download does not preserve executable mode. The entire
        # CI checksum inventory and classification were verified before copying,
        # this chmod or exec. Only the private copy's executable mode changes.
        self.updater.chmod(0o700)
        tag = f"{HEAD}-{self.sequence}"
        url = f"https://github.com/{REPO}/releases/download/{tag}/"
        self.command(
            self.updater, "manifest", "--binaries", stage, "--deploy", self.source / "lampOS/deploy",
            "--skills", self.source / "lampOS/skills", "--channel", CHANNEL, "--sequence", str(self.sequence),
            "--commit", self.sha, "--files-url", url,
        )
        key = self.work / "signing.key"
        try:
            with open(key, "x", opener=lambda path, flags: os.open(path, flags, 0o600)) as output:
                output.write(self.env.pop(SECRET))
            public = self.command(self.updater, "public-key", "--key-file", key).strip()
            try:
                require(len(base64.b64decode(public, validate=True)) == 32, "invalid public key")
            except ValueError as error:
                raise Refused("invalid public key") from error
            self.public_key = self.work / "public.key"
            self.public_key.write_bytes(public + b"\n")
            self.command(self.updater, "sign", "--key-file", key, "--manifest", stage / "manifest.json", "--out", stage / ENVELOPE)
        finally:
            key.unlink(missing_ok=True)
        manifest = self.verified_manifest(stage / ENVELOPE)
        require(manifest.get("commit") == self.sha and manifest["sequence"] == self.sequence and manifest.get("files_url") == url, "manifest identity differs from tested build")
        require(base64.b64decode(decode_json((stage / ENVELOPE).read_bytes())["manifest"]) == (stage / "manifest.json").read_bytes(), "envelope and manifest bytes differ")
        for name, expected in copied.items():
            require(file_info(stage / name) == expected, "publisher changed an artifact file")
        files = manifest.get("files")
        units = manifest.get("units")
        require(isinstance(files, list) and isinstance(units, list), "manifest has no asset inventory")
        expected_names = retained.keys() | {"lamp-product-skills"}
        require({file.get("name") for file in files if isinstance(file, dict)} == expected_names, "manifest binary inventory differs")
        # Development binaries and CI examples enter neither the signed manifest
        # nor the immutable release; add only the generated signed assets.
        inventory = dict(copied)
        declared = set()
        for entry in files + units:
            require(isinstance(entry, dict), "invalid asset entry")
            name = entry.get("name") if "name" in entry else f"{entry.get('lamp')}.{entry.get('unit')}"
            require(isinstance(name, str) and NAME.fullmatch(name) and name not in declared, "duplicate or unsafe release asset")
            declared.add(name)
            info = file_info(stage / name)
            require(info == {"size": entry.get("size"), "sha256": entry.get("sha256")}, "manifest asset hash or size mismatch")
            inventory[name] = info
        for name in ("manifest.json", ENVELOPE):
            require(name not in inventory, "reserved release asset")
            inventory[name] = file_info(stage / name)
        return stage, inventory

    def releases(self):
        entries = self.api("releases?per_page=100", pages=True)
        require(all(isinstance(entry, dict) and positive(entry.get("id")) and isinstance(entry.get("tag_name"), str) for entry in entries), "uncertain release inventory")
        tags = [entry["tag_name"] for entry in entries]
        require(len(tags) == len(set(tags)), "duplicate release tag")
        return {entry["tag_name"]: entry for entry in entries}

    def tag_absent(self, tag):
        refs = self.api(f"git/matching-refs/tags/{tag}")
        require(isinstance(refs, list) and all(isinstance(ref, dict) and isinstance(ref.get("ref"), str) for ref in refs), "uncertain tag inventory")
        require(not any(ref["ref"] == f"refs/tags/{tag}" for ref in refs), "immutable tag already exists")

    def assets(self, release):
        require(positive(release.get("id")), "invalid release ID")
        entries = self.api(f"releases/{release['id']}/assets?per_page=100", pages=True)
        require(all(isinstance(asset, dict) and positive(asset.get("id")) and isinstance(asset.get("name"), str) and NAME.fullmatch(asset["name"]) and type(asset.get("size")) is int and asset["size"] >= 0 and asset.get("state") == "uploaded" for asset in entries), "uncertain release assets")
        require(len({asset["id"] for asset in entries}) == len(entries) and len({asset["name"] for asset in entries}) == len(entries), "duplicate release assets")
        return {asset["name"]: asset for asset in entries}

    def download(self, asset, path):
        with path.open("wb") as output:
            self.command("gh", "api", f"repos/{REPO}/releases/assets/{asset['id']}", "-H", "Accept: application/octet-stream", output=output)
        require(path.stat().st_size == asset["size"], "downloaded asset size differs")

    def head(self):
        release = self.releases().get(HEAD)
        if release is None:
            self.tag_absent(HEAD)
            return None
        require(release.get("draft") is False and release.get("prerelease") is True, "channel release is not a published prerelease")
        assets = self.assets(release)
        require(ENVELOPE in assets, "existing channel has no canonical envelope; owner repair required")
        asset = assets[ENVELOPE]
        require(asset["size"] <= 512 * 1024, "channel envelope is too large")
        path = self.work / "current-envelope.json"
        self.download(asset, path)
        manifest = self.verified_manifest(path)
        return {"release": release, "asset": asset, "sha256": digest(path), "sequence": manifest["sequence"]}

    @staticmethod
    def same_head(left, right):
        if left is None or right is None:
            return left is None and right is None
        return (left["release"]["id"], left["asset"]["id"], left["sha256"], left["sequence"]) == (right["release"]["id"], right["asset"]["id"], right["sha256"], right["sequence"])

    def create_release(self, tag, files):
        notes = self.work / "release-notes.txt"
        notes.write_text(f"Built and tested from {self.sha}; ARM64 workflow run {self.env['GITHUB_RUN_ID']}, attempt {self.env['GITHUB_RUN_ATTEMPT']}, sequence {self.sequence}.\n")
        self.command("gh", "release", "create", tag, "--repo", REPO, "--target", self.sha, "--title", f"lampOS {tag}", "--notes-file", notes, "--prerelease", "--latest=false", *files)

    def verify_assets(self, release, inventory):
        assets = self.assets(release)
        require(set(assets) == set(inventory), "uploaded asset inventory differs")
        for name, expected in inventory.items():
            require(assets[name]["size"] == expected["size"], "uploaded asset size differs")
            downloaded = self.work / "downloaded-asset"
            self.download(assets[name], downloaded)
            require(file_info(downloaded) == expected, "uploaded asset hash differs")

    def rename(self, asset, name):
        value = self.api(f"releases/assets/{asset['id']}", method="PATCH", fields={"name": name})
        require(isinstance(value, dict) and value.get("id") == asset["id"] and value.get("name") == name, "asset rename was not confirmed")

    def restore(self, release, previous, candidate, staged_name):
        assets = self.assets(release)
        canonical = assets.get(ENVELOPE)
        if canonical and previous and canonical["id"] == previous["asset"]["id"]:
            pass
        else:
            if canonical:
                require(canonical["id"] == candidate["id"], "canonical envelope changed outside this run")
                self.rename(candidate, staged_name)
            if previous:
                self.rename(previous["asset"], ENVELOPE)
        assets = self.assets(release)
        if previous:
            require(ENVELOPE in assets and assets[ENVELOPE]["id"] == previous["asset"]["id"], "previous envelope was not restored")
            path = self.work / "restored-envelope.json"
            self.download(assets[ENVELOPE], path)
            require(digest(path) == previous["sha256"], "restored envelope bytes differ")
        else:
            require(ENVELOPE not in assets, "initial channel still has an unexpected head")

    def promote(self, stage, previous):
        staged_name = f"lampos-manifest-staged-{self.sequence}.json"
        staged = self.work / staged_name
        shutil.copyfile(stage / ENVELOPE, staged)
        if previous:
            release = previous["release"]
            require(staged_name not in self.assets(release), "staged envelope already exists")
            self.command("gh", "release", "upload", HEAD, staged, "--repo", REPO)
        else:
            require(self.head() is None, "channel appeared before bootstrap")
            self.create_release(HEAD, [staged])
            release = self.releases().get(HEAD)
            require(release is not None, "bootstrap release was not confirmed")
        assets = self.assets(release)
        require(staged_name in assets, "staged envelope is absent")
        candidate = assets[staged_name]
        downloaded = self.work / "staged-envelope.json"
        self.download(candidate, downloaded)
        require(file_info(downloaded) == file_info(staged), "staged envelope bytes differ")
        if previous:
            require(self.same_head(previous, self.head()), "channel changed before promotion")
        else:
            require(ENVELOPE not in assets, "bootstrap channel unexpectedly has a head")
        try:
            if previous:
                backup = f"lampos-manifest-previous-{self.sequence}-{previous['asset']['id']}.json"
                require(backup not in assets, "previous-envelope backup already exists")
                self.rename(previous["asset"], backup)
            self.rename(candidate, ENVELOPE)
            current = self.head()
            require(current is not None and current["asset"]["id"] == candidate["id"] and current["sha256"] == digest(staged), "promoted head was not verified")
        except (Refused, OSError, UnicodeError, ValueError) as error:
            try:
                self.restore(release, previous, candidate, staged_name)
            except (Refused, OSError, UnicodeError, ValueError) as recovery:
                raise Refused(f"promotion failed; restoration also failed; owner must inspect channel release {release['id']} and retained assets {previous['asset']['id'] if previous else 'none'}/{candidate['id']}") from recovery
            raise Refused("promotion failed; previous canonical envelope restored (or initial head left absent)") from error

    def run(self):
        self.validate_context()
        original = self.artifact_inventory()
        retained = self.release_binaries(original)
        with tempfile.TemporaryDirectory(prefix="lampos-publish-") as folder:
            self.work = Path(folder)
            stage, inventory = self.prepare(retained)
            repo = decode_json(self.command("gh", "api", f"repos/{REPO}"))
            require(isinstance(repo, dict) and repo.get("full_name") == REPO and repo.get("private") is False, "wrong or inaccessible public repository")
            tag = f"{HEAD}-{self.sequence}"
            require(tag not in self.releases(), "immutable release already exists")
            self.tag_absent(tag)
            previous = self.head()
            if previous:
                require(self.sequence > previous["sequence"], "candidate sequence is stale or repeated")
            else:
                require(self.env.get(BOOTSTRAP) == "on", "initial channel requires LAMPOS_UPDATE_BOOTSTRAP=on")
            self.create_release(tag, [stage / name for name in sorted(inventory)])
            release = self.releases().get(tag)
            require(release is not None and release.get("target_commitish") == self.sha and release.get("draft") is False and release.get("prerelease") is True, "immutable release identity differs")
            self.verify_assets(release, inventory)
            require(self.same_head(previous, self.head()), "channel changed during build upload")
            self.promote(stage, previous)
            return {"published": tag, "commit": self.sha, "sequence": self.sequence, "run_id": self.env["GITHUB_RUN_ID"], "attempt": self.env["GITHUB_RUN_ATTEMPT"], "previous_asset": previous["asset"]["id"] if previous else None}


def main():
    def interrupted(signum, _frame):
        raise Refused(f"interrupted by signal {signum}")

    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", required=True, type=Path)
    parser.add_argument("--source", required=True, type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(Publisher(args.artifact, args.source, os.environ).run(), sort_keys=True))
    except (Refused, OSError, UnicodeError, ValueError) as error:
        print(f"lampOS publication refused: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
