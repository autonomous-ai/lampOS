"""`lamp pair CODE`: no `--host` (the lamp found by mDNS) and no `--pin` (the
key the lamp shows at pairing kept and checked on every later connect)."""

from __future__ import annotations

import asyncio
import socket

import pytest

from lampos import Keyring, Lamp, WrongLamp, cli, discovery
from lampos.discovery import Found
from lampos.testing import FakeLamp


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, timeout=30))


def closed_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def found(fake, name="lamp-fake") -> Found:
    return Found(id="", name=name, host=f"{name}.local", port=fake.port, fp="", address="127.0.0.1")


def mdns(monkeypatch, *lamps):
    """mDNS that finds `lamps`, as `pairable` would: none of them is asked anything."""

    async def pairable(*_args, **_kwargs):
        return list(lamps)

    monkeypatch.setattr(cli, "pairable", pairable)


async def pair_cli(argv):
    # cli.main runs its own event loop; the fakes live in this one.
    return await asyncio.to_thread(cli.main, argv)


def test_pair_with_only_the_code_finds_the_lamp_and_keeps_its_pin(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("LAMPOS_HOME", str(tmp_path))

    async def go():
        async with FakeLamp(grants=("light",)) as fake:
            mdns(monkeypatch, found(fake))
            assert await pair_cli(["pair", fake.code]) == 0
            return fake.pin

    pin = run(go())
    out = capsys.readouterr().out
    pairing = Keyring(tmp_path).pairing()
    assert out == f"paired with lamp-fake ({pairing.name}) as {pairing.client}: light\n"
    # The pin of the certificate it was shown, and the host name mDNS gave,
    # which outlives the address.
    assert (pairing.fp, pairing.host) == (pin, "lamp-fake.local")


def test_a_lamp_paired_on_first_use_with_another_key_later_is_refused(tmp_path):
    async def go():
        ring = Keyring(tmp_path)
        async with FakeLamp() as fake:
            pairing = await Lamp.pair(fake.code, fake.host, port=fake.port, keyring=ring)
            assert pairing.fp == fake.pin
            async with await Lamp.connect(keyring=ring):
                pass
        # A reset lamp (a new certificate), or another machine at its address.
        async with FakeLamp() as reset:
            kept = ring.pairing()
            kept.port = reset.port
            ring.remember(kept)
            with pytest.raises(WrongLamp) as caught:
                await Lamp.connect(keyring=ring)
            assert reset.received == []
            return str(caught.value)

    said = run(go())
    assert "another key than the one this computer paired with" in said
    assert "re-pair if you reset the lamp" in said and "`lamp pair CODE`" in said


def test_a_given_pin_is_still_checked_before_the_code_is_sent(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("LAMPOS_HOME", str(tmp_path))

    async def go():
        async with FakeLamp() as fake:
            code = await pair_cli(["pair", fake.code, "--host", fake.host, "--port", str(fake.port), "--pin", "0" * 64])
            assert fake.received == []
            return code

    assert run(go()) == 1
    assert "wrong_lamp: the certificate at 127.0.0.1 is not the lamp's" in capsys.readouterr().err
    assert Keyring(tmp_path).pairings() == {}


def test_a_given_pin_without_host_picks_the_lamp_mdns_finds_with_it(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("LAMPOS_HOME", str(tmp_path))

    async def go():
        async with FakeLamp() as one, FakeLamp() as two:
            first, second = found(one, "lamp-one"), found(two, "lamp-two")
            first.fp, second.fp = one.fp, two.fp
            mdns(monkeypatch, first, second)
            assert await pair_cli(["pair", two.code, "--pin", two.pin]) == 0
            assert one.received == []
            return two.pin

    pin = run(go())
    assert Keyring(tmp_path).pairing().fp == pin


def test_several_lamps_found_are_named_and_none_is_sent_the_code(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("LAMPOS_HOME", str(tmp_path))

    async def go():
        async with FakeLamp() as one, FakeLamp() as two:
            mdns(monkeypatch, found(one, "lamp-one"), found(two, "lamp-two"))
            code = await pair_cli(["pair", one.code])
            assert one.received == [] and two.received == []
            return code

    assert run(go()) == 2
    err = capsys.readouterr().err
    assert err == "lamp: 2 lamps answered: lamp-one (lamp-one.local), lamp-two (lamp-two.local); say which with --host\n"
    assert Keyring(tmp_path).pairings() == {}


def test_no_lamp_found_says_to_turn_developer_access_on_or_give_host(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("LAMPOS_HOME", str(tmp_path))
    mdns(monkeypatch)
    assert cli.main(["pair", "12345678"]) == 2
    err = capsys.readouterr().err
    assert "developer access" in err and "--host lamp-xxxx.local" in err and err.count("\n") == 1


def test_pairable_keeps_only_lamps_whose_control_port_answers(monkeypatch):
    async def go():
        async with FakeLamp() as fake:
            on, off = found(fake, "lamp-on"), Found(id="", name="lamp-off", host="lamp-off.local", port=closed_port(), fp="", address="127.0.0.1")

            async def browse(seconds, devices):
                assert devices
                return [on, off]

            monkeypatch.setattr(discovery, "_browse", browse)
            lamps = await discovery.pairable(probe=1.0)
            # Only a TCP connection: no frame reached the lamp.
            assert fake.received == []
            return lamps

    assert [lamp.name for lamp in run(go())] == ["lamp-on"]


class _Zeroconf:
    """Just enough of python-zeroconf's asyncio API for `_browse`: each
    service in `SERVICES` is seen at once and resolves."""

    SERVICES: dict[tuple[str, str], dict] = {}

    class IPVersion:
        V4Only = 4

    class ServiceStateChange:
        Added = "added"

    class AsyncZeroconf:
        zeroconf = None

        async def __aenter__(self):
            return self

        async def __aexit__(self, *exc):
            return None

    class AsyncServiceBrowser:
        def __init__(self, zc, types, handlers):
            for service_type, name in _Zeroconf.SERVICES:
                if service_type in types:
                    for handler in handlers:
                        handler(zc, service_type, name, _Zeroconf.ServiceStateChange.Added)

        async def async_cancel(self):
            return None

    class AsyncServiceInfo:
        def __init__(self, service_type, name):
            self.__dict__.update(_Zeroconf.SERVICES[(service_type, name)])

        async def async_request(self, zc, ms):
            return True

        def parsed_addresses(self, version=None):
            return [a for a in self.addresses if version is None or ":" not in a]


def test_browse_reads_lampos_lamps_and_the_lamps_avahi_already_announces(monkeypatch):
    lampos, device = discovery.schema.SERVICE, discovery.DEVICE_SERVICE
    _Zeroconf.SERVICES = {
        (lampos, f"lamp-75c9.{lampos}"): {
            "server": "lamp-75c9.local.", "port": 7731, "addresses": ["fe80::1", "192.168.1.20"],
            "properties": {b"v": b"1", b"id": b"lamp-75c9", b"name": b"Desk", b"fp": b"ab" * 8},
        },
        # The same lamp on the image's own service: not twice.
        (device, f"lamp-75c9.{device}"): {"server": "lamp-75c9.local.", "port": 80, "addresses": ["192.168.1.20"], "properties": {}},
        (device, f"lamp-06c1.{device}"): {"server": "lamp-06c1.local.", "port": 80, "addresses": ["192.168.1.21"], "properties": {}},
        # Another device class: not a lamp.
        (device, f"intern-v2-d16f.{device}"): {"server": "intern-v2-d16f.local.", "port": 80, "addresses": ["192.168.1.22"], "properties": {}},
    }
    z = _Zeroconf
    monkeypatch.setattr(discovery, "_zeroconf", lambda: (z.IPVersion, z.ServiceStateChange, z.AsyncServiceBrowser, z.AsyncServiceInfo, z.AsyncZeroconf))
    every = run(discovery._browse(0, devices=True))
    assert every == [
        Found(id="lamp-75c9", name="Desk", host="lamp-75c9.local", port=7731, fp="ab" * 8, address="192.168.1.20"),
        Found(id="", name="lamp-06c1", host="lamp-06c1.local", port=7731, fp="", address="192.168.1.21"),
    ]
    # `find` (and `lamp find`) is the control door's announcement only.
    assert run(discovery.find(0)) == every[:1]
