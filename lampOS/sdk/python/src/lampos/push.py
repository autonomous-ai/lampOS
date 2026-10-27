"""Putting a skill of your own on your own paired lamp (docs/control.md,
"Skills pushed from a computer"): pack a skill folder as `lamp-skills pack`
does, and the parts `skill.push` sends.

A package is a ustar tar: `lamp-skill.json` first (format, name, version,
and each file's path, size and SHA-256), then each file as `NAME/path`,
sorted, mode 0644, mtime 0, owner 0, with no folder entries and no links.
Hidden files (a name beginning with a dot) are left out, as the lamp's
loader passes them over. The lamp checks the bytes against `sha256`, then
each file against the manifest.
"""

from __future__ import annotations

import base64
import hashlib
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

if sys.version_info >= (3, 11):
    import tomllib
else:  # pragma: no cover - Python 3.10
    import tomli as tomllib

from .errors import LampError

MANIFEST = "lamp-skill.json"
PACKAGE_MAX = 1 << 20
# A part's bytes, so its base64 is at most 48 KiB and a frame stays under 64 KiB.
PART_BYTES = 36 * 1024
PARTS_MAX = 32
# The grants a skill's own [permissions] keys ask for, where a key is one.
PERMISSION_GRANTS = {"body": "body", "light": "light", "speak": "speak"}
# Format 1's presence topics, the protocol's `look` (docs/control.md).
PRESENCE_TOPICS = ("humans/faces/tracked", "room/stood_up")
# The topics a push may read, as lamp-skills' `PUSHED_TOPICS` (#3879): presence,
# and those the protocol's events take with no grant (`time`, `lamp.changed`,
# `touch`).
PUSHED_TOPICS = (*PRESENCE_TOPICS, "clock/local", "voice/state", "lamp/pause", "controls/touch")
# What a push refuses until format 2 lists phrases and classes.
HEARD_TOPICS = ("voice/heard", "name/heard", "ears/classes")
# [permissions] keys outside the protocol's grants, refused by a push.
OUTSIDE = ("actions", "messages", "known_faces", "who", "output_limits", "propose", "core")

@dataclass
class Package:
    name: str
    version: str
    data: bytes

    @property
    def sha256(self) -> str:
        return hashlib.sha256(self.data).hexdigest()

    def parts(self) -> list[str]:
        """The package as base64 parts, in order."""
        chunks = [self.data[at : at + PART_BYTES] for at in range(0, len(self.data), PART_BYTES)]
        return [base64.b64encode(chunk).decode("ascii") for chunk in chunks]


MOST_DEPTH = 4
_PART = re.compile(r"[A-Za-z0-9_.-]{1,100}")


def path_ok(path: object) -> bool:
    """A file's path in a skill as lamp-skills' `path_ok` takes it: 1 to 4
    parts of letters, digits, -, _ and ., none beginning with a dot. So no
    path in a package reaches outside its folder when written out."""
    if not isinstance(path, str):
        return False
    parts = path.split("/")
    return len(parts) <= MOST_DEPTH and all(_PART.fullmatch(part) and not part.startswith(".") for part in parts)


def _path_refused(path: str) -> LampError:
    return LampError(
        f"{path}: a skill's paths are at most {MOST_DEPTH} parts of letters, digits, -, _ and ., none beginning with a dot",
        code="bad_request",
    )


def _files(folder: Path) -> dict[str, bytes]:
    """Every plain file but hidden ones, by its `/` path; a link refuses it,
    and so does a path lamp-skills would refuse (Reviewer-lead's P3 on #4029)."""
    files: dict[str, bytes] = {}
    for path in sorted(folder.rglob("*")):
        relative = path.relative_to(folder)
        if any(part.startswith(".") for part in relative.parts):
            continue
        if path.is_symlink():
            raise LampError(f"{relative.as_posix()}: a link: a skill is plain files and folders", code="bad_request")
        if path.is_file():
            if not path_ok(relative.as_posix()):
                raise _path_refused(relative.as_posix())
            files[relative.as_posix()] = path.read_bytes()
    return dict(sorted(files.items()))


def _octal(value: int, width: int) -> bytes:
    # The tar crate's `octal_into`: zero-padded digits, then a NUL.
    return f"{value:o}".rjust(width - 1, "0").encode() + b"\0"


def _entry(path: str, data: bytes) -> bytes:
    """One ustar header and its data, padded to 512 bytes, byte for byte as
    lamp-skills' `tar::Header::new_ustar` writes them (package.rs `append`):
    mode 0644, mtime 0, owner 0, no user or group names, the checksum as
    seven octal digits and a NUL. So one folder packs to one SHA-256 here,
    in JS and in `lamp-skills pack` (harness-reviewer's P3 on #4029)."""
    name, prefix = path.encode(), b""
    if len(name) > 100:
        # The prefix is up to the last `/` within 155 bytes, as the tar
        # crate's ustar `set_path` peels it. Where that leaves a name over
        # 100, or there is no such `/`, the crate writes a GNU long-name
        # entry instead: refused here, so the bytes stay the crate's (paths
        # are ASCII by `path_ok`, so bytes are characters).
        cut = path.rfind("/", 0, 156)
        if cut < 0 or len(path) - cut - 1 > 100:
            raise LampError(f"{path}: longer than a tar header holds; shorten its folders", code="bad_request")
        prefix, name = path[:cut].encode(), path[cut + 1 :].encode()
    header = bytearray(512)
    header[0 : len(name)] = name
    header[100:108] = _octal(0o644, 8)
    header[108:116] = _octal(0, 8)
    header[116:124] = _octal(0, 8)
    header[124:136] = _octal(len(data), 12)
    header[136:148] = _octal(0, 12)
    header[148:156] = b" " * 8
    header[156:157] = b"0"
    header[257:265] = b"ustar\x0000"
    header[345 : 345 + len(prefix)] = prefix
    header[148:156] = _octal(sum(header), 8)
    return bytes(header) + data + bytes(-len(data) % 512)


def pack(folder: str | Path) -> Package:
    """The skill folder as a package, `lamp-skills pack`'s shape. Its name is
    the folder's, and `skill.toml` must say a `version`."""
    folder = Path(folder).resolve()
    try:
        rules = tomllib.loads((folder / "skill.toml").read_text("utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        raise LampError(f"skill.toml: {error}", code="bad_request") from error
    version = rules.get("version")
    if not isinstance(version, str):
        raise LampError('skill.toml says no version: a package carries one, as version = "1.0.0"', code="bad_request")
    files = _files(folder)
    manifest = {
        "format": 1,
        "name": folder.name,
        "version": version,
        "files": [
            {"path": path, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
            for path, data in files.items()
        ],
    }
    # Two zero blocks end it, as the tar crate's `finish`, with no padding to
    # a 10 KiB record (Python's tarfile pads).
    data = b"".join(
        [
            _entry(MANIFEST, (json.dumps(manifest, indent=2) + "\n").encode()),
            *(_entry(f"{folder.name}/{path}", data) for path, data in files.items()),
            bytes(1024),
        ]
    )
    if len(data) > PACKAGE_MAX:
        raise LampError(f"the package is {len(data)} bytes: a pushed skill is 1 MiB at most", code="bad_request")
    if len(Package(folder.name, version, data).parts()) > PARTS_MAX:
        raise LampError(f"more than {PARTS_MAX} parts", code="bad_request")
    return Package(folder.name, version, data)


def asks_beyond(folder: str | Path, grants: list[str]) -> tuple[str, str] | None:
    """The code and why the lamp would refuse this skill as a push for this
    pairing's grants (docs/control.md, "Skills pushed from a computer"), or
    None: `no_grant` for a grant the pairing lacks, `refused` for what no
    grant allows. The lamp checks it all again; a skill that sees the picture isn't refused
    here, the lamp holds it off for the owner's yes."""
    try:
        rules = tomllib.loads((Path(folder) / "skill.toml").read_text("utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError):
        return None
    kind = rules.get("kind", "installable")
    if kind != "installable":
        return "refused", f'kind = "{kind}": a pushed skill is installable'
    permissions = rules.get("permissions") if isinstance(rules.get("permissions"), dict) else {}
    for key in OUTSIDE:
        if permissions.get(key):
            return "refused", f"[permissions] {key} is outside the protocol's grants: a push refuses it"
    for topic in permissions.get("topics", []) or []:
        if topic in HEARD_TOPICS:
            return "no_grant", f"topic {topic}: hear and sound wait for format 2's phrase and class lists"
        if topic not in PUSHED_TOPICS:
            return "refused", f"topic {topic}: a raw topic is outside the protocol's grants"
        if topic in PRESENCE_TOPICS and "look" not in grants:
            return "no_grant", f"topic {topic} needs the look grant, which this computer doesn't hold"
    for key, grant in PERMISSION_GRANTS.items():
        if permissions.get(key) and grant not in grants:
            return "no_grant", f"[permissions] {key} needs the {grant} grant, which this computer doesn't hold"
    return None
