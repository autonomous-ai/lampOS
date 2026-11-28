"""`lamp app check`, `pack`, `submit`, `status`, `request-public`, `login`
and the studio, against a fake Store on this computer that answers as
Backend3's spec says (#7415, 20 Nov)."""

from __future__ import annotations

import base64
import email.parser
import email.policy
import io
import json
import os
import stat
import struct
import tarfile
import threading
import urllib.request
import wave
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

from lampos import app as apps
from lampos import store as stores
from lampos.cli import main
from lampos.studio import Studio

OTP = "123456"
TOKEN = "tok-dev-1"


def wav(path: Path, frames: int = 2400, level: int = 500) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(24_000)
        out.writeframes(struct.pack("<h", level) * frames)


def png(width: int, height: int) -> bytes:
    def chunk(kind: bytes, body: bytes) -> bytes:
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    raw = b"".join(b"\0" + b"\0\0\0" * width for _ in range(height))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def make_app(root: Path, name: str = "gym-coach", **manifest) -> Path:
    folder = root / name
    (folder / "src" / "gym_coach").mkdir(parents=True)
    (folder / "src" / "gym_coach" / "app.py").write_text("print('hi')\n")
    wav(folder / "assets" / "ok.wav")
    skill = folder / "skills" / "gym-tips"
    skill.mkdir(parents=True)
    (skill / "SKILL.md").write_text("---\nname: gym-tips\ndescription: Tips for the gym.\n---\nNotes.\n")
    (skill / "skill.toml").write_text('version = "1.0.0"\n')
    (folder / "README.md").write_text("not packed\n")
    source = {
        "title": "Gym coach",
        "version": "1.0.0",
        "min_build": 143,
        "runtime": {"name": "python3", "entry": "gym_coach.app"},
        "grants": ["speak", "hear"],
        "triggers": ["start my workout now"],
        "settings": [{"key": "reps", "type": "number", "label": "Reps", "min": 1, "max": 50, "default": 10}],
    }
    source.update(manifest)
    (folder / "app.json").write_text(json.dumps(source))
    return folder


class FakeStore:
    """The account's sign-in and the Store's developer API on one port."""

    def __init__(self):
        self.requests: list[tuple[str, str, dict, bytes]] = []
        self.apps: dict[str, dict] = {}
        self.refuse_with: tuple[str, str] | None = None
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def _answer(self, status, body):
                data = json.dumps(body).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def _fail(self, status, code, message):
                self._answer(status, {"status": -1, "error_code": code, "error_message": message})

            def _handle(self, method):
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                fake.requests.append((method, self.path, dict(self.headers), body))
                path = self.path.split("?")[0]
                if path == "/account/send-login-verification":
                    return self._answer(200, {"status": 1, "data": None})
                if path == "/account/sign-in":
                    asked = json.loads(body)
                    if asked.get("grant_type") != "otp" or asked.get("otp") != OTP:
                        return self._fail(400, "authentication_error", "invalid otp")
                    return self._answer(200, {"status": 1, "data": {"access_token": TOKEN}})
                if self.headers.get("Authorization") != f"Bearer {TOKEN}":
                    return self._fail(401, "authentication_error", "unauthorized")
                if path == "/store/apps" and method == "POST":
                    if fake.refuse_with:
                        return self._fail(400, *fake.refuse_with)
                    message = email.parser.BytesParser(policy=email.policy.default).parsebytes(
                        b"Content-Type: " + self.headers["Content-Type"].encode() + b"\r\n\r\n" + body
                    )
                    parts = {part.get_param("name", header="content-disposition"): part for part in message.iter_parts()}
                    package = parts["file"].get_content()
                    with tarfile.open(fileobj=io.BytesIO(package)) as tar:
                        manifest = json.load(tar.extractfile("lamp-skill.json"))
                    app = {
                        "id": str(len(fake.apps) + 1),
                        "name": manifest["name"],
                        "title": parts["title"].get_content() if "title" in parts else manifest["title"],
                        "version": manifest["version"],
                        "review_status": "pending",
                        "review_note": None,
                        "visibility": "unlisted",
                        "public_requested": False,
                        "fields": sorted(parts),
                    }
                    fake.apps[app["id"]] = app
                    return self._answer(200, {"status": 1, "data": app})
                if path == "/store/apps" and method == "GET":
                    listed = list(fake.apps.values())
                    return self._answer(200, {"status": 1, "data": {"data": listed, "total": len(listed)}})
                parts = path.split("/")
                if len(parts) >= 4 and parts[2] == "apps":
                    app = fake.apps.get(parts[3])
                    if app is None:
                        return self._fail(404, "app_not_found", "not found")
                    if len(parts) == 4:
                        return self._answer(200, {"status": 1, "data": app})
                    if parts[4] == "request-public":
                        if app["review_status"] != "approved":
                            return self._fail(400, "app_not_approved", "not approved")
                        app["public_requested"] = True
                        return self._answer(200, {"status": 1, "data": app})
                return self._fail(404, "not_found", path)

            def do_GET(self):  # noqa: N802
                self._handle("GET")

            def do_POST(self):  # noqa: N802
                self._handle("POST")

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        base = f"http://127.0.0.1:{self.server.server_address[1]}"
        self.where = stores.Endpoints(account=base + "/account", store=base + "/store")
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def args(self) -> list[str]:
        return ["--store", self.where.store, "--account", self.where.account]

    def close(self):
        self.server.shutdown()
        self.server.server_close()


@pytest.fixture
def fake(tmp_path, monkeypatch):
    monkeypatch.setenv("LAMPOS_HOME", str(tmp_path / "home"))
    monkeypatch.delenv("LAMP_STORE_TOKEN", raising=False)
    monkeypatch.delenv("LAMP_STORE_URL", raising=False)
    monkeypatch.delenv("LAMP_ACCOUNT_URL", raising=False)
    store = FakeStore()
    yield store
    store.close()


def login(fake) -> None:
    assert main(["login", "--email", "dev@example.com", "--code", OTP, *fake.args()]) == 0


# check and pack


def test_an_app_folder_checks_and_packs_as_the_reader_takes_it(tmp_path):
    folder = make_app(tmp_path)
    report = apps.check(folder)
    assert report.ok, report.errors
    assert any("README.md: not packed" in w for w in report.warnings)
    package = apps.pack(folder)
    assert package.data == apps.pack(folder).data, "one folder packs to one SHA-256"
    with tarfile.open(fileobj=io.BytesIO(package.data)) as tar:
        names = tar.getnames()
        manifest = json.load(tar.extractfile("lamp-skill.json"))
    assert names[0] == "lamp-skill.json"
    assert names[1:] == sorted(names[1:]) and all(n.startswith("gym-coach/") for n in names[1:])
    assert (manifest["format"], manifest["kind"], manifest["name"]) == (2, "app", "gym-coach")
    assert manifest["skills"] == ["gym-tips"] and manifest["assets"] == ["assets/ok.wav"]
    assert "gym-coach/README.md" not in names
    out = tmp_path / "gym.tar"
    out.write_bytes(package.data)
    assert apps.check(out).ok, apps.check(out).errors


@pytest.mark.parametrize(
    "change, said",
    [
        ({"runtime": {"name": "python3", "entry": "gym_coach.missing"}}, "runtime.entry 'gym_coach.missing' is not in src/"),
        ({"grants": ["speak"]}, "phrases and triggers need the hear grant"),
        ({"grants": ["speak", "hear", "gmail"]}, "grant 'gmail' is not one of the door's"),
        ({"version": "1.0"}, "is not MAJOR.MINOR.PATCH"),
        ({"min_build": 0}, "min_build is the lamp build's sequence"),
        ({"triggers": ["go"]}, "fewer than 8 letters"),
        ({"settings": [{"key": "on", "type": "toggle", "label": "On", "default": 1}]}, "the default of a toggle is true or false"),
        ({"help": "x"}, "unknown field 'help'"),
    ],
)
def test_the_check_refuses_what_the_reader_refuses(tmp_path, change, said):
    report = apps.check(make_app(tmp_path, **change))
    assert any(said in line for line in report.errors), report.errors


def test_a_loud_asset_and_a_broken_companion_skill_are_refused(tmp_path):
    folder = make_app(tmp_path)
    wav(folder / "assets" / "ok.wav", level=30_000)
    (folder / "skills" / "gym-tips" / "SKILL.md").write_text("no front matter\n")
    errors = apps.check(folder).errors
    assert any("assets/ok.wav: a peak past -12 dBFS" in e for e in errors), errors
    assert any(e.startswith("skills/gym-tips: ") for e in errors), errors


def test_a_tar_changed_after_packing_is_refused(tmp_path):
    package = apps.pack(make_app(tmp_path))
    changed = package.data.replace(b"print('hi')", b"print('HI')")
    out = tmp_path / "changed.tar"
    out.write_bytes(changed)
    assert any("its size or sha256 is not what" in e for e in apps.check(out).errors)


def test_a_skill_package_is_told_to_go_through_the_lamp(tmp_path):
    from lampos.push import pack as pack_skill

    skill = tmp_path / "hello"
    skill.mkdir()
    (skill / "SKILL.md").write_text("---\nname: hello\ndescription: Hi.\n---\n")
    (skill / "skill.toml").write_text('version = "1.0.0"\n')
    out = tmp_path / "hello.tar"
    out.write_bytes(pack_skill(skill).data)
    assert any("lamp skill push" in e for e in apps.check(out).errors)


def test_cli_check_and_pack(tmp_path, capsys):
    folder = make_app(tmp_path)
    assert main(["app", "check", str(folder)]) == 0
    assert "gym-coach: ok, an app (format 2)" in capsys.readouterr().out
    out = tmp_path / "out.tar"
    assert main(["app", "pack", str(folder), "-o", str(out)]) == 0
    assert out.read_bytes() == apps.pack(folder).data
    assert main(["app", "check", str(make_app(tmp_path / "bad", min_build=0))]) == 1


# sign-in


def test_login_keeps_the_token_0600_and_never_asks_a_password(fake, tmp_path, capsys, monkeypatch):
    asked = []
    answers = iter(["dev@example.com", OTP])
    monkeypatch.setattr("builtins.input", lambda prompt: asked.append(prompt) or next(answers))
    assert main(["login", *fake.args()]) == 0
    assert asked == ["Your Autonomous account's email: ", "The code from the email: "]
    kept = tmp_path / "home" / "store.json"
    assert stat.S_IMODE(kept.stat().st_mode) == 0o600
    assert json.loads(kept.read_text())[fake.where.account]["token"] == TOKEN
    sent = [json.loads(body) for _, path, _, body in fake.requests if path.startswith("/account/")]
    assert sent == [{"email": "dev@example.com"}, {"grant_type": "otp", "email": "dev@example.com", "otp": OTP}]
    assert all("password" not in json.dumps(body) for body in sent)
    assert all(headers.get("Location") == "en-US" for _, _, headers, _ in fake.requests)
    assert main(["logout", *fake.args()]) == 0
    assert not kept.exists()


def test_a_wrong_code_is_said_and_nothing_is_kept(fake, tmp_path, capsys):
    assert main(["login", "--email", "dev@example.com", "--code", "000000", *fake.args()]) == 1
    assert "authentication_error" in capsys.readouterr().err
    assert not (tmp_path / "home" / "store.json").exists()


def test_a_staging_token_never_goes_to_production(fake, monkeypatch):
    stores.save_token("staging-token", stores.STAGING)
    assert stores.kept_token(stores.PRODUCTION) == (None, None)
    assert stores.kept_token(stores.STAGING)[0] == "staging-token"
    monkeypatch.setenv("LAMP_STORE_TOKEN", "env-token")
    assert stores.kept_token(stores.PRODUCTION)[0] == "env-token"


# submit, status, request-public


def test_submit_uploads_and_status_follows_the_review(fake, tmp_path, capsys):
    login(fake)
    folder = make_app(tmp_path)
    icon, cover = tmp_path / "icon.png", tmp_path / "cover.png"
    icon.write_bytes(png(1024, 1024))
    cover.write_bytes(png(1600, 900))
    code = main(["app", "submit", str(folder), "--icon", str(icon), "--cover", str(cover), "--title", "Gym coach", "--category", "health", *fake.args()])
    assert code == 0, capsys.readouterr()
    out = capsys.readouterr().out
    assert "1\tgym-coach 1.0.0\tpending\tunlisted" in out
    assert fake.apps["1"]["fields"] == ["category", "cover", "file", "icon", "title"]
    assert main(["app", "status", *fake.args()]) == 0
    assert "gym-coach 1.0.0\tpending" in capsys.readouterr().out
    fake.apps["1"].update(review_status="rejected", review_note="It says it counts reps; it never does.")
    assert main(["app", "status", "1", *fake.args()]) == 0
    assert "reviewer: It says it counts reps; it never does." in capsys.readouterr().out
    assert main(["app", "request-public", "1", *fake.args()]) == 1
    assert "only an approved app can ask to go public" in capsys.readouterr().err
    fake.apps["1"]["review_status"] = "approved"
    assert main(["app", "request-public", "1", *fake.args()]) == 0
    assert fake.apps["1"]["public_requested"] is True
    assert main(["app", "status", "9", *fake.args()]) == 1
    assert "no app of yours has that id" in capsys.readouterr().err


def test_the_stores_refusal_is_said_with_its_reason(fake, tmp_path, capsys):
    login(fake)
    fake.refuse_with = ("lampos_package_invalid", 'lamp-skill.json: help.en: no quotes, markup or links (found "<")')
    assert main(["app", "submit", str(make_app(tmp_path)), *fake.args()]) == 1
    err = capsys.readouterr().err
    assert 'help.en: no quotes, markup or links (found "<")' in err and "lampos_package_invalid" in err
    fake.refuse_with = ("developer_daily_limit", "limit")
    assert main(["app", "submit", str(make_app(tmp_path / "again")), *fake.args()]) == 1
    assert "today's 10 accepted uploads" in capsys.readouterr().err


# Each refusal the Python README's table lists, with the words it promises.
REFUSALS = {
    "skill_version_exists": "raise `version` in app.json",
    "skill_name_exists": "change `name` in app.json",
    "skill_name_reserved": "a lampOS built-in's: change `name` in app.json",
    "developer_app_only": "`lamp skill push`",
    "listing_invalid": "the title or description is too long, or holds markup or a control character",
    "skill_media_invalid": "icon 1024x1024, cover 1600x900",
    "developer_upload_in_progress": "already sending an upload: wait for it, then try again",
    "developer_upload_disabled": "uploads are off for now: try again later",
    "developer_upload_unavailable": "upload is unavailable for now: try again later",
}


@pytest.mark.parametrize("code", sorted(REFUSALS))
def test_each_refusal_says_what_to_do(fake, tmp_path, capsys, code):
    login(fake)
    fake.refuse_with = (code, "raw words")
    assert main(["app", "submit", str(make_app(tmp_path)), *fake.args()]) == 1
    err = capsys.readouterr().err
    assert REFUSALS[code] in err, err
    assert "(the Store: raw words)" in err


def test_no_sign_in_sends_nothing(fake, tmp_path, capsys):
    assert main(["app", "submit", str(make_app(tmp_path)), *fake.args()]) == 1
    assert "run `lamp login`" in capsys.readouterr().err
    assert fake.requests == []


def test_a_token_the_store_refuses_says_sign_in_again(fake, tmp_path, capsys):
    assert main(["login", "--token", "stale", *fake.args()]) == 0
    assert main(["app", "status", *fake.args()]) == 1
    assert "run `lamp login` again" in capsys.readouterr().err


def test_a_bad_package_is_refused_before_any_upload(fake, tmp_path, capsys):
    login(fake)
    sent = len(fake.requests)
    assert main(["app", "submit", str(make_app(tmp_path, min_build=0)), *fake.args()]) == 1
    assert "refused by the check, so nothing was sent" in capsys.readouterr().err
    icon = tmp_path / "icon.png"
    icon.write_bytes(png(512, 512))
    assert main(["app", "submit", str(make_app(tmp_path / "ok")), "--icon", str(icon), *fake.args()]) == 1
    assert "512x512; the Store takes exactly 1024x1024" in capsys.readouterr().err
    assert len(fake.requests) == sent


# the studio


def _studio_call(studio, name, body=None, *, host=None, origin=None):
    request = urllib.request.Request(
        studio.url + name,
        data=None if body is None else json.dumps(body).encode(),
        headers={"Content-Type": "application/json", **({"Host": host} if host else {}), **({"Origin": origin} if origin else {})},
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as answer:
            return answer.status, answer.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def test_the_studio_checks_signs_in_submits_and_lists(fake, tmp_path):
    studio = Studio(fake.where)
    studio.start()
    try:
        status, page = _studio_call(studio, "")
        assert status == 200 and b"Lamp app studio" in page
        # A listing with no visibility yet reads as the Store's word, as `lamp app status` says it.
        assert b"x.visibility||'unlisted'" in page and b"'hidden'" not in page
        assert stores.describe({"id": "1", "name": "a", "version": "1"}).split("\t")[3] == "unlisted"
        package = base64.b64encode(apps.pack(make_app(tmp_path)).data).decode()
        _, checked = _studio_call(studio, "api/check", {"file": package})
        assert json.loads(checked)["data"]["ok"] is True
        _, answer = _studio_call(studio, "api/submit", {"file": package})
        assert json.loads(answer)["code"] == "authentication_error"
        _studio_call(studio, "api/login/code", {"email": "dev@example.com"})
        _, answer = _studio_call(studio, "api/login", {"email": "dev@example.com", "code": OTP})
        assert json.loads(answer)["data"]["signed_in"] is True
        _, answer = _studio_call(studio, "api/submit", {"file": package, "title": "Gym coach"})
        assert json.loads(answer)["data"]["review_status"] == "pending"
        _, listed = _studio_call(studio, "api/apps")
        assert json.loads(listed)["data"]["total"] == 1
        bad = base64.b64encode(apps.pack(make_app(tmp_path / "x")).data.replace(b"print('hi')", b"print('HI')")).decode()
        _, answer = _studio_call(studio, "api/submit", {"file": bad})
        assert json.loads(answer)["data"]["refused_here"] is True
        assert len(fake.apps) == 1
    finally:
        studio.close()


def test_the_studio_answers_only_its_own_page(fake):
    studio = Studio(fake.where)
    studio.start()
    try:
        wrong_path = urllib.request.Request(f"http://127.0.0.1:{studio.port}/api/me")
        with pytest.raises(urllib.error.HTTPError):
            urllib.request.urlopen(wrong_path, timeout=10)
        assert _studio_call(studio, "api/me", host="evil.example")[0] == 404
        assert _studio_call(studio, "api/logout", {}, origin="https://evil.example")[0] == 404
        assert _studio_call(studio, "api/me")[0] == 200
    finally:
        studio.close()


# redirects and plain-http URLs (Migrate's HOLD on #8358)


def _server(status: int, headers: dict[str, str], seen: list[dict]) -> ThreadingHTTPServer:
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def _handle(self):
            seen.append(dict(self.headers))
            body = json.dumps({"status": 1, "data": {"data": [], "total": 0}}).encode()
            self.send_response(status)
            for key, value in headers.items():
                self.send_header(key, value)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        do_GET = do_POST = _handle

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


@pytest.mark.parametrize("status", [301, 302, 307, 308])
def test_a_redirect_is_refused_and_the_token_goes_nowhere_else(status):
    elsewhere_seen: list[dict] = []
    elsewhere = _server(200, {}, elsewhere_seen)
    other = f"http://127.0.0.1:{elsewhere.server_address[1]}/store/apps"
    store_seen: list[dict] = []
    redirecting = _server(status, {"Location": other}, store_seen)
    base = f"http://127.0.0.1:{redirecting.server_address[1]}"
    try:
        store = stores.Store(TOKEN, where=stores.Endpoints(account=base + "/account", store=base + "/store"), timeout=5)
        with pytest.raises(stores.StoreError) as raised:
            store.apps()
        assert raised.value.code == "store" and raised.value.status == status
        assert raised.value.why == f"the Store answered HTTP {status}"
        assert len(store_seen) == 1 and store_seen[0]["Authorization"] == f"Bearer {TOKEN}"
        assert elsewhere_seen == [], "the redirect's target got no request"
    finally:
        for server in (elsewhere, redirecting):
            server.shutdown()
            server.server_close()


@pytest.mark.parametrize(
    "url",
    ["https://store.example.com/api", "http://localhost:8080/store", "http://127.0.0.1:9/store", "http://[::1]:9/store"],
)
def test_https_or_this_computer_is_taken(url, monkeypatch):
    monkeypatch.setenv("LAMP_STORE_URL", url + "/")
    monkeypatch.setenv("LAMP_ACCOUNT_URL", url)
    assert stores.endpoints() == stores.Endpoints(account=url, store=url)


@pytest.mark.parametrize("env", ["LAMP_STORE_URL", "LAMP_ACCOUNT_URL"])
@pytest.mark.parametrize("url", ["http://store.example.com/api", "http://10.0.0.5/store", "ftp://store.example.com", "store.example.com"])
def test_plain_http_elsewhere_is_refused_before_any_call(env, url, monkeypatch, fake):
    monkeypatch.setenv(env, url)
    with pytest.raises(stores.StoreError) as raised:
        stores.endpoints()
    assert raised.value.code == "bad_url" and env in str(raised.value) and "https://" in str(raised.value)
    with pytest.raises(stores.StoreError):
        stores.Store(TOKEN, where=stores.Endpoints(account=url, store=url))
    monkeypatch.setenv("LAMP_STORE_TOKEN", TOKEN)
    assert main(["app", "status"]) == 1
    assert fake.requests == []


# an app's tools (#8386): the reader's `tools` field and its one file at the folder's top


def _tar(manifest: dict, files: dict[str, bytes]) -> bytes:
    """A package's tar as `pack` writes it, from a manifest and its files as given."""
    from lampos.push import _entry

    name = manifest["name"]
    return b"".join(
        [
            _entry("lamp-skill.json", (json.dumps(manifest, indent=2) + "\n").encode()),
            *(_entry(f"{name}/{path}", body) for path, body in sorted(files.items())),
            bytes(1024),
        ]
    )


TOOLS = json.dumps({"tools": [{"name": "start_workout", "description": "Start a workout.", "parameters": {"type": "object", "properties": {}}}]})


def test_a_manifest_naming_tools_json_at_the_top_checks_and_packs_it(tmp_path):
    folder = make_app(tmp_path, tools="tools.json")
    (folder / "tools.json").write_text(TOOLS)
    report = apps.check(folder)
    assert report.ok, report.errors
    assert not any("tools.json" in w for w in report.warnings), report.warnings
    package = apps.pack(folder)
    with tarfile.open(fileobj=io.BytesIO(package.data)) as tar:
        names = tar.getnames()
        manifest = json.load(tar.extractfile("lamp-skill.json"))
        assert tar.extractfile("gym-coach/tools.json").read() == TOOLS.encode()
    assert "gym-coach/tools.json" in names
    assert manifest["tools"] == "tools.json"
    assert "tools.json" in [entry["path"] for entry in manifest["files"]]
    out = tmp_path / "gym.tar"
    out.write_bytes(package.data)
    assert apps.check(out).ok, apps.check(out).errors


def test_a_tools_file_under_src_checks(tmp_path):
    folder = make_app(tmp_path, tools="src/gym_coach/tools.json")
    (folder / "src" / "gym_coach" / "tools.json").write_text(TOOLS)
    report = apps.check(folder)
    assert report.ok, report.errors
    with tarfile.open(fileobj=io.BytesIO(apps.pack(folder).data)) as tar:
        assert json.load(tar.extractfile("lamp-skill.json"))["tools"] == "src/gym_coach/tools.json"


@pytest.mark.parametrize(
    "path",
    ["other.json", "../tools.json", "/tools.json", "src/tools.txt", "src/.json", "src/.hidden.json", "assets/tools.json", "src", "", 5],
)
def test_a_tools_path_the_reader_refuses_is_refused(tmp_path, path):
    report = apps.check(make_app(tmp_path, tools=path))
    assert any("lamp-skill.json: tools " in line and "is not tools.json or a .json file under src/" in line for line in report.errors), report.errors


def test_tools_json_at_the_top_without_the_manifest_naming_it_is_refused(tmp_path):
    folder = make_app(tmp_path, tools="tools.json")
    (folder / "tools.json").write_text(TOOLS)
    manifest = apps.pack(folder).manifest
    files = {entry["path"]: (folder / entry["path"]).read_bytes() for entry in manifest["files"]}
    for named in (None, "src/gym_coach/tools.json"):
        changed = {k: v for k, v in manifest.items() if k != "tools"}
        if named:
            changed["tools"] = named
        out = tmp_path / f"{named is None}.tar"
        out.write_bytes(_tar(changed, files))
        errors = apps.check(out).errors
        assert "tools.json: an app's files are under src/, assets/ and skills/" in errors, errors


def test_a_folders_tools_json_is_not_packed_unless_the_manifest_names_it(tmp_path):
    folder = make_app(tmp_path)
    (folder / "tools.json").write_text(TOOLS)
    report = apps.check(folder)
    assert report.ok, report.errors
    assert any(w.startswith("tools.json: not packed") for w in report.warnings), report.warnings
    with tarfile.open(fileobj=io.BytesIO(apps.pack(folder).data)) as tar:
        assert "gym-coach/tools.json" not in tar.getnames()
