"""`lamp`: pair this computer with a lamp, find lamps, check a pairing, or try
it all with no lamp.

    lamp find
    lamp pair 12345678 --pin <64 hex> [--host lamp-75c9.local] [--name "Dev's laptop"]
    lamp hello [LAMP_ID]
    lamp fake [--every 10] [--cycles N] [--everything] [--hands]
    lamp skill check FOLDER...
    lamp skill push FOLDER [LAMP_ID]
    lamp skill list [LAMP_ID]
    lamp skill remove NAME [LAMP_ID]
    lamp speaker FILE.wav|- [--rate 16000|24000] [LAMP_ID]
    lamp login [--email EMAIL] [--code CODE] [--token TOKEN] | lamp logout
    lamp app check FOLDER|TAR...
    lamp app pack FOLDER [-o TAR]
    lamp app submit FOLDER|TAR [--icon PNG] [--cover PNG] [--title T] [--description D] [--category SLUG]
    lamp app status [ID]
    lamp app request-public ID
    lamp app studio [--port N] [--no-open]
    lamp app push FOLDER|TAR [LAMP_ID]
    lamp app list [LAMP_ID]
    lamp app remove NAME [LAMP_ID]

The Store commands take --staging, or --store URL and --account URL, to
use another Store than production's.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys
import warnings
from pathlib import Path

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
    it in another terminal, with the same LAMPOS_HOME. With `--hands`, a
    hand waves for 3 s after each arrival (examples/follow_hand.py)."""
    from .keys import Keyring
    from .testing import LAMP_CONTROL_REFUSES, FakeLamp, hand_track

    hands = getattr(args, "hands", False)
    everything = getattr(args, "everything", False)
    refuses = {} if everything else dict(LAMP_CONTROL_REFUSES)
    async with FakeLamp(
        grants=schema.grants(), name="Fake lamp (lamp fake)", refuses=refuses
    ) as fake:
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
                if hands and event == "person.arrived":
                    await fake.emit_hand_track(hand_track("wave", seconds=3, seed=cycle), realtime=True)
                    print("said a hand waving for 3 s, then gone", flush=True)
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
            print(f"{report.folder.name}: ok, format {report.format}")
        else:
            failed += 1
    return 1 if failed else 0


async def _skill_push(args) -> int:
    with warnings.catch_warnings(record=True) as said:
        warnings.simplefilter("always")
        async with await Lamp.connect(args.lamp) as lamp:
            skill = await lamp.push_skill(args.folder)
    for warning in said:
        print(f"lamp: warning: {warning.message}", file=sys.stderr)
    held = f", held: {skill['held']}" if skill.get("held") else ""
    print(f"{skill.get('name')} {skill.get('version')} pushed, {'on' if skill.get('on') else 'off'}{held}")
    for shown in skill.get("shown", []):
        print(f"  {shown}")
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


def _running(app: dict) -> str:
    return "stopped" if app.get("stopped") or app.get("started") is False else "running"


async def _app_push(args) -> int:
    async with await Lamp.connect(args.lamp) as lamp:
        app = await lamp.push_app(args.package)
    again = ", replacing the one before" if app.get("updated") else ""
    print(f"{app.get('name')} {app.get('version')} pushed{again}, {_running(app)}")
    return 0


async def _app_list(args) -> int:
    async with await Lamp.connect(args.lamp) as lamp:
        for app in await lamp.apps():
            print(f"{app.get('name')}\t{app.get('version')}\t{_running(app)}")
    return 0


async def _app_remove(args) -> int:
    async with await Lamp.connect(args.lamp) as lamp:
        await lamp.remove_app(args.name)
    print(f"{args.name} removed")
    return 0


async def _speaker(args) -> int:
    """Play a WAV, or raw mono S16LE from stdin (`-`, at `--rate`), through
    the lamp's speaker in real time (the `speaker` grant)."""
    import wave

    if args.file == "-":
        rate, pcm = args.rate or 16_000, sys.stdin.buffer.read()
    else:
        try:
            with wave.open(args.file, "rb") as clip:
                rate = clip.getframerate()
        except (OSError, EOFError, wave.Error) as error:
            print(f"lamp: {args.file}: not a WAV that can be read: {error}", file=sys.stderr)
            return 2
        pcm = b""
        if args.rate not in (None, rate):
            print(f"lamp: {args.file} is {rate} Hz, not {args.rate}", file=sys.stderr)
            return 2
    if rate not in (16_000, 24_000):
        print(f"lamp: {rate} Hz; a speaker stream takes 16000 or 24000 (--rate for stdin)", file=sys.stderr)
        return 2
    async with await Lamp.connect(args.lamp) as lamp:
        async with lamp.speaker_stream(rate) as out:
            if args.file == "-":
                await out.play_pcm(pcm)
            else:
                await out.play_wav(args.file)
        if out.ended not in (None, "finished"):
            print(f"lamp: the lamp ended the stream: {out.ended}", file=sys.stderr)
            return 1
    return 0


async def _hello(args) -> int:
    async with await Lamp.connect(args.lamp) as lamp:
        build = f"build {lamp.build}, " if lamp.build else ""
        print(f"{lamp.info.get('id')}: {build}grants {', '.join(lamp.grants)}")
    return 0


def _where(args):
    from .store import endpoints

    return endpoints(staging=args.staging, store=args.store, account=args.account)


def _report_lines(report) -> None:
    name = report.folder.name
    for line in report.errors:
        print(f"{name}: error: {line}")
    for line in report.warnings:
        print(f"{name}: warning: {line}")


async def _app_check(args) -> int:
    from .app import check

    failed = 0
    for path in args.paths:
        report = check(path)
        _report_lines(report)
        if report.ok:
            print(f"{report.folder.name}: ok, an app (format 2)")
        else:
            failed += 1
    return 1 if failed else 0


async def _app_pack(args) -> int:
    from .app import pack

    package = pack(args.folder)
    out = Path(args.output or f"{package.name}-{package.version}.tar")
    out.write_bytes(package.data)
    print(f"{out}: {package.name} {package.version}, {len(package.data)} bytes, sha256 {package.sha256} (unsigned: the Store signs it at approval)")
    return 0


def _package_for_upload(path: str):
    """The bytes to upload and their file name: a folder packed, a tar as
    it is; either refused here, before anything is sent, if the check
    refuses it."""
    from .app import check, pack

    report = check(path)
    _report_lines(report)
    if not report.ok:
        print(f"lamp: {report.folder.name}: refused by the check, so nothing was sent", file=sys.stderr)
        return None
    if Path(path).is_dir():
        package = pack(path)
        return package.data, f"{package.name}-{package.version}.tar"
    return Path(path).read_bytes(), Path(path).name


async def _app_submit(args) -> int:
    from .store import Store, describe

    store = Store.signed_in(where=_where(args))
    upload = _package_for_upload(args.path)
    if upload is None:
        return 1
    data, filename = upload
    art = {key: Path(value).read_bytes() for key, value in (("icon", args.icon), ("cover", args.cover)) if value}
    app = store.submit(data, filename=filename, title=args.title, description=args.description, category=args.category, **art)
    print(describe(app))
    print("submitted: a reviewer reads it next; `lamp app status` follows the review")
    return 0


async def _app_status(args) -> int:
    from .store import Store, describe

    store = Store.signed_in(where=_where(args))
    if args.id:
        print(describe(store.app(args.id)))
        return 0
    apps, total = store.apps()
    for app in apps:
        print(describe(app))
    if not apps:
        print("no apps yet: `lamp app submit` sends one")
    elif total > len(apps):
        print(f"({len(apps)} of {total})")
    return 0


async def _app_request_public(args) -> int:
    from .store import Store

    Store.signed_in(where=_where(args)).request_public(args.id)
    print(f"{args.id}: asked to go public; staff decide, and `lamp app status` shows it")
    return 0


async def _app_studio(args) -> int:
    from .studio import Studio

    studio = Studio(_where(args), port=args.port)
    print(f"the app studio is at {studio.url} (this computer only); Ctrl-C ends it", flush=True)
    if not args.no_open:
        import webbrowser

        webbrowser.open(studio.url)
    try:
        await asyncio.to_thread(studio.serve_forever)
    finally:
        studio.close()
    return 0


async def _login(args) -> int:
    from .store import Store, save_token

    where = _where(args)
    if args.token:
        path = save_token(args.token, where, args.email)
        print(f"kept the token for {where.account} in {path}")
        return 0
    email = args.email or input("Your Autonomous account's email: ").strip()
    store = Store(where=where)
    if not args.code:
        store.send_code(email)
        print(f"a sign-in code is on its way to {email}")
    code = args.code or input("The code from the email: ").strip()
    token = store.sign_in(email, code)
    path = save_token(token, where, email)
    print(f"signed in as {email}; the token is kept in {path} (0600)")
    return 0


async def _logout(args) -> int:
    from .store import forget_token

    where = _where(args)
    print(f"signed out of {where.account}" if forget_token(where) else f"no sign-in kept for {where.account}")
    return 0


def _store_options(command: argparse.ArgumentParser) -> argparse.ArgumentParser:
    command.add_argument("--staging", action="store_true", help="the staging Store and account, not production's")
    command.add_argument("--store", help="the Store developer API's base URL (LAMP_STORE_URL)")
    command.add_argument("--account", help="the account sign-in's base URL (LAMP_ACCOUNT_URL)")
    return command


def parser() -> argparse.ArgumentParser:
    """Every `lamp` command and option (the README's reference names each)."""
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
    fake.add_argument(
        "--hands",
        action="store_true",
        help="a hand waves for 3 s after each arrival, and looks are answered, as on a lamp with the body's lease socket (follow_hand.py)",
    )
    fake.set_defaults(run=_fake)
    hello = commands.add_parser("hello", help="connect to a paired lamp and show its grants")
    hello.add_argument("lamp", nargs="?")
    hello.set_defaults(run=_hello)
    skill = commands.add_parser("skill", help="skill folders: check, push, list, remove")
    skill_commands = skill.add_subparsers(dest="skill_command", required=True)
    skill_check = skill_commands.add_parser("check", help="check skill folders against the skill format, 1 or 2")
    skill_check.add_argument("folders", nargs="+", help="each a folder with SKILL.md and skill.toml")
    skill_check.set_defaults(run=_skill_check)
    skill_push = skill_commands.add_parser("push", help="check a skill folder and put it on your paired lamp (the skills grant)")
    skill_push.add_argument("folder")
    skill_push.add_argument("lamp", nargs="?")
    skill_push.set_defaults(run=_skill_push)
    skill_list = skill_commands.add_parser("list", help="the skills this computer has pushed")
    skill_list.add_argument("lamp", nargs="?")
    skill_list.set_defaults(run=_skill_list)
    skill_remove = skill_commands.add_parser("remove", help="take one of this computer's pushed skills off the lamp")
    skill_remove.add_argument("name")
    skill_remove.add_argument("lamp", nargs="?")
    skill_remove.set_defaults(run=_skill_remove)
    speaker = commands.add_parser(
        "speaker", help="play a WAV (or raw mono S16LE on stdin, `-`) through the lamp's speaker, in real time (the speaker grant)"
    )
    speaker.add_argument("file", help="a WAV of 16-bit mono PCM at 16000 or 24000 Hz, or - for raw samples on stdin")
    speaker.add_argument("lamp", nargs="?")
    speaker.add_argument("--rate", type=int, choices=(16_000, 24_000), default=None, help="stdin's sample rate (16000)")
    speaker.set_defaults(run=_speaker)
    login = _store_options(commands.add_parser("login", help="sign in to the Store with your Autonomous account's emailed code"))
    login.add_argument("--email", help="the account's email (asked if left out)")
    login.add_argument("--code", help="the emailed code, when you already have one (asked if left out)")
    login.add_argument("--token", help="keep this account token instead of signing in (also LAMP_STORE_TOKEN)")
    login.set_defaults(run=_login)
    _store_options(commands.add_parser("logout", help="forget the Store sign-in kept here")).set_defaults(run=_logout)
    app = commands.add_parser(
        "app", help="apps: check, pack, submit, status, request-public, studio for the Store; push, list, remove on your own lamp"
    )
    app_commands = app.add_subparsers(dest="app_command", required=True)
    app_check = app_commands.add_parser("check", help="check app folders or packed tars as the Store's reader does")
    app_check.add_argument("paths", nargs="+", help="each an app's folder (with app.json) or a packed .tar")
    app_check.set_defaults(run=_app_check)
    app_pack = app_commands.add_parser("pack", help="pack an app's folder as the Store's unsigned tar")
    app_pack.add_argument("folder")
    app_pack.add_argument("-o", "--output", help="the tar's path (NAME-VERSION.tar)")
    app_pack.set_defaults(run=_app_pack)
    app_submit = _store_options(app_commands.add_parser("submit", help="check, then upload an app for review (it stays unlisted)"))
    app_submit.add_argument("path", help="an app's folder (packed for you) or a packed .tar")
    app_submit.add_argument("--icon", help="a PNG, exactly 1024x1024, 2 MB at most")
    app_submit.add_argument("--cover", help="a PNG, exactly 1600x900, 2 MB at most")
    app_submit.add_argument("--title", help="the Store's title, 100 characters at most")
    app_submit.add_argument("--description", help="plain text, 1000 characters at most")
    app_submit.add_argument("--category", help="a Store category slug")
    app_submit.set_defaults(run=_app_submit)
    app_status = _store_options(app_commands.add_parser("status", help="your apps' reviews, or one's"))
    app_status.add_argument("id", nargs="?")
    app_status.set_defaults(run=_app_status)
    app_public = _store_options(app_commands.add_parser("request-public", help="ask staff to make an approved app public"))
    app_public.add_argument("id")
    app_public.set_defaults(run=_app_request_public)
    app_studio = _store_options(app_commands.add_parser("studio", help="a page on this computer to check, submit and follow an app"))
    app_studio.add_argument("--port", type=int, default=0, help="the port on 127.0.0.1 (any free one)")
    app_studio.add_argument("--no-open", action="store_true", help="print the page's address, don't open a browser")
    app_studio.set_defaults(run=_app_studio)
    app_push = app_commands.add_parser("push", help="check an app folder or packed tar, and put it on your paired lamp, unsigned (the skills grant)")
    app_push.add_argument("package", help="the app's folder (app.json, src/, ...) or its packed tar")
    app_push.add_argument("lamp", nargs="?")
    app_push.set_defaults(run=_app_push)
    app_list = app_commands.add_parser("list", help="the apps this computer has pushed")
    app_list.add_argument("lamp", nargs="?")
    app_list.set_defaults(run=_app_list)
    app_remove = app_commands.add_parser("remove", help="take one of this computer's pushed apps off the lamp")
    app_remove.add_argument("name")
    app_remove.add_argument("lamp", nargs="?")
    app_remove.set_defaults(run=_app_remove)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        return asyncio.run(args.run(args))
    except LampError as error:
        print(f"lamp: {error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    sys.exit(main())
