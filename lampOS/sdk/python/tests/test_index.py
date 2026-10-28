"""Shared-skill indexes (decision 4, #3997): a signed list of signed packages,
checked on this computer, then installed as any shared package against the
fake lamp."""

from __future__ import annotations

import base64
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from lampos import Keyring, Lamp, LampError
from lampos import index, push
from lampos.testing import FakeLamp

from test_client import paired, run

BASE = "https://skills.example/desk/"


def _package(tmp: Path, name: str, rules: str, author) -> bytes:
    folder = tmp / "src" / name
    folder.mkdir(parents=True)
    (folder / "SKILL.md").write_text(f"---\nname: {name}\ndescription: A test skill.\n---\n")
    (folder / "skill.toml").write_text(rules)
    return push.pack(folder, author=author).data


@pytest.fixture
def made(tmp_path):
    publisher = Keyring(tmp_path / "publisher").author_key(make=True)
    author = Keyring(tmp_path / "author").author_key(make=True)
    wave = _package(tmp_path, "desk-wave", 'version = "1.0.0"\n[permissions]\nlight = true\nspeak = true\n', author)
    nod = _package(tmp_path, "desk-nod", 'version = "0.2.0"\n[permissions]\nbody = true\n', author)
    data = index.make_index("desk-things", [wave, nod], BASE, 3, publisher)
    return {"publisher": publisher, "author": author, "wave": wave, "nod": nod, "index": data}


def _resign(data: bytes, listing: dict, key) -> bytes:
    """The same index with another listing, signed by `key`."""
    text = json.dumps(listing, separators=(",", ":"))
    public = push._public_bytes(key)
    return json.dumps({
        "format": 1, "index": text, "key": push.fingerprint(public),
        "public": base64.b64encode(public).decode(), "signature": base64.b64encode(key.sign(text.encode())).decode(),
    }).encode()


def test_an_index_lists_each_signed_package_as_it_is(made):
    listing = index.open_index(made["index"])
    assert (listing.name, listing.seq, listing.publisher) == ("desk-things", 3, push.fingerprint(push._public_bytes(made["publisher"])))
    wave = next(e for e in listing.skills if e.name == "desk-wave")
    assert (wave.version, wave.size, wave.url, wave.asks) == ("1.0.0", len(made["wave"]), BASE + "desk-wave-1.0.0.tar", ["light", "speak"])
    assert wave.author == push.open_package(made["wave"]).author
    assert index.check_package(wave, made["wave"]).name == "desk-wave"


def test_a_changed_or_unsigned_index_is_refused(made):
    outer = json.loads(made["index"])
    changed = dict(outer, index=outer["index"].replace('"seq":3', '"seq":4'))
    with pytest.raises(LampError, match="changed after it was signed"):
        index.open_index(json.dumps(changed).encode())
    with pytest.raises(LampError, match="signed by its publisher"):
        index.open_index(json.dumps({"format": 1, "index": outer["index"]}).encode())
    with pytest.raises(LampError, match="more than"):
        index.open_index(b" " * (push.PACKAGE_MAX + 1))


def test_the_pinned_key_and_the_last_seq_hold(made, tmp_path):
    listing = json.loads(json.loads(made["index"])["index"])
    other = Keyring(tmp_path / "other").author_key(make=True)
    pinned = index.open_index(made["index"]).public
    with pytest.raises(LampError, match="remove it and add it again"):
        index.open_index(_resign(made["index"], listing, other), pinned=pinned)
    older = _resign(made["index"], dict(listing, seq=2), made["publisher"])
    with pytest.raises(LampError, match="went back from seq 3 to 2"):
        index.open_index(older, pinned=pinned, last_seq=3)
    assert index.open_index(_resign(made["index"], dict(listing, seq=3), made["publisher"]), pinned=pinned, last_seq=3).seq == 3


@pytest.mark.parametrize(
    "change, says",
    [
        ({"url": "http://skills.example/desk-wave.tar"}, "https only"),
        ({"author": None}, "signed packages only"),
        ({"name": "../wave"}, "not a skill's name"),
        ({"size": 2 << 20}, "size is 1 to"),
    ],
)
def test_an_entry_out_of_shape_is_refused(made, change, says):
    listing = json.loads(json.loads(made["index"])["index"])
    listing["skills"][0].update(change)
    with pytest.raises(LampError, match=says):
        index.open_index(_resign(made["index"], listing, made["publisher"]))


def test_a_package_that_isnt_the_entrys_is_refused(made, tmp_path):
    listing = index.open_index(made["index"])
    wave = next(e for e in listing.skills if e.name == "desk-wave")
    with pytest.raises(LampError, match="not the one the index lists"):
        index.check_package(wave, made["nod"])
    # Same bytes as listed, but the index says another author or fewer asks.
    stranger = Keyring(tmp_path / "stranger").author_key(make=True)
    for field, value, says in (("author", push.fingerprint(push._public_bytes(stranger)), "the index says key"),
                               ("asks", ["light"], "the index says \\[light\\]")):
        lying = index.Entry(**{**wave.__dict__, field: value})
        with pytest.raises(LampError, match=says):
            index.check_package(lying, made["wave"])


def test_install_by_name_checks_the_entry_then_pushes_with_asks(made, tmp_path):
    served = {"https://lan.example/lamp-index.json": made["index"], BASE + "desk-wave-1.0.0.tar": made["wave"], BASE + "desk-nod-0.2.0.tar": made["nod"]}
    kept = index.Indexes(tmp_path / "home")
    kept.keep("https://lan.example/lamp-index.json", index.open_index(made["index"]))

    async def go():
        async with FakeLamp(grants=("light", "speak", "skills")) as fake:
            ring = await paired(fake, tmp_path / "keys")
            async with await Lamp.connect(keyring=ring) as lamp:
                listing, entry, data = index.find(kept, "desk-wave", fetch=served.__getitem__)
                skill = await lamp.install_package(data)
                assert (skill["name"], skill["author"], skill["asks"]) == ("desk-wave", entry.author, ["light", "speak"])
                assert fake.asked("skill.push")[0]["asks"] == ["light", "speak"]
                # A package swapped at the URL never reaches the lamp.
                swapped = dict(served, **{BASE + "desk-wave-1.0.0.tar": made["nod"]})
                with pytest.raises(LampError, match="not the one the index lists"):
                    index.find(kept, "desk-wave", fetch=swapped.__getitem__)
                with pytest.raises(LampError, match="in no index"):
                    index.find(kept, "desk-lift", fetch=served.__getitem__)

    run(go())


def test_lamp_skill_index_make_add_list_browse_and_remove(made, tmp_path):
    env = {**os.environ, "LAMPOS_HOME": str(tmp_path / "home")}

    def lamp(*args: str, typed: str = "") -> tuple[int, str]:
        done = subprocess.run([sys.executable, "-m", "lampos.cli", *args], env=env, input=typed, capture_output=True, text=True)
        return done.returncode, (done.stdout + done.stderr).strip()

    packages = []
    for name in ("wave", "nod"):
        path = tmp_path / f"{name}.tar"
        path.write_bytes(made[name])
        packages.append(str(path))
    out = tmp_path / "lamp-index.json"
    assert lamp("skill", "index", "make", "desk-things", *packages, "--url-base", BASE, "--seq", "1", "--out", str(out))[0] == 1  # no author key yet
    lamp("skill", "keygen")
    code, said = lamp("skill", "index", "make", "desk-things", *packages, "--url-base", BASE, "--seq", "1", "--out", str(out))
    assert code == 0 and "published by key" in said and "not the Store" in said and "2 skills" in said, said
    code, said = lamp("skill", "index", "add", str(out), typed="n\n")
    assert code == 1 and said.endswith("not added")
    code, said = lamp("skill", "index", "add", str(out), typed="y\n")
    assert code == 0 and said.endswith("index desk-things added"), said
    assert lamp("skill", "index", "list")[1].startswith("desk-things\tkey ")
    code, said = lamp("skill", "browse", "wave")
    assert code == 0 and said.startswith("desk-wave\t1.0.0\tsigned by key ") and said.endswith("\tasks light, speak\tindex desk-things"), said
    assert lamp("skill", "index", "remove", "desk-things")[1] == "index desk-things removed"
    assert lamp("skill", "browse")[1] == ""
