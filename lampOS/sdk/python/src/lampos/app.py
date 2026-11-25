"""`lamp app check` and `lamp app pack`: an app (format 2, docs/apps.md)
checked and packed on this computer, before it goes to the Store.

An app's folder, as a developer keeps it:

    gym-coach/
      app.json          the manifest without format, kind and files
      src/gym_coach/app.py
      assets/ok.wav
      skills/gym-tips/SKILL.md, skill.toml, ...

`app.json` holds the manifest's own fields (name, title, version,
min_build, runtime, grants, net, logs, npu, phrases, triggers, settings,
skills, assets). `name` may be left out (the folder's), and so may `skills`
and `assets` (the folders under skills/ and the WAV files under assets/).
`pack` adds `format`, `kind` and `files` and writes the tar the Store
takes: `lamp-skill.json` first, then every file as `NAME/path`, sorted,
byte for byte as `lamp skill push` packs a skill. It is unsigned: the Store
signs it at approval (`lamp-skill.sig`).

The checks are lamp-skills' `app_package` reader's (crates/skills/src/
app_package.rs), less the signature, and each companion skill is checked
as `lamp skill check` checks a folder. The Store runs the same reader and
says its own reason for anything this misses.
"""

from __future__ import annotations

import hashlib
import io
import json
import math
import re
import struct
import tarfile
import tempfile
import wave
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .errors import LampError
from .push import MANIFEST, _entry
from .skillcheck import Report
from .skillcheck import check as check_skill

SOURCE = "app.json"
SIGNATURE = "lamp-skill.sig"
FORMAT = 2
KIND = "app"
MOST_FILES = 512
MOST_BYTES = 16 * 1024 * 1024
MOST_PACKAGE_BYTES = MOST_BYTES + 2 * 1024 * 1024
MOST_DEPTH = 8
MOST_PATH_BYTES = 1024
GRANTS = (
    "body", "light", "speak", "look", "hear", "sound", "skills", "air", "device",
    "camera", "microphone", "speaker", "notes", "speech", "volume", "remember",
)
NPU_OPS = {"pose": "camera"}
PHRASE_BYTES = 64
PHRASE_LETTERS = 8
MOST_PHRASES = 16
MOST_TRIGGERS = 8
MOST_SETTINGS = 16
MOST_ASSETS = 20
ASSET_RATE = 24_000
ASSET_MOST_FRAMES = 240_000
# lamp-voice's `decode_sound_with_limit`: no sample past -12 dBFS, no 10 ms louder than -30 dBFS RMS.
ASSET_PEAK_DBFS = -12.0
ASSET_LOUDEST_DBFS = -30.0
ASSET_WINDOW = 240
# The manifest's fields, in the order `pack` writes them.
FIELDS = (
    "format", "kind", "name", "title", "version", "min_build", "runtime", "grants", "net", "logs",
    "npu", "phrases", "triggers", "settings", "skills", "assets", "files",
)
REQUIRED = ("format", "kind", "name", "title", "version", "min_build", "runtime", "grants", "triggers", "files")
TOPS = ("src", "assets", "skills")

_APP_NAME = re.compile(r"^[a-z][a-z0-9-]{0,23}$")
_SKILL_NAME = re.compile(r"^[a-z0-9-]{1,64}$")
_KEY = re.compile(r"^[a-z0-9_]{1,32}$")
_CHOICE = re.compile(r"^[a-z0-9][a-z0-9_-]{0,31}$")
_ENTRY_PART = re.compile(r"^[a-z_][a-z0-9_]*$")
_PART = re.compile(r"^[A-Za-z0-9._-]{1,100}$")
_VERSION_PART = re.compile(r"^(0|[1-9][0-9]{0,8})$")


@dataclass
class AppPackage:
    """A packed app: its name, version, manifest and the tar's bytes."""

    name: str
    version: str
    manifest: dict[str, Any]
    data: bytes

    @property
    def sha256(self) -> str:
        return hashlib.sha256(self.data).hexdigest()


def _plain(text: object, most: int) -> bool:
    return (
        isinstance(text, str)
        and 1 <= len(text) <= most
        and not any(ord(c) < 32 or 127 <= ord(c) < 160 for c in text)
        and text.strip() == text
    )


def _version_ok(version: object) -> bool:
    return isinstance(version, str) and len(parts := version.split(".")) == 3 and all(_VERSION_PART.match(p) for p in parts)


def _number(value: object) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _phrases(what: str, phrases: object, most: int) -> list[str]:
    if not isinstance(phrases, list) or not all(isinstance(p, str) for p in phrases):
        return [f"{what}: a list of text"]
    if len(phrases) > most:
        return [f"{what}: at most {most}"]
    errors, seen = [], set()
    for phrase in phrases:
        if len(phrase.encode()) > PHRASE_BYTES or any(ord(c) < 32 for c in phrase) or sum(c.isalpha() for c in phrase) < PHRASE_LETTERS:
            errors.append(f"{what}: {phrase!r} is more than {PHRASE_BYTES} bytes, or has fewer than {PHRASE_LETTERS} letters")
        elif phrase.lower() in seen:
            errors.append(f"{what}: {phrase!r} twice")
        seen.add(phrase.lower())
    return errors


def _setting(setting: object) -> str | None:
    if not isinstance(setting, dict):
        return "a setting is an object"
    known = {"key", "type", "label", "hint", "default", "max", "min", "unit", "options"}
    if extra := sorted(set(setting) - known):
        return f"setting {setting.get('key')!r}: unknown field {extra[0]!r}"
    key, kind = setting.get("key"), setting.get("type")
    at = f"setting {key!r}: "
    if not isinstance(key, str) or not _KEY.match(key):
        return at + "a key is 1 to 32 of a-z, 0-9 and _"
    if not _plain(setting.get("label"), 40):
        return at + "a label is 1 to 40 characters on one line"
    if "hint" in setting and not _plain(setting["hint"], 120):
        return at + "a hint is 1 to 120 characters on one line"
    if "default" not in setting:
        return at + "a default is required"
    lo, hi, unit, options, default = (setting.get(k) for k in ("min", "max", "unit", "options", "default"))
    if kind == "text":
        if not _number(hi) or hi != int(hi) or not 1 <= hi <= 200:
            return at + "a text setting's max is a whole number, 1 to 200"
        if lo is not None or unit is not None or options is not None:
            return at + "a text setting has no min, unit or options"
        if not isinstance(default, str) or len(default) > hi or any(ord(c) < 32 for c in default):
            return at + "the default is text no longer than max, with no control character"
    elif kind == "number":
        if not _number(lo) or not _number(hi) or not (math.isfinite(lo) and math.isfinite(hi)) or lo >= hi:
            return at + "a number setting needs min below max, both finite"
        if options is not None:
            return at + "a number setting has no options"
        if unit is not None and not _plain(unit, 16):
            return at + "a unit is 1 to 16 characters"
        if not _number(default) or not lo <= default <= hi:
            return at + "the default is a number from min to max"
    elif kind == "toggle":
        if any(v is not None for v in (lo, hi, unit, options)):
            return at + "a toggle has only key, label, hint and default"
        if not isinstance(default, bool):
            return at + "the default of a toggle is true or false"
    elif kind == "choice":
        if any(v is not None for v in (lo, hi, unit)):
            return at + "a choice has options, no min, max or unit"
        if not isinstance(options, list) or not 2 <= len(options) <= 16:
            return at + "a choice has 2 to 16 options"
        values = set()
        for option in options:
            if (
                not isinstance(option, dict)
                or set(option) != {"value", "label"}
                or not isinstance(option["value"], str)
                or not _CHOICE.match(option["value"])
                or not _plain(option["label"], 40)
            ):
                return at + "an option is {value, label}: value [a-z0-9][a-z0-9_-] up to 32, label 1 to 40 characters"
            if option["value"] in values:
                return at + "an option's value twice"
            values.add(option["value"])
        if default not in values:
            return at + "the default is one of the options' values"
    else:
        return at + "type is text, number, toggle or choice"
    return None


def manifest_errors(manifest: object) -> list[str]:
    """The manifest's own rules, as `app_package::check_manifest` holds them
    (no files): each line is one refusal."""
    if not isinstance(manifest, dict):
        return [f"{MANIFEST}: not a JSON object"]
    errors: list[str] = []
    say = lambda why: errors.append(f"{MANIFEST}: {why}")  # noqa: E731
    for extra in sorted(set(manifest) - set(FIELDS)):
        say(f"unknown field {extra!r}")
    for missing in REQUIRED:
        if missing not in manifest:
            say(f"no {missing!r}")
    if errors:
        return errors
    if manifest["format"] != FORMAT:
        say(f"format {manifest['format']} is not an app's ({FORMAT})")
    if manifest["kind"] != KIND:
        say(f'kind is "{KIND}" for an app')
    name = manifest["name"]
    if not isinstance(name, str) or not _APP_NAME.match(name):
        say(f"{name!r} is not an app's name: 1 to 24 lowercase letters, digits and hyphens, a letter first")
    if not _plain(manifest["title"], 40):
        say("a title is 1 to 40 characters on one line")
    if not _version_ok(manifest["version"]):
        say(f"version {manifest['version']!r} is not MAJOR.MINOR.PATCH")
    build = manifest["min_build"]
    if not isinstance(build, int) or isinstance(build, bool) or build < 1:
        say("min_build is the lamp build's sequence, 1 or more")
    runtime = manifest["runtime"]
    if not isinstance(runtime, dict) or set(runtime) != {"name", "entry"}:
        say("runtime is {name, entry}")
    else:
        if runtime["name"] != "python3":
            say(f"runtime.name {runtime['name']!r} is not one this lamp runs (python3)")
        entry = runtime["entry"]
        if not isinstance(entry, str) or not 0 < len(entry) <= 100 or not all(_ENTRY_PART.match(p) for p in entry.split(".")):
            say(f"runtime.entry {entry!r} is not a dotted module path (a.b.c)")
    grants = manifest["grants"]
    held: set[str] = set()
    if not isinstance(grants, list):
        say("grants is a list")
    else:
        for grant in grants:
            if grant not in GRANTS:
                say(f"grant {grant!r} is not one of the door's: {', '.join(GRANTS)}")
            elif grant in held:
                say(f"grant {grant!r} twice")
            held.add(grant)
    for flag in ("net", "logs"):
        if flag in manifest and not isinstance(manifest[flag], bool):
            say(f"{flag} is true or false")
    ops: set[str] = set()
    for op in manifest.get("npu", []) if isinstance(manifest.get("npu", []), list) else [None]:
        if op not in NPU_OPS:
            say(f"npu op {op!r} is not one of the lamp's: {', '.join(NPU_OPS)}")
        elif op in ops:
            say(f"npu op {op!r} twice")
        elif NPU_OPS[op] not in held:
            say(f"npu op {op!r} needs the {NPU_OPS[op]!r} grant")
        ops.add(op)
    phrases, triggers = manifest.get("phrases", []), manifest["triggers"]
    errors.extend(f"{MANIFEST}: {why}" for why in _phrases("phrases", phrases, MOST_PHRASES))
    errors.extend(f"{MANIFEST}: {why}" for why in _phrases("triggers", triggers, MOST_TRIGGERS))
    if (phrases or triggers) and "hear" not in held:
        say("phrases and triggers need the hear grant")
    settings = manifest.get("settings", [])
    if not isinstance(settings, list) or len(settings) > MOST_SETTINGS:
        say(f"settings: a list of at most {MOST_SETTINGS}")
    else:
        keys: set[object] = set()
        for setting in settings:
            if why := _setting(setting):
                say(why)
            elif setting["key"] in keys:
                say(f"setting {setting['key']!r} twice")
            else:
                keys.add(setting["key"])
    skills = manifest.get("skills", [])
    if not isinstance(skills, list) or len(set(map(str, skills))) != len(skills) or not all(isinstance(s, str) and _SKILL_NAME.match(s) for s in skills):
        say("skills: skill names (lowercase letters, digits and hyphens), none twice")
    assets = manifest.get("assets", [])
    if not isinstance(assets, list) or len(assets) > MOST_ASSETS:
        say(f"assets: a list of at most {MOST_ASSETS}")
    else:
        for asset in assets:
            if not isinstance(asset, str) or not asset.startswith("assets/") or not asset.endswith(".wav"):
                say(f"asset {asset!r} is under assets/ and ends .wav")
        if len(set(map(str, assets))) != len(assets):
            say("an asset twice")
    if not isinstance(manifest["files"], list):
        say("files is a list")
    return errors


def path_errors(path: str) -> str | None:
    """A file's path inside the app's folder: 2 to 8 parts of
    [A-Za-z0-9._-], none beginning with a dot, under src/, assets/ or skills/."""
    parts = path.split("/")
    if len(path.encode()) > MOST_PATH_BYTES or len(parts) > MOST_DEPTH:
        return f"{path}: more than {MOST_DEPTH} parts or {MOST_PATH_BYTES} bytes"
    if not all(_PART.match(p) and not p.startswith(".") for p in parts):
        return f"{path}: a path's parts are letters, digits, -, _ and ., none beginning with a dot"
    if parts[0] not in TOPS or len(parts) < 2:
        return f"{path}: an app's files are under src/, assets/ and skills/"
    return None


def asset_error(data: bytes) -> str | None:
    """A WAV as lamp-voice's `decode_sound_with_limit` takes it."""
    try:
        with wave.open(io.BytesIO(data), "rb") as clip:
            shape = (clip.getsampwidth(), clip.getnchannels(), clip.getframerate())
            frames = clip.getnframes()
            pcm = clip.readframes(frames)
    except (wave.Error, EOFError, OSError):
        return "not a WAV file"
    if shape != (2, 1, ASSET_RATE):
        return f"not 16-bit mono PCM at {ASSET_RATE} Hz"
    if not 1 <= frames <= ASSET_MOST_FRAMES:
        return f"not 1 to {ASSET_MOST_FRAMES} frames ({ASSET_MOST_FRAMES // ASSET_RATE} s at most)"
    samples = struct.unpack(f"<{len(pcm) // 2}h", pcm[: len(pcm) // 2 * 2])
    full = 32_767.0
    if max((abs(s) for s in samples), default=0) > full * 10 ** (ASSET_PEAK_DBFS / 20):
        return f"a peak past {ASSET_PEAK_DBFS:g} dBFS"
    for at in range(0, len(samples), ASSET_WINDOW):
        window = samples[at : at + ASSET_WINDOW]
        if math.sqrt(sum(s * s for s in window) / len(window)) > full * 10 ** (ASSET_LOUDEST_DBFS / 20):
            return f"louder than {ASSET_LOUDEST_DBFS:g} dBFS RMS over 10 ms"
    return None


def _content(manifest: dict[str, Any], files: dict[str, bytes], report: Report) -> None:
    """The files against the manifest, as the reader checks them after the
    manifest's own rules: paths, sizes, entry module, assets, skills."""
    if len(files) > MOST_FILES or sum(map(len, files.values())) > MOST_BYTES:
        report.errors.append(f"more than {MOST_FILES} files or {MOST_BYTES} bytes: too big for an app")
    for path in files:
        if why := path_errors(path):
            report.errors.append(why)
    runtime = manifest.get("runtime")
    if isinstance(runtime, dict) and isinstance(runtime.get("entry"), str):
        module = runtime["entry"].replace(".", "/")
        if f"src/{module}.py" not in files and f"src/{module}/__init__.py" not in files:
            report.errors.append(f"runtime.entry {runtime['entry']!r} is not in src/: no src/{module}.py or src/{module}/__init__.py")
    under = {p for p in files if p.startswith("assets/")}
    named = set(map(str, manifest.get("assets", []) or []))
    if under != named:
        report.errors.append(f"{MANIFEST}'s assets and the files under assets/ differ: {sorted(under ^ named)[0]!r}")
    for asset in sorted(under & named):
        if why := asset_error(files[asset]):
            report.errors.append(f"{asset}: {why}")
    folders: dict[str, dict[str, bytes]] = {}
    for path, data in files.items():
        if path.startswith("skills/"):
            rest = path[len("skills/") :]
            if "/" not in rest:
                report.errors.append(f"{path}: a file beside the companion skills' folders")
                continue
            name, inside = rest.split("/", 1)
            folders.setdefault(name, {})[inside] = data
    named = set(map(str, manifest.get("skills", []) or []))
    if set(folders) != named:
        report.errors.append(f"{MANIFEST}'s skills and the folders under skills/ differ: {sorted(set(folders) ^ named)[0]!r}")
    for name, inside in sorted(folders.items()):
        if "skill.toml" not in inside:
            report.errors.append(f"skills/{name}: no skill.toml: not a skill's folder")
            continue
        # The companion skill as `lamp skill check` checks a folder: written out, as the lamp places it.
        with tempfile.TemporaryDirectory() as scratch:
            folder = Path(scratch) / name
            for path, data in inside.items():
                (folder / path).parent.mkdir(parents=True, exist_ok=True)
                (folder / path).write_bytes(data)
            skill = check_skill(folder, shape_required=False)
        report.errors.extend(f"skills/{name}: {line}" for line in skill.errors)
        report.warnings.extend(f"skills/{name}: {line}" for line in skill.warnings)


def _folder_files(folder: Path, report: Report) -> dict[str, bytes]:
    """The folder's src/, assets/ and skills/ files by path, hidden files and
    Python's caches left out; anything else beside them is a warning."""
    files: dict[str, bytes] = {}
    for top in sorted(folder.iterdir()):
        if top.name.startswith(".") or top.name == SOURCE:
            continue
        if top.name not in TOPS:
            report.warnings.append(f"{top.name}: not packed: an app's files are src/, assets/ and skills/")
            continue
        for path in sorted(top.rglob("*")):
            relative = path.relative_to(folder)
            if any(part.startswith(".") or part == "__pycache__" for part in relative.parts) or path.suffix == ".pyc":
                continue
            if path.is_symlink():
                report.errors.append(f"{relative.as_posix()}: a link: an app is plain files and folders")
            elif path.is_file():
                files[relative.as_posix()] = path.read_bytes()
    return dict(sorted(files.items()))


def _from_folder(folder: Path, report: Report) -> tuple[dict[str, Any] | None, dict[str, bytes]]:
    try:
        source = json.loads((folder / SOURCE).read_text("utf-8"))
    except FileNotFoundError:
        report.errors.append(f"{SOURCE}: missing: an app's folder holds its manifest there (docs/apps.md)")
        return None, {}
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        report.errors.append(f"{SOURCE}: {error}")
        return None, {}
    if not isinstance(source, dict):
        report.errors.append(f"{SOURCE}: not a JSON object")
        return None, {}
    if "files" in source:
        report.errors.append(f"{SOURCE}: files is written by `lamp app pack`, not kept in {SOURCE}")
    for key, value in (("format", FORMAT), ("kind", KIND)):
        if source.get(key, value) != value:
            report.errors.append(f"{SOURCE}: {key} is {value!r} for an app")
    files = _folder_files(folder, report)
    manifest: dict[str, Any] = {"format": FORMAT, "kind": KIND, "name": folder.name, **source}
    if manifest["name"] != folder.name:
        report.warnings.append(f"{SOURCE} names {manifest['name']!r}; the package's folder takes that name, not {folder.name!r}")
    manifest.setdefault("skills", sorted({p.split("/")[1] for p in files if p.startswith("skills/") and p.count("/") >= 2}))
    manifest.setdefault("assets", sorted(p for p in files if p.startswith("assets/") and p.endswith(".wav")))
    manifest["files"] = [
        {"path": path, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()} for path, data in files.items()
    ]
    ordered = {key: manifest[key] for key in FIELDS if key in manifest}
    ordered.update({k: v for k, v in manifest.items() if k not in ordered})
    if not ordered.get("npu", True):
        ordered.pop("npu")
    return ordered, files


def read_tar(data: bytes, report: Report) -> tuple[dict[str, Any] | None, dict[str, bytes]]:
    """A package's tar read as lamp-skills reads it: plain files only, the
    manifest and signature beside one folder, no gzip, nothing twice."""
    if len(data) > MOST_PACKAGE_BYTES:
        report.errors.append(f"more than {MOST_PACKAGE_BYTES} bytes: too big for an app")
        return None, {}
    if data[:2] == b"\x1f\x8b":
        report.errors.append("a gzip: a package is a plain tar")
        return None, {}
    manifest_bytes, folder, files = None, None, {}
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as tar:
            for member in tar:
                path = member.name.rstrip("/")
                if member.isdir():
                    continue
                if not member.isreg():
                    report.errors.append(f"{path}: a link, a device or another entry that is not a plain file")
                    continue
                body = tar.extractfile(member).read()  # type: ignore[union-attr]
                if path in (MANIFEST, SIGNATURE):
                    if path == MANIFEST:
                        if manifest_bytes is not None:
                            report.errors.append(f"{path}: twice in the package")
                        manifest_bytes = body
                    else:
                        report.warnings.append(f"{SIGNATURE}: the Store signs the package at approval; this one is replaced")
                    continue
                if "/" not in path:
                    report.errors.append(f"{path}: a file beside the app's folder that is neither {MANIFEST} nor {SIGNATURE}")
                    continue
                top, rest = path.split("/", 1)
                if folder is not None and top != folder:
                    report.errors.append(f"{path}: a second folder beside {folder}: a package holds one app")
                    continue
                folder = top
                if rest in files:
                    report.errors.append(f"{path}: twice in the package")
                files[rest] = body
    except tarfile.TarError as error:
        report.errors.append(f"not a tar: {error}")
        return None, {}
    if manifest_bytes is None:
        report.errors.append(f"no {MANIFEST}: not an app's package")
        return None, files
    try:
        manifest = json.loads(manifest_bytes)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        report.errors.append(f"{MANIFEST}: {error}")
        return None, files
    if isinstance(manifest, dict):
        if manifest.get("format") == 1:
            report.errors.append(f"{MANIFEST}: format 1 is a skill: a skill goes on through the lamp (`lamp skill push`), not the Store's app upload")
            return None, files
        if folder != manifest.get("name"):
            report.errors.append(f"{MANIFEST} names {manifest.get('name')!r}, and the package's folder is {folder or ''!r}")
        listed: dict[str, dict[str, Any]] = {}
        for entry in manifest.get("files", []) if isinstance(manifest.get("files"), list) else []:
            if not isinstance(entry, dict) or set(entry) != {"path", "size", "sha256"}:
                report.errors.append(f"{MANIFEST}: a file is {{path, size, sha256}}")
                continue
            if entry["path"] in listed:
                report.errors.append(f"{MANIFEST} lists {entry['path']} twice")
            listed[entry["path"]] = entry
            got = files.get(entry["path"])
            if got is None:
                report.errors.append(f"{entry['path']} is in {MANIFEST} and not in the package")
            elif len(got) != entry["size"] or hashlib.sha256(got).hexdigest() != entry["sha256"]:
                report.errors.append(f"{entry['path']}: its size or sha256 is not what {MANIFEST} says: the package was changed or damaged")
        for extra in sorted(set(files) - set(listed)):
            report.errors.append(f"{extra} is in the package and not in {MANIFEST}")
    return manifest, files


def check(path: str | Path) -> Report:
    """An app's folder or packed tar, checked as the Store's reader checks
    it, less the signature; never raises for what is in it."""
    path = Path(path).resolve()
    report = Report(path)
    report.format = FORMAT
    if path.is_dir():
        manifest, files = _from_folder(path, report)
    elif path.is_file():
        manifest, files = read_tar(path.read_bytes(), report)
    else:
        report.errors.append("not a folder or a file")
        return report
    if manifest is None:
        return report
    report.errors.extend(manifest_errors(manifest))
    if isinstance(manifest, dict):
        _content(manifest, files, report)
    return report


def pack(folder: str | Path) -> AppPackage:
    """The app's folder as the Store's unsigned tar; a folder `check` refuses
    is refused, with its first reason."""
    folder = Path(folder).resolve()
    report = check(folder)
    if not report.ok:
        raise LampError(report.errors[0] + (f" (and {len(report.errors) - 1} more: `lamp app check`)" if len(report.errors) > 1 else ""), code="bad_request")
    manifest, files = _from_folder(folder, Report(folder))
    assert manifest is not None
    name = manifest["name"]
    data = b"".join(
        [
            _entry(MANIFEST, (json.dumps(manifest, indent=2, ensure_ascii=False) + "\n").encode()),
            *(_entry(f"{name}/{path}", body) for path, body in files.items()),
            bytes(1024),
        ]
    )
    if len(data) > MOST_PACKAGE_BYTES:
        raise LampError(f"the package is {len(data)} bytes: an app is {MOST_PACKAGE_BYTES} at most", code="bad_request")
    return AppPackage(name, manifest["version"], manifest, data)
