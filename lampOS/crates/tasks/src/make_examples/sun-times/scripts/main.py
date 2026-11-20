import asyncio
import urllib.request

from lampos import Lamp

# A public service, no key needed: today's sunset where the lamp is, as
# plain text (HH:MM:SS). urllib uses the run's proxy (HTTPS_PROXY) on its own.
URL = "https://wttr.in/?format=%s"


def fetch_sunset():
    request = urllib.request.Request(URL, headers={"User-Agent": "lamp-skill"})
    with urllib.request.urlopen(request, timeout=15) as reply:
        return reply.read(64).decode("utf-8").strip()[:5]


async def main():
    lamp = await Lamp.local()
    try:
        sunset = await asyncio.to_thread(fetch_sunset)
        await lamp.say(f"The sun sets at {sunset} today.", wait=True)
    except Exception:
        await lamp.say("I couldn't reach the sunset time just now.", wait=True)
    await lamp.close()
    # Seen by the lamp's agent that ran it, which then ends with a dash, so
    # the time is heard once.
    print("Said aloud through the lamp.")


asyncio.run(main())
