"""A lamp that does not answer: one line from the SDK, never a traceback."""

from __future__ import annotations

import asyncio
import socket
import ssl
import sys
import threading

import pytest

from lampos import Keyring, Lamp, LampError, SecureConnectionFailed, Unreachable, cli
from lampos import client

PIN = "a" * 64


def run(coroutine):
    return asyncio.run(asyncio.wait_for(coroutine, timeout=20))


def closed_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def test_a_refused_port_is_unreachable_with_the_address_and_what_to_check(tmp_path):
    port = closed_port()
    with pytest.raises(Unreachable) as caught:
        run(Lamp.pair("12345678", "127.0.0.1", pin=PIN, port=port, keyring=Keyring(tmp_path)))
    said = str(caught.value)
    assert isinstance(caught.value, LampError) and caught.value.code == "unreachable"
    assert said.startswith(f"unreachable: no answer from 127.0.0.1:{port} (")
    assert "developer access" in said and "lamp's network" in said
    assert "\n" not in said


def test_an_unknown_name_is_unreachable(tmp_path):
    with pytest.raises(Unreachable) as caught:
        run(Lamp.pair("12345678", "no-such-lamp.invalid", pin=PIN, keyring=Keyring(tmp_path)))
    assert "no answer from no-such-lamp.invalid:" in str(caught.value)


def test_a_timeout_is_unreachable(tmp_path, monkeypatch):
    async def slow(*args, **kwargs):
        raise asyncio.TimeoutError("timed out during opening handshake")

    monkeypatch.setattr(client, "ws_connect", slow)
    with pytest.raises(Unreachable) as caught:
        run(Lamp.pair("12345678", "lamp-xxxx.local", pin=PIN, keyring=Keyring(tmp_path)))
    assert "timed out during opening handshake" in str(caught.value)


def test_a_mac_no_route_error_names_the_local_network_setting_only_on_a_mac(monkeypatch):
    error = OSError(65, "No route to host")
    monkeypatch.setattr(sys, "platform", "darwin")
    said = client._no_answer("lamp-xxxx.local", 7731, error)
    assert "Local Network" in said and "if it is listed" in said
    assert "terminal" not in said and 'On a Mac when Python is not listed' in said
    monkeypatch.setattr(sys, "platform", "linux")
    assert "Local Network" not in client._no_answer("lamp-xxxx.local", 7731, error)
    monkeypatch.setattr(sys, "platform", "darwin")
    assert "Local Network" not in client._no_answer("lamp-xxxx.local", 7731, OSError(111, "Connection refused"))


def test_connect_to_a_paired_lamp_that_stopped_answering_is_unreachable(tmp_path):
    from lampos.testing import FakeLamp

    async def go():
        ring = Keyring(tmp_path)
        async with FakeLamp(grants=["light"]) as fake:
            await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, keyring=ring)
        return await Lamp.connect(keyring=ring)

    with pytest.raises(Unreachable):
        run(go())


def test_the_command_says_it_in_one_line_and_exits_1(tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("LAMPOS_HOME", str(tmp_path))
    port = closed_port()
    code = cli.main(["pair", "12345678", "--pin", PIN, "--host", "127.0.0.1", "--port", str(port)])
    out = capsys.readouterr()
    assert code == 1
    assert out.out == ""
    assert out.err.count("\n") == 1 and out.err.startswith("lamp: unreachable: no answer from 127.0.0.1:")
    assert "Traceback" not in out.err


def test_the_classes_and_codes_the_protocol_lists_are_unchanged():
    from lampos import errors, schema

    assert set(errors.WHAT_TO_DO) == set(schema.error_codes())
    assert "unreachable" not in schema.error_codes()


def test_a_tls_failure_is_not_said_to_be_no_answer(tmp_path):
    """Something answers on the port but does not speak TLS: the network works,
    so the line must not send the developer to check it (SSLError is an OSError)."""
    server = socket.socket()
    server.bind(("127.0.0.1", 0))
    server.listen(1)
    port = server.getsockname()[1]

    def answer_plain():
        conn, _ = server.accept()
        conn.recv(4096)
        conn.sendall(b"HTTP/1.1 400 Bad Request\r\n\r\n")
        conn.close()

    thread = threading.Thread(target=answer_plain, daemon=True)
    thread.start()
    try:
        with pytest.raises(SecureConnectionFailed) as caught:
            run(Lamp.pair("12345678", "127.0.0.1", pin=PIN, port=port, keyring=Keyring(tmp_path)))
    finally:
        server.close()
        thread.join(5)
    said = str(caught.value)
    assert not isinstance(caught.value, Unreachable) and caught.value.code == "tls_failed"
    assert said.startswith(f"tls_failed: something answered at 127.0.0.1:{port} but the secure connection failed (")
    assert "no answer" not in said and "\n" not in said


def test_a_tls_error_from_the_connect_is_caught_before_the_os_error_clause(tmp_path, monkeypatch):
    async def broken(*args, **kwargs):
        raise ssl.SSLError(1, "[SSL: WRONG_VERSION_NUMBER] wrong version number")

    monkeypatch.setattr(client, "ws_connect", broken)
    with pytest.raises(SecureConnectionFailed) as caught:
        run(Lamp.pair("12345678", "lamp-xxxx.local", pin=PIN, keyring=Keyring(tmp_path)))
    assert "answered" in str(caught.value) and not isinstance(caught.value, Unreachable)


def test_the_command_says_a_tls_failure_in_one_line_too(tmp_path, monkeypatch, capsys):
    async def broken(*args, **kwargs):
        raise ssl.SSLError(1, "[SSL: WRONG_VERSION_NUMBER] wrong version number")

    monkeypatch.setattr(client, "ws_connect", broken)
    monkeypatch.setenv("LAMPOS_HOME", str(tmp_path))
    code = cli.main(["pair", "12345678", "--pin", PIN, "--host", "lamp-xxxx.local"])
    out = capsys.readouterr()
    assert code == 1 and out.err.count("\n") == 1 and out.err.startswith("lamp: tls_failed: something answered at lamp-xxxx.local:")
