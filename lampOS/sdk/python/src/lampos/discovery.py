"""Finding lamps on the home network by mDNS.

`find` lists the lamps that announce the control door (`_lampos._tcp`, with
TXT `id`, `name`, `v` and `fp`). `pairable` is what `lamp pair CODE` with no
`--host` uses: those, and the lamps the image's avahi already announces
(`_autonomous._tcp`, named `lamp-xxxx`), kept only if their control port
answers, which it does only while the owner has developer access on. Any host
on the network can answer mDNS, so what is found only says where to connect:
the pairing's code and its pin say whether it is the lamp.
"""

from __future__ import annotations

import asyncio
import contextlib
from dataclasses import dataclass

from . import schema
from .errors import LampError

# What the lamp image's avahi announces today (scripts/imager): every device,
# its instance named by its host name, so a lamp is `lamp-xxxx`.
DEVICE_SERVICE = "_autonomous._tcp.local."
LAMP_PREFIX = "lamp-"


@dataclass
class Found:
    """A lamp that answered: its id, name, host name, port and TXT `fp`, the
    first 16 hex of its certificate's SHA-256, and `address`, the IP mDNS
    resolved its name to (empty if none came). Any host on the network can
    answer mDNS, so `fp` only helps pick a lamp: pairing checks the full pin,
    or, without one, keeps the certificate it is shown first."""

    id: str
    name: str
    host: str
    port: int
    fp: str
    address: str = ""


def _zeroconf():
    try:
        from zeroconf import IPVersion, ServiceStateChange
        from zeroconf.asyncio import AsyncServiceBrowser, AsyncServiceInfo, AsyncZeroconf
    except ImportError as error:
        raise LampError("finding lamps needs zeroconf: pip install zeroconf", code="no_zeroconf") from error
    return IPVersion, ServiceStateChange, AsyncServiceBrowser, AsyncServiceInfo, AsyncZeroconf


async def _browse(seconds: float, devices: bool) -> list[Found]:
    """The `_lampos._tcp` lamps and, with `devices`, the `_autonomous._tcp`
    lamps no `_lampos._tcp` answer named, on the control port."""
    IPVersion, ServiceStateChange, AsyncServiceBrowser, AsyncServiceInfo, AsyncZeroconf = _zeroconf()
    types = [schema.SERVICE] + ([DEVICE_SERVICE] if devices else [])
    names: set[tuple[str, str]] = set()

    def seen(zeroconf, service_type, name, state_change):
        if state_change is ServiceStateChange.Added:
            names.add((service_type, name))

    async with AsyncZeroconf() as zeroconf:
        browser = AsyncServiceBrowser(zeroconf.zeroconf, types, handlers=[seen])
        await asyncio.sleep(seconds)
        found: list[Found] = []
        for service_type, name in sorted(names, key=lambda pair: (pair[0] != schema.SERVICE, pair[1])):
            info = AsyncServiceInfo(service_type, name)
            if not await info.async_request(zeroconf.zeroconf, 1500):
                continue
            host = (info.server or "").rstrip(".")
            address = next(iter(info.parsed_addresses(IPVersion.V4Only) or info.parsed_addresses()), "")
            if service_type == schema.SERVICE:
                txt = {k.decode(): (v or b"").decode() for k, v in info.properties.items()}
                if txt.get("v") != str(schema.VERSION):
                    continue
                found.append(
                    Found(
                        id=txt.get("id", ""),
                        name=txt.get("name", ""),
                        host=host,
                        port=info.port or schema.PORT,
                        fp=txt.get("fp", ""),
                        address=address,
                    )
                )
                continue
            instance = name[: -len(service_type)].rstrip(".")
            if not instance.startswith(LAMP_PREFIX) or any(lamp.host == host for lamp in found):
                continue
            found.append(Found(id="", name=instance, host=host, port=schema.PORT, fp="", address=address))
        await browser.async_cancel()
        return found


async def find(seconds: float = 3.0) -> list[Found]:
    """The lamps that announce `_lampos._tcp` within `seconds`."""
    return await _browse(seconds, devices=False)


async def _answers(lamp: Found, seconds: float) -> bool:
    """Whether the lamp's control port takes a TCP connection: nothing is
    sent, so no code or proof reaches a host that is not the lamp."""
    try:
        _, writer = await asyncio.wait_for(asyncio.open_connection(lamp.address or lamp.host, lamp.port), seconds)
    except (OSError, asyncio.TimeoutError):
        return False
    writer.close()
    with contextlib.suppress(OSError):
        await writer.wait_closed()
    return True


async def pairable(seconds: float = 3.0, *, probe: float = 1.5) -> list[Found]:
    """The lamps `lamp pair CODE` could pair with: the ones mDNS finds
    (`_lampos._tcp`, or `lamp-xxxx` on `_autonomous._tcp`) whose control port
    answers within `probe` seconds, as it does only while developer access
    is on."""
    lamps = await _browse(seconds, devices=True)
    answers = await asyncio.gather(*(_answers(lamp, probe) for lamp in lamps))
    return [lamp for lamp, ok in zip(lamps, answers) if ok]
