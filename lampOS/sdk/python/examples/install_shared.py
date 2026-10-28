"""Put someone else's shared skill on your lamp: read the package from a file
or an https URL, show who signed it and what it asks, and install it only on
a yes. A signature says the bytes are that key's, not who is behind the key,
so it is never called "verified":

    python install_shared.py https://example.com/desk-wave-1.0.0.tar
"""

import asyncio
import sys
from pathlib import Path

from lampos import Lamp
from lampos.push import asks_of, fetch, open_package, read_capped, tomllib


async def main(source: str):
    if "://" in source:
        data = fetch(source)  # https only, 1 MiB at most
    else:
        with Path(source).open("rb") as file:
            data = read_capped(file)
    opened = open_package(data)  # the signature, every path and file
    asks = asks_of(tomllib.loads(opened.files["skill.toml"].decode()))
    signer = f"signed by key {opened.author}" if opened.author else "not signed"
    print(f"{opened.name} {opened.version}, {signer}, not the Store; asks: {', '.join(asks) or 'nothing'}")
    if input("Install it? [y/N] ").strip().lower() != "y":
        return
    async with await Lamp.connect() as lamp:
        skill = await lamp.install_package(data)
        if skill["held"]:
            print(f"{skill['name']} waits for the owner's yes in the app: {skill['held']}")
        else:
            print(f"{skill['name']} {skill['version']} is on")


asyncio.run(main(sys.argv[1]))
