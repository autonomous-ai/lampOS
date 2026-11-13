"""`import lampos` no longer needs `cryptography`, only pairing and keys use it (#6507): `import lampos` and
`Lamp.local(token)` work with that wheel blocked (a skill's script on the lamp may run in a Python that has
`websockets` and not `cryptography`), and only a call that needs a key says it is missing."""

from __future__ import annotations

import asyncio
import json
import sys
import textwrap
from pathlib import Path

import pytest

from lampos.testing import FakeLamp

SRC = str(Path(__file__).resolve().parent.parent / "src")

# Runs in a child that cannot import `cryptography` (a None entry in sys.modules makes any import of it fail).
CHILD = textwrap.dedent(
    """
    import asyncio, json, sys
    sys.modules["cryptography"] = None
    sys.path.insert(0, {src!r})
    from lampos import Lamp, Keyring, NotPaired

    async def main():
        async with await Lamp.local({token!r}, path={path!r}) as lamp:
            reply = await lamp.light("warm", 3, 1000)
            print(json.dumps({{"run": lamp.run, "grants": lamp.grants, "to": reply["to"]}}))
        try:
            Keyring().key()
        except ImportError as error:
            print(json.dumps({{"key": "needs cryptography", "name": error.name.split(".")[0]}}))

    asyncio.run(main())
    """
)


def test_import_and_lamp_local_work_with_cryptography_blocked(tmp_path):
    async def go():
        async with FakeLamp(local=True, grants=("light",)) as fake:
            token = fake.mint_token(["light"], run="r1", skill="water")
            code = CHILD.format(src=SRC, token=token, path=fake.local_path)
            child = await asyncio.create_subprocess_exec(
                sys.executable,
                "-c",
                code,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                env={"HOME": str(tmp_path), "LAMPOS_HOME": str(tmp_path / "lampos")},
            )
            out, err = await asyncio.wait_for(child.communicate(), 30)
            assert child.returncode == 0, err.decode()
            return [json.loads(line) for line in out.decode().splitlines()]

    first, second = asyncio.run(go())
    assert first == {"run": "r1", "grants": ["light"], "to": "ring"}
    assert second == {"key": "needs cryptography", "name": "cryptography"}
