"""A computer's pairing with its lamps: its Ed25519 key and, per lamp, the
client id, grants and the pin of its certificate (its full SHA-256).

Kept in ~/.config/lampos/ (or $LAMPOS_HOME): `key` (the private key, 0600)
and `lamps.json`. Nothing else of the pairing is kept, and the code itself
never is.
"""

from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
from dataclasses import asdict, dataclass, field
from pathlib import Path

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def home() -> Path:
    return Path(os.environ.get("LAMPOS_HOME") or Path.home() / ".config" / "lampos")


def b64(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def unb64(text: str, length: int | None = None) -> bytes:
    """Strict base64; with `length`, exactly that many bytes."""
    data = base64.b64decode(text.encode("ascii"), validate=True)
    if length is not None and len(data) != length:
        raise ValueError(f"{len(data)} bytes, not {length}")
    return data


# Each proof names what it is for, so one made for pairing can never stand
# as a hello's (docs/control.md).
PAIR = b"lampos/v1/pair\0"
HELLO = b"lampos/v1/hello\0"


def proof(code: str, pin: bytes, key: bytes, nonce: bytes) -> bytes:
    """Pairing's proof: HMAC-SHA256 keyed with the code's ASCII digits, over
    PAIR, the pin's 32 bytes, the 32 key bytes, then the welcome's 32 nonce
    bytes (docs/control.md). With the pin inside, a proof relayed through a
    host in the middle fails at the real lamp."""
    return hmac.new(code.encode("ascii"), PAIR + pin + key + nonce, hashlib.sha256).digest()


def pin_of(certificate_der: bytes) -> str:
    """The certificate's pin: its full SHA-256, 64 lowercase hex, as the
    owner's app shows it with the pairing code."""
    return hashlib.sha256(certificate_der).hexdigest()


def is_pin(text: str) -> bool:
    return len(text) == 64 and all(c in "0123456789abcdef" for c in text)


@dataclass
class Pairing:
    """One lamp this computer is paired with."""

    lamp: str
    client: str
    grants: list[str]
    fp: str  # the certificate's pin: its full SHA-256, 64 hex
    host: str
    port: int
    name: str = ""
    extra: dict = field(default_factory=dict)


class Keyring:
    """This computer's key and pairings, in `folder`."""

    def __init__(self, folder: Path | None = None):
        self.folder = Path(folder) if folder else home()

    def _own_folder(self) -> None:
        """The folder, this user's alone (0700) whatever the umask: anyone
        who could write in it could swap a lamp's host and pin, or the key
        (harness-reviewer's P2 on #3760)."""
        self.folder.mkdir(mode=0o700, parents=True, exist_ok=True)
        os.chmod(self.folder, 0o700)

    def key(self) -> Ed25519PrivateKey:
        """The computer's key, made on first use."""
        key = self._key_file("key", make=True)
        assert key is not None
        return key

    def author_key(self, *, make: bool = False) -> Ed25519PrivateKey | None:
        """The key this person signs their skills with (`lamp skill keygen`),
        kept apart from the pairing key: revoking a pairing never changes
        who wrote a skill. None if it hasn't been made."""
        return self._key_file("author.pem", make=make)

    def _key_file(self, name: str, *, make: bool) -> Ed25519PrivateKey | None:
        path = self.folder / name
        if path.exists():
            loaded = serialization.load_pem_private_key(path.read_bytes(), password=None)
            assert isinstance(loaded, Ed25519PrivateKey)
            return loaded
        if not make:
            return None
        key = Ed25519PrivateKey.generate()
        self._own_folder()
        pem = key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as file:
            file.write(pem)
        return key

    def public(self) -> bytes:
        return self.key().public_key().public_bytes(
            serialization.Encoding.Raw, serialization.PublicFormat.Raw
        )

    def hello(self, nonce: bytes) -> bytes:
        """Hello's signature: over HELLO, then the welcome's nonce."""
        return self.key().sign(HELLO + nonce)

    def _path(self) -> Path:
        return self.folder / "lamps.json"

    def pairings(self) -> dict[str, Pairing]:
        path = self._path()
        if not path.exists():
            return {}
        return {lamp: Pairing(**fields) for lamp, fields in json.loads(path.read_text("utf-8")).items()}

    def remember(self, pairing: Pairing) -> None:
        pairings = self.pairings()
        pairings[pairing.lamp] = pairing
        self._own_folder()
        path = self._path()
        written = path.with_suffix(".new")
        written.unlink(missing_ok=True)
        descriptor = os.open(written, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "w", encoding="utf-8") as file:
            file.write(json.dumps({lamp: asdict(p) for lamp, p in pairings.items()}, indent=2))
        written.replace(path)

    def pairing(self, lamp: str | None = None) -> Pairing | None:
        """The pairing with `lamp`, or the only one there is."""
        pairings = self.pairings()
        if lamp is not None:
            return pairings.get(lamp)
        return next(iter(pairings.values())) if len(pairings) == 1 else None
