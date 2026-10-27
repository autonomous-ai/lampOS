"""Finding lamps on the home network by mDNS (`_lampos._tcp`).

Needs the `zeroconf` package (`pip install lampos[find]`). Without it,
pair with the lamp's address: `lamp pair CODE --host lamp-75c9.local`.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass

from . import schema
from .errors import LampError


@dataclass
class Found:
    """A lamp that answered: its id, name, address, port and TXT `fp`, the
    first 16 hex of its certificate's SHA-256. Any host on the network can
    answer mDNS, so `fp` only helps pick a lamp: pairing checks the full pin
    from the owner's app."""

    id: str
    name: str
    host: str
    port: int
    fp: str


async def find(seconds: float = 3.0) -> list[Found]:
    """The lamps that answer within `seconds`."""
    try:
        from zeroconf import ServiceStateChange
        from zeroconf.asyncio import AsyncServiceBrowser, AsyncServiceInfo, AsyncZeroconf
    except ImportError as error:
        raise LampError("finding lamps needs zeroconf: pip install 'lampos[find]'", code="no_zeroconf") from error

    names: set[str] = set()

    def seen(zeroconf, service_type, name, state_change):
        if state_change is ServiceStateChange.Added:
            names.add(name)

    async with AsyncZeroconf() as zeroconf:
        browser = AsyncServiceBrowser(zeroconf.zeroconf, schema.SERVICE, handlers=[seen])
        await asyncio.sleep(seconds)
        found = []
        for name in sorted(names):
            info = AsyncServiceInfo(schema.SERVICE, name)
            if not await info.async_request(zeroconf.zeroconf, 1500):
                continue
            txt = {k.decode(): (v or b"").decode() for k, v in info.properties.items()}
            if txt.get("v") != str(schema.VERSION):
                continue
            found.append(
                Found(
                    id=txt.get("id", ""),
                    name=txt.get("name", ""),
                    host=(info.server or "").rstrip("."),
                    port=info.port or schema.PORT,
                    fp=txt.get("fp", ""),
                )
            )
        await browser.async_cancel()
        return found
