"""`lamp`: pair this computer with a lamp, find lamps, check a pairing, or try
it all with no lamp.

    lamp find
    lamp pair 12345678 --pin <64 hex> [--host lamp-75c9.local] [--name "Dev's laptop"]
    lamp hello [LAMP_ID]
    lamp fake [--every 10] [--cycles N]
    lamp skill check FOLDER...
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys

from . import schema
from .client import Lamp
from .discovery import find
from .errors import LampError


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
    from .keys import Keyring
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
    from .skillcheck import check

    failed = 0
    for folder in args.folders:
        report = check(folder)
        for line in report.errors:
            print(f"{report.folder.name}: error: {line}")
        for line in report.warnings:
            print(f"{report.folder.name}: warning: {line}")
        if report.ok:
            print(f"{report.folder.name}: ok, format 1")
        else:
            failed += 1
    return 1 if failed else 0


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
    skill = commands.add_parser("skill", help="skill folders: lamp skill check FOLDER...")
    skill_commands = skill.add_subparsers(dest="skill_command", required=True)
    skill_check = skill_commands.add_parser("check", help="check skill folders against the skill format, version 1")
    skill_check.add_argument("folders", nargs="+", help="each a folder with SKILL.md and skill.toml")
    skill_check.set_defaults(run=_skill_check)
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
