import asyncio
import json
import urllib.request

from lampos import Lamp

# A public JSON API, no key needed. The run's proxy (HTTPS_PROXY) is used by
# urllib on its own; only the public web on ports 80 and 443 is reachable.
URL = "https://api.frankfurter.app/latest?from=EUR&to=USD"


def fetch_rate():
    request = urllib.request.Request(URL, headers={"User-Agent": "lamp-skill"})
    with urllib.request.urlopen(request, timeout=15) as reply:
        return json.load(reply)["rates"]["USD"]


async def main():
    lamp = await Lamp.local()
    try:
        rate = await asyncio.to_thread(fetch_rate)
        await lamp.say(f"One euro is {rate:.2f} US dollars right now.", wait=True)
    except Exception:
        await lamp.say("I couldn't reach the exchange rate just now.", wait=True)
    await lamp.close()
    # Seen by the lamp's agent that ran it, which then ends with a dash, so
    # the rate is heard once.
    print("Said aloud through the lamp.")


asyncio.run(main())
