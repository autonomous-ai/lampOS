# Mac. Legs S and K's install route: a skill folder pushed from the runner's paired Mac to the lamp's developer door,
# the product's way a skill that was not made on the lamp gets on without the app (lamp-control's skill.push, its
# push.rs, then root's manage helper, lamp-skills package.rs install_pushed_at, into /var/lib/lampOS/skills-dev).
# It is the SDK's Lamp.push_skill (sdk/python/src/lampos/client.py:1646) without the SDK's own pre-check, which lags
# the lamp (it knows no `net` grant and no `secrets` key): the lamp checks everything itself, and its refusal is printed.
#   python3 push.py push <folder> [lamp]   -> PUSHED <the lamp's answer, JSON> | PUSH REFUSED <why>  (exit 0 | 3)
#   python3 push.py remove <name> [lamp]   -> REMOVED <name> | REMOVE REFUSED <why>
# Needs the lampos SDK (pip install lampOS/sdk/python) and this Mac paired with the `skills` grant (`lamp pair`).
import asyncio, json, sys


async def main(op, what, lamp_name):
    try:
        from lampos import Lamp
        from lampos.push import pack
    except Exception as error:
        print("PUSH REFUSED no lampos SDK on this Mac: %s" % error); return 3
    try:
        async with await Lamp.connect(lamp_name) as lamp:
            if op == "remove":
                await lamp.remove_skill(what); print("REMOVED %s" % what); return 0
            package = pack(what); parts = package.parts(); reply = {}
            for part, data in enumerate(parts):
                reply = await lamp.request("skill.push", name=package.name, sha256=package.sha256,
                                           size=len(package.data), part=part, parts=len(parts), data=data)
            print("PUSHED %s" % json.dumps(reply.get("skill", {}), sort_keys=True)); return 0
    except Exception as error:
        print("%s REFUSED %s: %s" % ("REMOVE" if op == "remove" else "PUSH", type(error).__name__, error)); return 3


if __name__ == "__main__":
    a = sys.argv[1:]
    if len(a) < 2 or a[0] not in ("push", "remove"): print(__doc__ or "push.py push|remove <folder|name> [lamp]"); sys.exit(2)
    sys.exit(asyncio.run(main(a[0], a[1], a[2] if len(a) > 2 and a[2] else None)))
