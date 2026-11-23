"""An app package of this computer's own put on its own lamp (`app.push`, `app.list`, `app.remove`; docs/apps.md):
checked here before a byte is sent, sent in parts as a skill is, listed and taken off, against the fake lamp."""

from __future__ import annotations

import asyncio
import hashlib
import io
import json
import os
import sys
import tarfile
from pathlib import Path

import pytest

from lampos import Keyring, Lamp, LampError, errors
from lampos.testing import FakeLamp


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, timeout=20))


async def paired(fake: FakeLamp, folder: Path) -> Keyring:
    ring = Keyring(folder)
    await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, name="test laptop", keyring=ring)
    return ring


APP_PY = b"from lampos import Lamp\n\nprint('hello')\n"


def app_tar(
    folder: Path,
    name: str = "english-lessons",
    *,
    version: str = "1.2.0",
    grants: tuple[str, ...] = ("speak",),
    signed: bool = False,
    extra: bytes = b"",
    gzip: bool = False,
) -> Path:
    """A small valid unsigned format 2 package (docs/apps.md): `lamp-skill.json` beside one folder named for the app,
    a ustar tar. `signed` adds a `lamp-skill.sig`; `extra` an asset-sized file to make it big."""
    files = {"src/lamp_learn/app.py": APP_PY}
    if extra:
        files["src/lamp_learn/blob.py"] = extra
    manifest = {
        "format": 2,
        "kind": "app",
        "name": name,
        "title": "English lessons",
        "version": version,
        "min_build": 1,
        "runtime": {"name": "python3", "entry": "lamp_learn.app"},
        "grants": list(grants),
        "net": False,
        "logs": True,
        "phrases": [],
        "triggers": [],
        "settings": [],
        "skills": [],
        "assets": [],
        "files": [{"path": p, "size": len(d), "sha256": hashlib.sha256(d).hexdigest()} for p, d in sorted(files.items())],
    }
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz" if gzip else "w", format=tarfile.USTAR_FORMAT) as tar:
        entries = {"lamp-skill.json": json.dumps(manifest, indent=2).encode()}
        if signed:
            entries["lamp-skill.sig"] = json.dumps({"key": "ab" * 8, "signature": "c2ln"}).encode()
        entries.update({f"{name}/{p}": d for p, d in sorted(files.items())})
        for path, data in entries.items():
            info = tarfile.TarInfo(path)
            info.size, info.mode = len(data), 0o644
            tar.addfile(info, io.BytesIO(data))
    out = folder / f"{name}-{version}{'-signed' if signed else ''}.tar"
    folder.mkdir(parents=True, exist_ok=True)
    out.write_bytes(buffer.getvalue())
    return out


def test_an_app_is_pushed_in_parts_listed_replaced_and_removed(tmp_path, monkeypatch):
    from lampos import push

    monkeypatch.setattr(push, "PART_BYTES", 2000)  # several parts from a small package

    async def go():
        async with FakeLamp(grants=("speak", "light", "skills")) as fake:
            ring = await paired(fake, tmp_path / "keys")
            async with await Lamp.connect(keyring=ring) as lamp:
                package = app_tar(tmp_path / "pkg")
                app = await lamp.push_app(package)
                assert app == {
                    "name": "english-lessons", "title": "English lessons", "version": "1.2.0",
                    "sha256": hashlib.sha256(package.read_bytes()).hexdigest(), "grants": ["speak"],
                    "net": False, "logs": True, "updated": False, "started": True, "from": "computer",
                }
                pushes = fake.asked("app.push")
                assert len(pushes) > 1 and [p["part"] for p in pushes] == list(range(len(pushes)))
                assert all(p["parts"] == len(pushes) and p["name"] == "english-lessons" for p in pushes)
                (listed,) = await lamp.apps()
                assert sorted(listed) == ["grants", "logs", "name", "net", "sha256", "stopped", "title", "version"]
                assert (listed["version"], listed["stopped"]) == ("1.2.0", False)
                # A re-push of the name replaces it, any version (an older one too).
                again = await lamp.push_app(app_tar(tmp_path / "pkg2", version="1.0.0", grants=("speak", "light")))
                assert (again["version"], again["updated"], again["grants"]) == ("1.0.0", True, ["speak", "light"])
                assert [(a["name"], a["version"]) for a in await lamp.apps()] == [("english-lessons", "1.0.0")]
                assert (await lamp.remove_app("english-lessons"))["removed"] == "english-lessons"
                assert await lamp.apps() == []
                with pytest.raises(errors.BY_CODE["refused"], match="not one of this computer's pushed apps"):
                    await lamp.remove_app("english-lessons")

    run(go())


def test_an_app_folder_is_checked_packed_and_pushed(tmp_path):
    """A developer's folder (`app.json` beside `src/`), packed as `lamp app pack` packs it; a folder the check
    refuses is refused before a byte is sent."""
    folder = tmp_path / "desk-hello"
    (folder / "src" / "desk_hello").mkdir(parents=True)
    (folder / "src" / "desk_hello" / "app.py").write_text("print('hi')\n")
    source = {"title": "Desk hello", "version": "0.3.0", "min_build": 1, "runtime": {"name": "python3", "entry": "desk_hello.app"},
              "grants": ["speak"], "triggers": []}
    (folder / "app.json").write_text(json.dumps(source))

    async def go():
        from lampos import app as apps

        async with FakeLamp(grants=("speak", "skills")) as fake:
            ring = await paired(fake, tmp_path / "keys")
            async with await Lamp.connect(keyring=ring) as lamp:
                app = await lamp.push_app(folder)
                assert (app["name"], app["version"], app["sha256"]) == ("desk-hello", "0.3.0", apps.pack(folder).sha256)
                (folder / "app.json").write_text(json.dumps({**source, "version": "one"}))
                sent = len(fake.asked("app.push"))
                with pytest.raises(errors.BY_CODE["bad_request"]):
                    await lamp.push_app(folder)
                assert len(fake.asked("app.push")) == sent

    run(go())


def test_an_app_push_without_the_skills_grant_or_beyond_the_pairing_is_refused_before_a_byte_is_sent(tmp_path):
    async def go():
        async with FakeLamp(grants=("speak",)) as fake:
            ring = await paired(fake, tmp_path / "keys")
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(errors.BY_CODE["no_grant"], match="not granted: skills"):
                    await lamp.push_app(app_tar(tmp_path / "a"))
            assert fake.asked("app.push") == []
        async with FakeLamp(grants=("speak", "skills")) as fake:
            ring = await paired(fake, tmp_path / "keys2")
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(errors.BY_CODE["no_grant"], match="not granted: camera"):
                    await lamp.push_app(app_tar(tmp_path / "b", grants=("speak", "camera")))
                # `remember` is an installed Store app's alone: never a pushed app's.
                with pytest.raises(errors.BY_CODE["no_grant"], match="not granted: remember"):
                    await lamp.push_app(app_tar(tmp_path / "c", grants=("speak", "remember")))
            assert fake.asked("app.push") == []

    run(go())


def test_a_signed_oversize_gzip_or_skill_package_is_refused_before_a_byte_is_sent(tmp_path):
    async def go():
        async with FakeLamp(grants=("speak", "skills")) as fake:
            ring = await paired(fake, tmp_path / "keys")
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(errors.BY_CODE["refused"], match="it carries lamp-skill.sig: a pushed app is unsigned"):
                    await lamp.push_app(app_tar(tmp_path / "a", signed=True))
                big = app_tar(tmp_path / "b", extra=os.urandom(4 << 20))
                with pytest.raises(errors.BY_CODE["bad_request"], match="a pushed app is 4 MiB at most"):
                    await lamp.push_app(big)
                with pytest.raises(errors.BY_CODE["bad_request"], match="a gzip: a package is a plain tar"):
                    await lamp.push_app(app_tar(tmp_path / "c", gzip=True))
                skill = tmp_path / "skill.tar"
                with tarfile.open(skill, "w", format=tarfile.USTAR_FORMAT) as tar:
                    data = json.dumps({"format": 1, "name": "desk-wave", "version": "0.1.0", "files": []}).encode()
                    info = tarfile.TarInfo("lamp-skill.json")
                    info.size = len(data)
                    tar.addfile(info, io.BytesIO(data))
                with pytest.raises(errors.BY_CODE["bad_request"], match="format 1 is a skill"):
                    await lamp.push_app(skill)
                with pytest.raises(errors.BY_CODE["bad_request"], match="an app's name"):
                    await lamp.push_app(app_tar(tmp_path / "d", name="English"))
            assert fake.asked("app.push") == []

    run(go())


def test_the_lamp_refuses_an_app_push_in_its_own_words(tmp_path):
    async def go():
        for options, why in (
            ({"store_apps": False}, "apps are off on this lamp"),
            ({"apps_helper": False}, "app push is not on this lamp"),
        ):
            async with FakeLamp(grants=("speak", "skills"), **options) as fake:
                ring = await paired(fake, tmp_path / why)
                async with await Lamp.connect(keyring=ring) as lamp:
                    with pytest.raises(errors.BY_CODE["refused"], match=why):
                        await lamp.push_app(app_tar(tmp_path / why))
                    with pytest.raises(errors.BY_CODE["refused"], match=why):
                        await lamp.apps()
        async with FakeLamp(grants=("speak", "skills")) as fake:
            fake.store_app_names.add("english-lessons")
            ring = await paired(fake, tmp_path / "store")
            async with await Lamp.connect(keyring=ring) as lamp:
                with pytest.raises(LampError, match="a Store app has that name"):
                    await lamp.push_app(app_tar(tmp_path / "store"))

    run(go())


def test_lamp_app_push_list_and_remove_print_as_the_skill_commands_do(tmp_path):
    async def lamp_cli(home: Path, *args: str) -> tuple[int, str, str]:
        process = await asyncio.create_subprocess_exec(
            sys.executable, "-m", "lampos.cli", *args,
            env={**os.environ, "LAMPOS_HOME": str(home)},
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        )
        out, err = await process.communicate()
        return process.returncode, out.decode().strip(), err.decode().strip()

    async def go():
        async with FakeLamp(grants=("speak", "skills")) as fake:
            home = tmp_path / "home"
            await paired(fake, home)
            package = str(app_tar(tmp_path / "pkg"))
            assert await lamp_cli(home, "app", "push", package) == (0, "english-lessons 1.2.0 pushed, running", "")
            assert (await lamp_cli(home, "app", "push", package))[1] == "english-lessons 1.2.0 pushed, replacing the one before, running"
            assert (await lamp_cli(home, "app", "list"))[1] == "english-lessons\t1.2.0\trunning"
            assert (await lamp_cli(home, "app", "remove", "english-lessons"))[1] == "english-lessons removed"
            assert (await lamp_cli(home, "app", "list"))[1] == ""
            signed = await lamp_cli(home, "app", "push", str(app_tar(tmp_path / "s", signed=True)))
            assert (signed[0], signed[2]) == (1, "lamp: refused: it carries lamp-skill.sig: a pushed app is unsigned")
            assert fake.asked("app.remove") == [{"name": "english-lessons"}]

    run(go())
