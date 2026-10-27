"""`lamp skill check FOLDER`: a skill folder against the public skill format,
version 1 (docs/skill-format.md), before it goes to a lamp or the Store.

It checks what can be checked off the lamp: the folder's files, SKILL.md's
front matter, skill.toml's shape against the package's copy of
docs/skill-format.schema.json, and the bounds of its moves and sounds. The
rules between keys (a trigger or a tool, permissions that cover every read
and call) are the lamp's own load check, which this doesn't repeat; a
folder that passes here can still be refused there.
"""

from __future__ import annotations

import json
import os
import re
import stat
import sys
import wave
from dataclasses import dataclass, field
from functools import lru_cache
from importlib import resources
from pathlib import Path
from typing import Any

if sys.version_info >= (3, 11):
    import tomllib
else:  # pragma: no cover - Python 3.10
    import tomli as tomllib

NAME = re.compile(r"^[a-z0-9-]{1,64}$")
# Script skills are a later format: a folder holding one is refused, found by
# its suffix, its executable bit or a `#!` line.
SCRIPTS = {".py", ".sh", ".js", ".mjs", ".ts", ".rb", ".pl", ".lua", ".exe", ".bin"}
# The files the lamp reads, never through a link (`O_NOFOLLOW`, lamp-skills).
READ_ON_THE_LAMP = ("SKILL.md", "skill.toml", "moves", "sounds", "lights", "shows")
SOUND_MOST_S = 3.0
SOUND_RATE = 24_000
# A move, as lamp-body's `moves::check` holds it (crates/body/src/moves.rs).
MOVE_MOST_BYTES = 256 * 1024
MOVE_NAME = re.compile(r"^[a-z0-9_-]{1,32}$")
MOVE_STEP_MS = (40, 1_000)
MOVE_LASTS_MS = (1_500, 10_000)
# Head pitch, head roll, base yaw, shoulder, elbow: counts from rest, and
# counts a second.
MOVE_FARTHEST = (200, 200, 150, 350, 350)
MOVE_FASTEST = (900.0, 1_300.0, 1_000.0, 800.0, 800.0)
# The body's own clips besides the protocol's gestures (clips.rs, idle.rs).
BODY_OWN = ("idle", "scanning", "called", "dip", "look_up", "tilt_left", "tilt_right", "two")


@dataclass
class Report:
    """What the check found: `errors` refuse the folder, `warnings` don't."""

    folder: Path
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.errors


@lru_cache(maxsize=1)
def format_schema() -> dict[str, Any]:
    text = resources.files("lampos").joinpath("skill-format.schema.json").read_text("utf-8")
    return json.loads(text)


def _read(folder: Path, name: str, report: Report) -> str | None:
    """A file the lamp reads, as its text, or None with the reason kept: a
    file that can't be read is the folder's error, never a raise."""
    path = folder / name
    if path.is_symlink():
        report.errors.append(f"{name}: a link; the lamp never reads through one")
        return None
    if not path.exists():
        report.errors.append(f"{name}: missing")
        return None
    if not path.is_file():
        report.errors.append(f"{name}: not a file")
        return None
    try:
        data = path.read_bytes()
    except OSError as error:
        report.errors.append(f"{name}: can't be read: {error.strerror or error}")
        return None
    if data.startswith(b"\xef\xbb\xbf"):
        report.errors.append(f"{name}: starts with a byte-order mark; the lamp reads plain UTF-8")
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        report.errors.append(f"{name}: not UTF-8 text (saved as UTF-16, say)")
        return None


def _front_matter(text: str) -> tuple[dict[str, str], str | None]:
    """The lamp's own reading (lamp-skills' `front_matter`): exactly `---\n`
    first, the body up to the next `\n---`, and a key at a line's start
    followed at once by `:`, its value trimmed and taken as it is, quotes
    and all."""
    if not text.startswith("---\n") or "\n---" not in text[4:]:
        return {}, "SKILL.md: begins with front matter between two --- lines, the first exactly ---"
    body = text[4:].split("\n---", 1)[0]
    keys: dict[str, str] = {}
    for key in ("name", "description"):
        for line in body.split("\n"):
            line = line.removesuffix("\r")
            if line.startswith(key) and line[len(key) :].startswith(":"):
                # The first such line is the key's, empty or not, as the
                # lamp's `find_map` takes it.
                value = line[len(key) + 1 :].strip()
                if value:
                    keys[key] = value
                break
    return keys, None


def _skill_md(folder: Path, report: Report) -> None:
    text = _read(folder, "SKILL.md", report)
    if text is None:
        return
    keys, wrong = _front_matter(text)
    if wrong:
        report.errors.append(wrong)
        return
    name, description = keys.get("name", ""), keys.get("description", "")
    if not name:
        report.errors.append("SKILL.md: the front matter has no name (a line `name: ...`)")
    elif not NAME.match(name):
        report.errors.append("SKILL.md: name must be 1 to 64 characters of a-z, 0-9 and -")
    elif name != folder.name:
        report.errors.append(f"SKILL.md: name {name!r} isn't the folder's name {folder.name!r}")
    if not description:
        report.errors.append("SKILL.md: the front matter has no description (a line `description: ...`)")


def _where(path: Any) -> str:
    return "".join(f"[{part}]" if isinstance(part, int) else f".{part}" for part in path).lstrip(".") or "(top)"


def _skill_toml(folder: Path, report: Report) -> dict[str, Any] | None:
    text = _read(folder, "skill.toml", report)
    if text is None:
        return None
    try:
        rules = tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        report.errors.append(f"skill.toml: not TOML: {error}")
        return None
    try:
        from jsonschema import Draft202012Validator
    except ImportError:
        report.errors.append("skill.toml: its shape needs jsonschema: pip install 'lampos[check]'")
        return rules
    validator = Draft202012Validator(format_schema())
    for error in sorted(validator.iter_errors(rules), key=lambda e: list(map(str, e.absolute_path))):
        unknown = _unknown_keys(error)
        if unknown and not error.absolute_path:
            report.errors.append(f"skill.toml: {unknown} isn't a key of format 1")
        elif unknown:
            report.errors.append(f"skill.toml: {_where(error.absolute_path)}: {unknown} isn't a key there")
        else:
            report.errors.append(f"skill.toml: {_where(error.absolute_path)}: {error.message}")
    return rules


def _unknown_keys(error: Any) -> str:
    """The keys an `additionalProperties: false` refused, as `[a], [b]`."""
    if error.validator != "additionalProperties" or not isinstance(error.instance, dict):
        return ""
    known = set(error.schema.get("properties", {}))
    patterns = [re.compile(p) for p in error.schema.get("patternProperties", {})]
    names = [k for k in error.instance if k not in known and not any(p.search(k) for p in patterns)]
    return ", ".join(f"[{name}]" for name in sorted(names))


def _is_script(path: Path) -> bool:
    if path.suffix.lower() in SCRIPTS:
        return True
    try:
        if path.stat().st_mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH):
            return True
        with path.open("rb") as file:
            return file.read(2) == b"#!"
    except OSError:
        return False


def _walk(folder: Path, report: Report) -> None:
    """Every entry, links not followed: a link the lamp would read is an
    error, any other a warning; a script anywhere is refused."""
    for root, dirs, files in os.walk(folder, followlinks=False):
        for name in sorted(dirs + files):
            path = Path(root) / name
            where = path.relative_to(folder)
            if path.is_symlink():
                read = where.parts[0] in READ_ON_THE_LAMP
                line = f"{where}: a link; the lamp never reads through one"
                if read and str(where) not in ("SKILL.md", "skill.toml"):  # those two say it themselves
                    report.errors.append(line)
                elif not read:
                    report.warnings.append(f"{where}: a link; the lamp won't follow it, and the Store won't pack it")
            elif name in files and _is_script(path):
                report.errors.append(f"{where}: a script; script skills are a later format")


def _move(path: Path, where: Path, report: Report) -> None:
    """One move, held as lamp-body's `moves::read` and `moves::check` hold it."""
    if path.is_symlink():
        return  # said by _walk
    if not path.is_file():
        report.errors.append(f"{where}: not a file")
        return
    try:
        if path.stat().st_size > MOVE_MOST_BYTES:
            report.errors.append(f"{where}: larger than 256 KiB")
            return
        move = json.loads(path.read_bytes().decode("utf-8"))
    except OSError as error:
        report.errors.append(f"{where}: can't be read: {error.strerror or error}")
        return
    except (ValueError, UnicodeDecodeError) as error:
        report.errors.append(f"{where}: not JSON: {error}")
        return
    if not isinstance(move, dict) or set(move) != {"name", "step_ms", "frames"}:
        report.errors.append(f"{where}: a move is JSON of exactly name, step_ms and frames")
        return
    name, step_ms, frames = move["name"], move["step_ms"], move["frames"]
    if not isinstance(name, str) or not MOVE_NAME.match(name):
        report.errors.append(f"{where}: name {name!r}: 1 to 32 of a-z, 0-9, - and _")
        return
    if name != path.stem:
        report.errors.append(f"{where}: its name {name!r} isn't {path.stem!r}")
    from . import schema

    if name in schema.gestures() or name in BODY_OWN:
        report.errors.append(f"{where}: {name!r} is one of the body's own gestures")
    if not isinstance(step_ms, int) or isinstance(step_ms, bool) or not MOVE_STEP_MS[0] <= step_ms <= MOVE_STEP_MS[1]:
        report.errors.append(f"{where}: step_ms {step_ms!r} is not from 40 to 1000")
        return
    if not isinstance(frames, list) or not all(
        isinstance(f, list) and len(f) == 5 and all(isinstance(c, int) and not isinstance(c, bool) for c in f)
        for f in frames
    ):
        report.errors.append(f"{where}: frames are lists of five whole counts")
        return
    lasts = max(len(frames) - 1, 0) * step_ms
    if not MOVE_LASTS_MS[0] <= lasts <= MOVE_LASTS_MS[1]:
        report.errors.append(f"{where}: it lasts {lasts} ms, not 1500 to 10000")
    if not frames or frames[0] != [0] * 5 or frames[-1] != [0] * 5:
        report.errors.append(f"{where}: it does not begin and end at nothing")
    step = step_ms / 1_000
    for at, (before, after) in enumerate(zip(frames, frames[1:]), start=1):
        for joint in range(5):
            if abs(after[joint]) > MOVE_FARTHEST[joint]:
                report.errors.append(
                    f"{where}: frame {at} takes joint {joint} {after[joint]} counts, farther than {MOVE_FARTHEST[joint]}"
                )
                return
            speed = abs(after[joint] - before[joint]) / step
            if speed > MOVE_FASTEST[joint] + 2 / step:
                report.errors.append(
                    f"{where}: frame {at} asks joint {joint} for {speed:.0f} counts a second, "
                    f"faster than {MOVE_FASTEST[joint]:.0f}"
                )
                return


def _files(folder: Path, report: Report) -> None:
    _walk(folder, report)
    for path in sorted((folder / "moves").glob("*.json")):
        # lamp-body's `moves::load` passes over dot-names (a Mac's `._x.json`).
        if not path.name.startswith("."):
            _move(path, path.relative_to(folder), report)
    for path in sorted((folder / "sounds").glob("*")):
        where = path.relative_to(folder)
        if path.is_symlink():
            continue  # said by _walk
        if not path.is_file():
            report.errors.append(f"{where}: not a file")
            continue
        if path.suffix.lower() != ".wav":
            report.errors.append(f"{where}: sounds are .wav files")
            continue
        try:
            with wave.open(str(path), "rb") as sound:
                shape = (sound.getsampwidth(), sound.getnchannels(), sound.getframerate())
                seconds = sound.getnframes() / max(sound.getframerate(), 1)
        except (wave.Error, EOFError, OSError) as error:
            report.errors.append(f"{where}: not a WAV the lamp reads: {error}")
            continue
        if shape != (2, 1, SOUND_RATE):
            report.errors.append(f"{where}: must be 16-bit mono PCM at 24 kHz")
        if seconds > SOUND_MOST_S:
            report.errors.append(f"{where}: {seconds:.1f} s, longer than 3 s")


def _permissions(rules: dict[str, Any], report: Report) -> None:
    permissions = rules.get("permissions")
    if isinstance(permissions, dict) and permissions.get("look") is True and permissions.get("camera") is not True:
        # Format 1's `look = true` alone gives the picture (lamp-skills'
        # `sees_picture`); `camera` is that grant's own name, and format 1's
        # presence is the presence topics (docs/skill-format.md).
        report.warnings.append(
            "skill.toml: permissions.look = true alone gives the model the full picture, as format 1 reads it; "
            "ask camera = true for the picture; in format 1, presence is the topics"
        )


def check(folder: str | Path) -> Report:
    """Check one skill folder; never raises for what's in it."""
    folder = Path(folder).resolve()
    report = Report(folder)
    if not folder.is_dir():
        report.errors.append("not a folder")
        return report
    _skill_md(folder, report)
    rules = _skill_toml(folder, report)
    _files(folder, report)
    if isinstance(rules, dict):
        _permissions(rules, report)
    return report
