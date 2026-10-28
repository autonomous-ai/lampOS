"""`lamp`: pair this computer with a lamp, find lamps, check a pairing, or try
it all with no lamp.

    lamp find
    lamp pair 12345678 --pin <64 hex> [--host lamp-75c9.local] [--name "Dev's laptop"]
    lamp hello [LAMP_ID]
    lamp fake [--every 10] [--cycles N]
    lamp skill check FOLDER|PACKAGE...
    lamp skill keygen
    lamp skill pack FOLDER [--sign] [--out DIR]
    lamp skill push FOLDER [LAMP_ID]
    lamp skill install FILE|URL [LAMP_ID] [--yes]
    lamp skill list [LAMP_ID]
    lamp skill remove NAME [LAMP_ID]
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys
from pathlib import Path

from cryptography.hazmat.primitives import serialization

from . import schema
from .client import Lamp
from .discovery import find
from .errors import LampError
from .keys import Keyring


async def _pair(args) -> int:
    host, port = args.host, args.port
    if host is None:
        pin = args.pin.replace(" ", "").replace(":", "").lower()
        # mDNS only finds where the lamp is; its pin, checked before
        # anything is sent, is what says it is the lamp.
        lamps = [lamp for lamp in await find() if pin.startswith(lamp.fp)]
        if len(lamps) != 1:
            print(f"found {len(lamps)} lamps with that pin; say which with --host", file=sys.stderr)
            return 2
        host, port = lamps[0].host, lamps[0].port
    pairing = await Lamp.pair(args.code, host, pin=args.pin, port=port, name=args.name)
    print(f"paired with {pairing.lamp} ({pairing.name}) as {pairing.client}: {', '.join(pairing.grants)}")
    return 0


async def _fake(args) -> int:
    """A fake lamp on this computer, with every grant, paired here at once,
    that says a person arrived, a phrase heard and the person left, every
    `--every` seconds, and prints what it is asked. Try an example against
    it in another terminal, with the same LAMPOS_HOME."""
    from .testing import LAMP_CONTROL_REFUSES, FakeLamp

    refuses = {} if getattr(args, "everything", False) else LAMP_CONTROL_REFUSES
    async with FakeLamp(grants=schema.grants(), name="Fake lamp (lamp fake)", refuses=refuses) as fake:
        pairing = await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, keyring=Keyring())
        print(f"a fake lamp at {fake.host}:{fake.port}, paired as {pairing.client} in {Keyring().folder}", flush=True)
        print("run an example in another terminal with the same LAMPOS_HOME; Ctrl-C ends this", flush=True)
        shown = 0
        cycle = 0
        while args.cycles is None or cycle < args.cycles:
            cycle += 1
            for event, fields in (
                ("person.arrived", {"count": 1, "near": 0.3}),
                ("phrase.heard", {"phrase": "good night lamp"}),
                ("person.left", {"count": 0}),
            ):
                await asyncio.sleep(args.every / 3)
                await fake.emit(event, **fields)
                print(f"said {event}", flush=True)
                for request in fake.requests[shown:]:
                    print("  asked", json.dumps({k: v for k, v in request.items() if k != "id"}), flush=True)
                shown = len(fake.requests)
    return 0


async def _find(_args) -> int:
    for lamp in await find():
        print(f"{lamp.id}\t{lamp.name}\t{lamp.host}:{lamp.port}\tpin starts {lamp.fp}")
    return 0


async def _skill_check(args) -> int:
    import tempfile

    from .push import open_package
    from .skillcheck import check

    failed = 0
    for given in args.folders:
        signed = ""
        with tempfile.TemporaryDirectory() as scratch:
            folder = Path(given)
            if folder.is_file():
                # A package: its signature and files first, then the skill
                # inside, read as a folder is.
                try:
                    opened = open_package(folder.read_bytes())
                except LampError as error:
                    print(f"{folder.name}: error: {error.why}")
                    failed += 1
                    continue
                # Integrity, not trust: anyone can make a key (Fourteen, #3504).
                signed = f", signed by key {opened.author}, not the Store" if opened.author else ", not signed"
                folder = Path(scratch) / opened.name
                for path, body in opened.files.items():
                    (folder / path).parent.mkdir(parents=True, exist_ok=True)
                    (folder / path).write_bytes(body)
            report = check(folder)
        for line in report.errors:
            print(f"{report.folder.name}: error: {line}")
        for line in report.warnings:
            print(f"{report.folder.name}: warning: {line}")
        if report.ok:
            print(f"{report.folder.name}: ok, format {report.format}{signed}")
        else:
            failed += 1
    return 1 if failed else 0


async def _skill_keygen(args) -> int:
    from .push import fingerprint

    ring = Keyring()
    made = ring.author_key() is None
    key = ring.author_key(make=True)
    public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    print(f"author key {fingerprint(public)} {'made' if made else 'already there'}: {ring.folder / 'author.pem'}")
    return 0


async def _skill_pack(args) -> int:
    from .push import pack
    from .skillcheck import check

    report = check(args.folder)
    if not report.ok:
        raise LampError(f"{report.folder.name}: {report.errors[0]}", code="bad_request")
    author = None
    if args.sign:
        author = Keyring().author_key()
        if author is None:
            raise LampError("no author key: make one with `lamp skill keygen`", code="bad_request")
    package = pack(args.folder, author=author)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"{package.name}-{package.version}.tar"
    path.write_bytes(package.data)
    print(f"{path} {package.sha256}{' signed' if author else ''}")
    return 0


async def _skill_push(args) -> int:
    async with await Lamp.connect(args.lamp) as lamp:
        skill = await lamp.push_skill(args.folder)
    held = f", held: {skill['held']}" if skill.get("held") else ""
    print(f"{skill.get('name')} {skill.get('version')} pushed, {'on' if skill.get('on') else 'off'}{held}")
    for shown in skill.get("shown", []):
        print(f"  {shown}")
    return 0


# What each ask lets a skill do, in the words a person is shown.
ASK_WORDS = {
    "body": "move the lamp",
    "camera": "see the camera's picture",
    "light": "change the light",
    "look": "know when someone is at the lamp",
    "speak": "speak and play sounds",
}


async def _skill_install(args) -> int:
    from .push import asks_of, fetch, open_package, read_capped, tomllib

    # A URL is fetched over https only; anything else is a file here.
    if "://" in args.source:
        data = fetch(args.source)
    else:
        with open(args.source, "rb") as file:
            data = read_capped(file)
    opened = open_package(data)
    asks = asks_of(tomllib.loads(opened.files.get("skill.toml", b"").decode("utf-8", "replace")))
    # Integrity, not trust: anyone can make a key (Fourteen, #3504).
    print(f"{opened.name} {opened.version}")
    print(f"  signed by key {opened.author}, not the Store" if opened.author else "  not signed, not the Store")
    for ask in asks:
        print(f"  asks to {ASK_WORDS[ask]} ({ask})")
    if "camera" in asks:
        print("  It asks for the camera's picture: the lamp keeps it off until the owner says yes in the app.")
    if not args.yes:
        answer = input(f"Put {opened.name} on your lamp? [y/N] ").strip().lower()
        if answer not in ("y", "yes"):
            print("not installed")
            return 1
    async with await Lamp.connect(args.lamp) as lamp:
        skill = await lamp.install_package(data)
    held = f", held: {skill['held']}" if skill.get("held") else ""
    print(f"{skill.get('name')} {skill.get('version')} installed, {'on' if skill.get('on') else 'off'}{held}")
    return 0


async def _skill_list(args) -> int:
    async with await Lamp.connect(args.lamp) as lamp:
        for skill in await lamp.skills():
            state = "on" if skill.get("on") else "off"
            held = f" (held: {skill['held']})" if skill.get("held") else ""
            print(f"{skill.get('name')}\t{skill.get('version')}\t{state}{held}")
    return 0


async def _skill_remove(args) -> int:
    async with await Lamp.connect(args.lamp) as lamp:
        await lamp.remove_skill(args.name)
    print(f"{args.name} removed")
    return 0


async def _hello(args) -> int:
    async with await Lamp.connect(args.lamp) as lamp:
        print(f"{lamp.info.get('id')}: grants {', '.join(lamp.grants)}")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="lamp", description="lampOS control, v1")
    commands = parser.add_subparsers(dest="command", required=True)
    pair = commands.add_parser("pair", help="pair this computer with a lamp, with the owner's code")
    pair.add_argument("code", help="the 8 digits the app shows")
    pair.add_argument("--pin", required=True, help="the lamp's pin the app shows with the code (64 hex)")
    pair.add_argument("--host", help="the lamp's address; without it, found by mDNS")
    pair.add_argument("--port", type=int, default=schema.PORT)
    pair.add_argument("--name", help="this computer's name in the app")
    pair.set_defaults(run=_pair)
    commands.add_parser("find", help="lamps on this network").set_defaults(run=_find)
    fake = commands.add_parser("fake", help="a fake lamp here, paired at once, saying an arrival now and then")
    fake.add_argument("--every", type=float, default=10.0, help="seconds between arrivals (10)")
    fake.add_argument("--cycles", type=int, help="stop after this many (default: until Ctrl-C)")
    fake.add_argument(
        "--everything",
        action="store_true",
        help="answer every op; by default it refuses what lamp-control doesn't route yet, as a lamp does",
    )
    fake.set_defaults(run=_fake)
    hello = commands.add_parser("hello", help="connect to a paired lamp and show its grants")
    hello.add_argument("lamp", nargs="?")
    hello.set_defaults(run=_hello)
    skill = commands.add_parser("skill", help="skill folders: check, push, list, remove")
    skill_commands = skill.add_subparsers(dest="skill_command", required=True)
    skill_check = skill_commands.add_parser("check", help="check skill folders or packages against the skill format, version 1")
    skill_check.add_argument("folders", nargs="+", help="each a folder with SKILL.md and skill.toml, or a package (.tar)")
    skill_check.set_defaults(run=_skill_check)
    skill_keygen = skill_commands.add_parser("keygen", help="make the key you sign your skills with, apart from the pairing key")
    skill_keygen.set_defaults(run=_skill_keygen)
    skill_pack = skill_commands.add_parser("pack", help="check a skill folder and pack it as NAME-VERSION.tar, to share it")
    skill_pack.add_argument("folder")
    skill_pack.add_argument("--sign", action="store_true", help="sign it with your author key (lamp skill keygen)")
    skill_pack.add_argument("--out", default=".", help="the folder to write it in (.)")
    skill_pack.set_defaults(run=_skill_pack)
    skill_push = skill_commands.add_parser("push", help="check a skill folder and put it on your paired lamp (the skills grant)")
    skill_push.add_argument("folder")
    skill_push.add_argument("lamp", nargs="?")
    skill_push.set_defaults(run=_skill_push)
    skill_install = skill_commands.add_parser("install", help="put a shared package (a .tar file or an https URL) on your paired lamp, after showing what it asks")
    skill_install.add_argument("source", help="a package file, or an https:// URL (1 MiB at most)")
    skill_install.add_argument("lamp", nargs="?")
    skill_install.add_argument("--yes", action="store_true", help="don't ask: you have read what it asks")
    skill_install.set_defaults(run=_skill_install)
    skill_list = skill_commands.add_parser("list", help="the skills this computer has pushed")
    skill_list.add_argument("lamp", nargs="?")
    skill_list.set_defaults(run=_skill_list)
    skill_remove = skill_commands.add_parser("remove", help="take one of this computer's pushed skills off the lamp")
    skill_remove.add_argument("name")
    skill_remove.add_argument("lamp", nargs="?")
    skill_remove.set_defaults(run=_skill_remove)
    args = parser.parse_args(argv)
    try:
        return asyncio.run(args.run(args))
    except LampError as error:
        print(f"lamp: {error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    sys.exit(main())
