"""The Skills Store's developer API: sign in with an emailed code, upload an
app, read its review, and ask for it to go public (#7415; Backend3's spec of
20 Nov). Every host, path and error code the SDK knows is in this file's
constants, so a change on the Store's side is a change here only.

    store = Store.signed_in()               # the token `lamp login` kept
    app = store.submit(package.data, title="Gym coach")
    store.app(app["id"])["review_status"]   # pending, approved or rejected

The token is the developer's Autonomous account's, kept in
~/.config/lampos/store.json (or $LAMPOS_HOME), 0600, one per account host,
so a staging token never goes to production. `LAMP_STORE_TOKEN` wins over
the file. No password is ever asked or sent: the sign-in is the code the
account's email gets.
"""

from __future__ import annotations

import json
import os
import secrets
import struct
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .errors import LampError
from .keys import home


@dataclass(frozen=True)
class Endpoints:
    """Where the account's sign-in and the Store's developer API answer."""

    account: str
    store: str


PRODUCTION = Endpoints(
    account="https://apiv2.autonomous.ai/api/v1/customers",
    store="https://campaign-api.autonomous.ai/api/v1/skills-store/developer",
)
STAGING = Endpoints(
    account="https://apiv2.staging.autonomousdev.xyz/api/v1/customers",
    store="https://campaign-api.staging.autonomousdev.xyz/api/v1/skills-store/developer",
)
PATHS = {
    "send_code": "/send-login-verification",
    "sign_in": "/sign-in",
    "apps": "/apps",
    "app": "/apps/{id}",
    "request_public": "/apps/{id}/request-public",
}
# The Autonomous API's locale header; LAMP_STORE_LOCATION changes it.
LOCATION = "en-US"
TOKEN_FILE = "store.json"
ENV_TOKEN = "LAMP_STORE_TOKEN"
ENV_ACCOUNT = "LAMP_ACCOUNT_URL"
ENV_STORE = "LAMP_STORE_URL"

MOST_PACKAGE_BYTES = 18 * 1024 * 1024
MOST_ART_BYTES = 2 * 1024 * 1024
ICON_SIZE = (1024, 1024)
COVER_SIZE = (1600, 900)
MOST_TITLE = 100
MOST_DESCRIPTION = 1000

# Every error code the Store's spec lists, with what the developer can do.
ERRORS: dict[str, str] = {
    "authentication_error": "not signed in, or the sign-in ran out: run `lamp login` again",
    "device_key_refused": "that is a lamp's key, not a developer's sign-in: run `lamp login` with your Autonomous account",
    "developer_app_only": "this is a skill (format 1): a skill goes on through the lamp (`lamp skill push`), not the Store's app upload",
    "lampos_package_invalid": "the Store's check refused the package",
    "skill_name_exists": "that app name belongs to another account: change `name` in app.json",
    "app_not_found": "no app of yours has that id (`lamp app status` lists yours)",
    "app_not_approved": "only an approved app can ask to go public: wait for the review",
    "app_already_public": "this app is public already",
    "developer_daily_limit": "today's 10 accepted uploads for this account are used: try again tomorrow",
    "developer_attempt_limit": "today's 30 upload attempts for this account are used: try again tomorrow",
    "developer_public_limit": "today's 3 asks to go public for this account are used: try again tomorrow",
}
NOT_SIGNED_IN = "not signed in to the Store: run `lamp login` (or set LAMP_STORE_TOKEN)"


class StoreError(LampError):
    """The Store or the sign-in refused: `code` is its `error_code`, `why`
    its words with what to do."""

    code = "store"

    def __init__(self, why: str, *, code: str | None = None, status: int | None = None):
        super().__init__(why, code=code)
        self.status = status


def endpoints(*, staging: bool = False, store: str | None = None, account: str | None = None) -> Endpoints:
    """Production by default; `staging`, then the URLs given or in
    LAMP_STORE_URL and LAMP_ACCOUNT_URL, change it."""
    base = STAGING if staging else PRODUCTION
    return Endpoints(
        account=(account or os.environ.get(ENV_ACCOUNT) or base.account).rstrip("/"),
        store=(store or os.environ.get(ENV_STORE) or base.store).rstrip("/"),
    )


def _token_path() -> Path:
    return home() / TOKEN_FILE


def _tokens() -> dict[str, Any]:
    try:
        kept = json.loads(_token_path().read_text("utf-8"))
    except (OSError, ValueError):
        return {}
    return kept if isinstance(kept, dict) else {}


def _write(kept: dict[str, Any]) -> Path:
    """The kept tokens, written whole to a 0600 file in a 0700 folder."""
    path = _token_path()
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = path.with_suffix(".tmp")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as out:
        json.dump(kept, out, indent=2)
    os.chmod(temporary, 0o600)
    os.replace(temporary, path)
    return path


def save_token(token: str, where: Endpoints, email: str | None = None) -> Path:
    """Keep `token` for `where`'s account host."""
    kept = _tokens()
    kept[where.account] = {"token": token, "email": email, "store": where.store}
    return _write(kept)


def forget_token(where: Endpoints) -> bool:
    """Drop the token kept for `where`; whether there was one."""
    kept = _tokens()
    if kept.pop(where.account, None) is None:
        return False
    if kept:
        _write(kept)
    else:
        _token_path().unlink(missing_ok=True)
    return True


def kept_token(where: Endpoints) -> tuple[str | None, str | None]:
    """The token for `where` and the email it is for: LAMP_STORE_TOKEN, or
    the one `lamp login` kept for that account host."""
    if os.environ.get(ENV_TOKEN):
        return os.environ[ENV_TOKEN], None
    entry = _tokens().get(where.account)
    if isinstance(entry, dict) and isinstance(entry.get("token"), str):
        return entry["token"], entry.get("email")
    return None, None


def png_size(data: bytes) -> tuple[int, int] | None:
    """A PNG's width and height from its IHDR, or None for a file that is
    not a PNG."""
    if len(data) < 24 or data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        return None
    return struct.unpack(">II", data[16:24])


def art_error(what: str, data: bytes, size: tuple[int, int]) -> str | None:
    if png_size(data) is None:
        return f"{what}: not a PNG"
    if png_size(data) != size:
        width, height = png_size(data)  # type: ignore[misc]
        return f"{what}: {width}x{height}; the Store takes exactly {size[0]}x{size[1]}"
    if len(data) > MOST_ART_BYTES:
        return f"{what}: {len(data)} bytes; the Store takes 2 MB at most"
    return None


def _multipart(fields: dict[str, str], files: dict[str, tuple[str, str, bytes]]) -> tuple[bytes, str]:
    boundary = "lampos-" + secrets.token_hex(16)
    parts: list[bytes] = []
    for name, value in fields.items():
        parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n'.encode() + value.encode() + b"\r\n")
    for name, (filename, kind, data) in files.items():
        parts.append(
            f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"; filename="{filename}"\r\nContent-Type: {kind}\r\n\r\n'.encode()
            + data
            + b"\r\n"
        )
    parts.append(f"--{boundary}--\r\n".encode())
    return b"".join(parts), f"multipart/form-data; boundary={boundary}"


class Store:
    """One developer's calls to the Store. `token` is the account's bearer
    token; without one, only `send_code` and `sign_in` answer."""

    def __init__(self, token: str | None = None, *, where: Endpoints | None = None, timeout: float = 120.0):
        self.token = token
        self.where = where or endpoints()
        self.timeout = timeout

    @classmethod
    def signed_in(cls, *, where: Endpoints | None = None, timeout: float = 120.0) -> Store:
        """With the token kept for `where` (LAMP_STORE_TOKEN first); a
        StoreError when there is none, before anything is sent."""
        where = where or endpoints()
        token, _ = kept_token(where)
        if not token:
            raise StoreError(NOT_SIGNED_IN, code="authentication_error")
        return cls(token, where=where, timeout=timeout)

    def _call(self, method: str, url: str, *, body: bytes | None = None, kind: str | None = None, auth: bool = True) -> Any:
        headers = {"Accept": "application/json", "Location": os.environ.get("LAMP_STORE_LOCATION", LOCATION), "User-Agent": "lampos-sdk"}
        if kind:
            headers["Content-Type"] = kind
        if auth:
            if not self.token:
                raise StoreError(NOT_SIGNED_IN, code="authentication_error")
            headers["Authorization"] = f"Bearer {self.token}"
        request = urllib.request.Request(url, data=body, method=method, headers=headers)
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as answer:
                status, text = answer.status, answer.read()
        except urllib.error.HTTPError as error:
            status, text = error.code, error.read()
        except (urllib.error.URLError, OSError) as error:
            reason = getattr(error, "reason", error)
            raise StoreError(f"no answer from {urllib.parse.urlsplit(url).netloc} ({reason})", code="unreachable") from None
        try:
            answer_json = json.loads(text or b"{}")
        except ValueError:
            raise StoreError(f"the Store answered HTTP {status} with no JSON", code="store", status=status) from None
        if isinstance(answer_json, dict) and answer_json.get("status") == 1:
            return answer_json.get("data")
        raise self._refusal(answer_json, status)

    @staticmethod
    def _refusal(answer: Any, status: int) -> StoreError:
        answer = answer if isinstance(answer, dict) else {}
        code = answer.get("error_code") or ("authentication_error" if status == 401 else "store")
        said = answer.get("error_message") or answer.get("message") or f"HTTP {status}"
        known = ERRORS.get(code)
        if code == "lampos_package_invalid":
            # The Store's reason is the reader's own words; they lead.
            return StoreError(f"{said} ({known})", code=code, status=status)
        return StoreError(f"{known} (the Store: {said})" if known else str(said), code=code, status=status)

    # Sign-in: the account's emailed code, never a password.
    def send_code(self, email: str) -> None:
        body = json.dumps({"email": email}).encode()
        self._call("POST", self.where.account + PATHS["send_code"], body=body, kind="application/json", auth=False)

    def sign_in(self, email: str, code: str) -> str:
        """The account's token for the emailed code; also kept as `self.token`."""
        body = json.dumps({"grant_type": "otp", "email": email, "otp": code}).encode()
        data = self._call("POST", self.where.account + PATHS["sign_in"], body=body, kind="application/json", auth=False)
        token = data.get("access_token") if isinstance(data, dict) else None
        if not isinstance(token, str) or not token:
            raise StoreError("the sign-in answered no access_token", code="authentication_error")
        self.token = token
        return token

    # The developer's apps.
    def submit(
        self,
        package: bytes,
        *,
        filename: str = "app.tar",
        icon: bytes | None = None,
        cover: bytes | None = None,
        title: str | None = None,
        description: str | None = None,
        category: str | None = None,
    ) -> dict[str, Any]:
        """Upload an unsigned format-2 tar; the Store's app, `review_status`
        "pending" and hidden. The same bytes again answer the same app. The
        art and listing's limits are checked here first, so a wrong one costs
        no attempt of the day's 30."""
        if len(package) > MOST_PACKAGE_BYTES:
            raise StoreError(f"the package is {len(package)} bytes; the Store takes 18 MiB at most", code="bad_request")
        for what, art, size in (("icon", icon, ICON_SIZE), ("cover", cover, COVER_SIZE)):
            if art is not None and (why := art_error(what, art, size)):
                raise StoreError(why, code="bad_request")
        if title is not None and not 0 < len(title) <= MOST_TITLE:
            raise StoreError(f"title: 1 to {MOST_TITLE} characters", code="bad_request")
        if description is not None and len(description) > MOST_DESCRIPTION:
            raise StoreError(f"description: at most {MOST_DESCRIPTION} characters of plain text", code="bad_request")
        fields = {k: v for k, v in (("title", title), ("description", description), ("category", category)) if v is not None}
        files = {"file": (filename, "application/x-tar", package)}
        if icon is not None:
            files["icon"] = ("icon.png", "image/png", icon)
        if cover is not None:
            files["cover"] = ("cover.png", "image/png", cover)
        body, kind = _multipart(fields, files)
        return self._call("POST", self.where.store + PATHS["apps"], body=body, kind=kind)

    def apps(self, *, page: int = 1, limit: int = 50) -> tuple[list[dict[str, Any]], int]:
        """This account's apps and how many there are."""
        query = urllib.parse.urlencode({"page": page, "limit": limit})
        data = self._call("GET", f"{self.where.store}{PATHS['apps']}?{query}") or {}
        return list(data.get("data") or []), int(data.get("total") or 0)

    def app(self, app_id: str | int) -> dict[str, Any]:
        return self._call("GET", self.where.store + PATHS["app"].format(id=urllib.parse.quote(str(app_id), safe="")))

    def request_public(self, app_id: str | int) -> dict[str, Any] | None:
        """Ask the reviewers to make an approved app public; asking again
        changes nothing."""
        return self._call("POST", self.where.store + PATHS["request_public"].format(id=urllib.parse.quote(str(app_id), safe="")))


def describe(app: dict[str, Any]) -> str:
    """One app's line: id, name, version, review, visibility, the note."""
    review = app.get("review_status") or "?"
    shown = app.get("visibility") or "hidden"
    asked = ", public asked" if app.get("public_requested") else ""
    note = f"\n  reviewer: {app['review_note']}" if app.get("review_note") else ""
    return f"{app.get('id')}\t{app.get('name')} {app.get('version')}\t{review}\t{shown}{asked}{note}"
