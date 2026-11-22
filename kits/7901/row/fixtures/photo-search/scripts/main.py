# #7901 ROW leg S fixture: the camera, then the internet (its egress socket, HTTPS_PROXY), then a try at the notes,
# which this skill was never granted (lamp-control refuses it no_grant: no skill reads another's notes).
import asyncio
import urllib.request

from lampos import Lamp


def fetch():
    with urllib.request.urlopen("https://example.com/", timeout=15) as page:
        return page.status


async def main():
    lamp = await Lamp.local()
    said = []
    try:
        await lamp.camera_snapshot()
        said.append("I took a picture")
    except Exception:
        said.append("the camera was refused")
    try:
        status = await asyncio.to_thread(fetch)
        said.append("reached the internet (%s)" % status)
    except Exception:
        said.append("did not reach the internet")
    try:
        await lamp.notes_get("prices")
        said.append("and read notes")
    except Exception:
        said.append("and no notes could be read")
    await lamp.say(", ".join(said) + ".", wait=True)
    await lamp.close()


asyncio.run(main())
