"""Shared-skill indexes, on this computer only (decision 4, #3997): a signed
list of signed packages to fetch from. An index grants nothing and nothing of
it runs on the lamp; each package it lists is checked against its entry, then
installed as any shared package is (`Lamp.install_package`), and the lamp
checks it all again.

`lamp-index.json`, format 1, at most 1 MiB:

    {"format": 1, "index": "<the listing, a JSON string>",
     "key": "<FPR16>", "public": "<base64, 32 bytes>",
     "signature": "<base64 Ed25519 over the UTF-8 bytes of index>"}

    listing = {"format": 1, "name": "desk-things", "seq": 7, "skills": [
        {"name", "version", "author": FPR16, "sha256", "size", "url", "asks"}]}

The signature covers the listing's exact bytes, carried as a string, so
nothing is canonicalised. The publisher's key is pinned at `index add`, and a
listing never goes back to a lower `seq`. A signature says the bytes are that
key's, not who is behind it: "published by key FPR16, not the Store", never
"verified".
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import re
from dataclasses import dataclass
from pathlib import Path

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey

from .errors import LampError
from .push import PACKAGE_MAX, _NAME, _public_bytes, asks_of, fingerprint, open_package, tomllib

FORMAT = 1
INDEX_FILE = "lamp-index.json"
_HEX64 = re.compile(r"[0-9a-f]{64}")
_FPR = re.compile(r"[0-9a-f]{16}")


def _bad(why: str) -> LampError:
    return LampError(f"not a skill index: {why}", code="bad_request")


@dataclass
class Entry:
    """One package an index lists, as it says it."""

    name: str
    version: str
    author: str
    sha256: str
    size: int
    url: str
    asks: list[str]


@dataclass
class Listing:
    """An index read back and checked: its name, `seq`, its publisher's
    whole key (base64) and fingerprint, and its entries."""

    name: str
    seq: int
    public: str
    publisher: str
    skills: list[Entry]


def _entry(raw: object) -> Entry:
    if not isinstance(raw, dict):
        raise _bad("an entry is an object")
    name, url = raw.get("name"), raw.get("url")
    if not isinstance(name, str) or not _NAME.fullmatch(name):
        raise _bad(f"{name!r} is not a skill's name (1 to 64 of a-z, 0-9 and -)")
    if not isinstance(url, str) or not url.startswith("https://"):
        raise _bad(f"{name}: its url is fetched over https only")
    author, sha, size, version, asks = (raw.get(key) for key in ("author", "sha256", "size", "version", "asks"))
    if not isinstance(author, str) or not _FPR.fullmatch(author):
        raise _bad(f"{name}: author is a key's fingerprint, 16 hex: an index lists signed packages only")
    if not isinstance(sha, str) or not _HEX64.fullmatch(sha):
        raise _bad(f"{name}: sha256 is 64 lowercase hex")
    if not isinstance(size, int) or isinstance(size, bool) or not 0 < size <= PACKAGE_MAX:
        raise _bad(f"{name}: size is 1 to {PACKAGE_MAX} bytes")
    if not isinstance(version, str) or not version:
        raise _bad(f"{name}: no version")
    if not isinstance(asks, list) or not all(isinstance(ask, str) for ask in asks):
        raise _bad(f"{name}: asks is a list of grant names")
    return Entry(name, version, author, sha, size, url, asks)


def open_index(data: bytes, *, pinned: str | None = None, last_seq: int | None = None) -> Listing:
    """Check an index: its signature over the listing's exact bytes, by the
    key it carries, and that key the `pinned` one (base64) when there is one;
    a `seq` no lower than `last_seq`; and each entry's shape."""
    if len(data) > PACKAGE_MAX:
        raise _bad(f"more than {PACKAGE_MAX} bytes")
    try:
        outer = json.loads(data)
        listing_text, key = outer["index"], outer["key"]
        public = base64.b64decode(outer["public"], validate=True)
        signature = base64.b64decode(outer["signature"], validate=True)
    except (ValueError, KeyError, TypeError) as error:
        raise _bad('it is {"format", "index", "key", "public", "signature"}, signed by its publisher') from error
    if outer.get("format") != FORMAT:
        raise _bad(f"format {outer.get('format')!r} is not one this SDK reads ({FORMAT})")
    if not isinstance(listing_text, str) or len(public) != 32 or fingerprint(public) != key:
        raise _bad("key is not the fingerprint of public")
    try:
        Ed25519PublicKey.from_public_bytes(public).verify(signature, listing_text.encode("utf-8"))
    except InvalidSignature as error:
        raise _bad("the signature does not match: the index was changed after it was signed") from error
    public_b64 = base64.b64encode(public).decode("ascii")
    try:
        listing = json.loads(listing_text)
        name, seq, skills = listing["name"], listing["seq"], listing["skills"]
    except (ValueError, KeyError, TypeError) as error:
        raise _bad(f"its listing: {error}") from error
    if listing.get("format") != FORMAT or not isinstance(skills, list):
        raise _bad(f"its listing is format {FORMAT}, with a list of skills")
    if not isinstance(name, str) or not _NAME.fullmatch(name):
        raise _bad(f"{name!r} is not an index's name (1 to 64 of a-z, 0-9 and -)")
    if pinned is not None and public_b64 != pinned:
        raise LampError(
            f"index {name} is published by key {fingerprint(base64.b64decode(pinned))} here, "
            f"and this one by key {key}: remove it and add it again to trust another key",
            code="refused",
        )
    if not isinstance(seq, int) or isinstance(seq, bool) or seq < 0:
        raise _bad("seq is a whole number")
    if last_seq is not None and seq < last_seq:
        raise LampError(f"index {name} went back from seq {last_seq} to {seq}: an older copy is never taken", code="refused")
    entries = [_entry(raw) for raw in skills]
    if len({entry.name for entry in entries}) != len(entries):
        raise _bad("a skill's name is listed twice")
    return Listing(name, seq, public_b64, key, entries)


def make_index(name: str, packages: list[bytes], url_base: str, seq: int, publisher: Ed25519PrivateKey) -> bytes:
    """An index of these signed packages, each at `url_base` + NAME-VERSION.tar,
    its entries taken from the packages themselves, signed by `publisher`."""
    if not url_base.startswith("https://"):
        raise LampError("an index's packages are fetched over https only: --url-base https://…", code="bad_request")
    base = url_base if url_base.endswith("/") else url_base + "/"
    skills = []
    for data in packages:
        opened = open_package(data)
        if not opened.author:
            raise LampError(f"{opened.name}: an index lists signed packages only (lamp skill pack --sign)", code="bad_request")
        rules = tomllib.loads(opened.files.get("skill.toml", b"").decode("utf-8"))
        skills.append({
            "name": opened.name,
            "version": opened.version,
            "author": opened.author,
            "sha256": hashlib.sha256(data).hexdigest(),
            "size": len(data),
            "url": f"{base}{opened.name}-{opened.version}.tar",
            "asks": asks_of(rules),
        })
    listing = json.dumps({"format": FORMAT, "name": name, "seq": seq, "skills": skills}, separators=(",", ":"))
    public = _public_bytes(publisher)
    signed = {
        "format": FORMAT,
        "index": listing,
        "key": fingerprint(public),
        "public": base64.b64encode(public).decode("ascii"),
        "signature": base64.b64encode(publisher.sign(listing.encode("utf-8"))).decode("ascii"),
    }
    data = (json.dumps(signed, separators=(",", ":")) + "\n").encode()
    open_index(data)  # what it writes, it reads
    return data


def check_package(entry: Entry, data: bytes):
    """The fetched package against its entry: the same bytes (SHA-256 and
    size), signed by the entry's author, asking what the entry says. Returns
    the opened package."""
    if len(data) != entry.size or hashlib.sha256(data).hexdigest() != entry.sha256:
        raise LampError(f"{entry.name}: the package fetched is not the one the index lists (SHA-256)", code="refused")
    opened = open_package(data)
    if opened.name != entry.name or opened.version != entry.version:
        raise LampError(f"{entry.name}: the package is {opened.name} {opened.version}, not the index's", code="refused")
    if opened.author != entry.author:
        signer = f"key {opened.author}" if opened.author else "no key"
        raise LampError(f"{entry.name}: signed by {signer}, and the index says key {entry.author}", code="refused")
    asks = asks_of(tomllib.loads(opened.files.get("skill.toml", b"").decode("utf-8")))
    if asks != entry.asks:
        raise LampError(f"{entry.name}: it asks [{', '.join(asks)}], and the index says [{', '.join(entry.asks)}]", code="refused")
    return opened


class Indexes:
    """The indexes this computer trusts to list skills, in `folder`'s
    `indexes.json`: each one's URL, its publisher's pinned key and the last
    `seq` seen."""

    def __init__(self, folder: Path):
        self.path = Path(folder) / "indexes.json"

    def all(self) -> dict[str, dict]:
        try:
            return json.loads(self.path.read_text())
        except FileNotFoundError:
            return {}

    def _write(self, kept: dict[str, dict]) -> None:
        self.path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        part = self.path.with_suffix(".part")
        part.write_text(json.dumps(kept, indent=2, sort_keys=True) + "\n")
        os.replace(part, self.path)

    def keep(self, url: str, listing: Listing) -> None:
        kept = self.all()
        held = kept.get(listing.name)
        if held and held["public"] != listing.public:
            raise LampError(f"an index named {listing.name} is kept already, by another key: remove it first", code="refused")
        kept[listing.name] = {"url": url, "public": listing.public, "key": listing.publisher, "seq": listing.seq}
        self._write(kept)

    def remove(self, name: str) -> None:
        kept = self.all()
        if name not in kept:
            raise LampError(f"no index named {name} here", code="bad_request")
        del kept[name]
        self._write(kept)


def read_kept(indexes: Indexes, name: str, data: bytes) -> Listing:
    """A kept index fetched again: held to its pinned key and its last
    `seq`, which is moved on."""
    held = indexes.all()[name]
    listing = open_index(data, pinned=held["public"], last_seq=held["seq"])
    if listing.name != name:
        raise LampError(f"the index at {held['url']} is now named {listing.name}, not {name}", code="refused")
    indexes.keep(held["url"], listing)
    return listing


def read_source(source: str, fetch=None) -> bytes:
    """An index or package from an https URL (the SDK's capped fetch) or a
    file here, 1 MiB at most either way."""
    from .push import fetch as fetch_https, read_capped

    if "://" in source:
        return (fetch or fetch_https)(source)
    with open(source, "rb") as file:
        return read_capped(file)


def refresh(indexes: Indexes, fetch=None) -> list[Listing]:
    """Every kept index fetched again, each held to its pinned key and last
    `seq`."""
    return [read_kept(indexes, name, read_source(held["url"], fetch)) for name, held in indexes.all().items()]


def find(indexes: Indexes, name: str, from_index: str | None = None, fetch=None) -> tuple[Listing, Entry, bytes]:
    """`name` looked up in the kept indexes (or `from_index` alone), its
    package fetched from the entry's URL and checked against the entry."""
    kept = indexes.all()
    if from_index is not None and from_index not in kept:
        raise LampError(f"no index named {from_index} here: lamp skill index add URL", code="bad_request")
    found = []
    for index_name in [from_index] if from_index else sorted(kept):
        listing = read_kept(indexes, index_name, read_source(kept[index_name]["url"], fetch))
        found += [(listing, entry) for entry in listing.skills if entry.name == name]
    if not found:
        raise LampError(f"{name} is in no index kept here", code="bad_request")
    if len(found) > 1:
        names = ", ".join(listing.name for listing, _ in found)
        raise LampError(f"{name} is listed by {names}: say which with --from", code="bad_request")
    listing, entry = found[0]
    data = read_source(entry.url, fetch)
    check_package(entry, data)
    return listing, entry, data
