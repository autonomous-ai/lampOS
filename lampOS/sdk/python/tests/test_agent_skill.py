"""The agent skill for developers (sdk/agent-skill/lampos-sdk) kept in step
with the SDKs and lamp-mcp: every public call, tool, event, state and error
code named, its links resolving, and its scripts working against the fake."""

from __future__ import annotations

import ast
import asyncio
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

import pytest
from lampos import Keyring, Lamp, schema
from lampos.testing import FakeLamp

SDK = Path(__file__).resolve().parents[2]
SKILL = SDK / "agent-skill" / "lampos-sdk"


def skill_text() -> str:
    return "\n".join(path.read_text() for path in sorted(SKILL.rglob("*.md")))


def public_methods(path: Path, *classes: str) -> set[str]:
    tree = ast.parse(path.read_text())
    return {
        node.name
        for cls in tree.body
        if isinstance(cls, ast.ClassDef) and cls.name in classes
        for node in cls.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and not node.name.startswith("_")
    }


def test_the_skill_says_when_to_use_it():
    head = SKILL.joinpath("SKILL.md").read_text().split("---")[1]
    assert re.search(r"^name: lampos-sdk$", head, re.M)
    description = re.search(r"^description: (.+)$", head, re.M).group(1)
    assert len(description) < 1024
    for word in ("lampOS", "lamp", "program the lamp"):
        assert word in description, word


def test_every_link_in_the_skill_resolves():
    for page in SKILL.rglob("*.md"):
        for target in re.findall(r"\]\(([^)#:]+)(?:#[^)]*)?\)", page.read_text()):
            assert (page.parent / target).exists(), f"{page.name}: {target}"


def test_the_skill_names_every_python_call():
    text = skill_text()
    for name in public_methods(SDK / "python/src/lampos/client.py", "Lamp", "Lease") - {"end", "following"}:
        # A property (`lease.held`) is named without a call.
        assert f"{name}(" in text or f".{name}`" in text or f".{name}," in text, f"Python: {name}"


def test_the_skill_names_every_js_call():
    js = (SDK / "js/src/client.js").read_text()
    text = skill_text()
    # Internal helpers a program doesn't call.
    inner = {"constructor", "act", "frame", "send", "event", "finish", "end", "renew", "following"}
    for cls in ("Lamp", "Lease"):
        body = re.search(r"export class " + cls + r"\b.*?\n}\n", js, re.S).group(0)
        for name in set(re.findall(r"^  (?:async |static )?([a-zA-Z]\w*)\(", body, re.M)) - inner:
            assert f"{name}(" in text or f".{name}`" in text or f".{name}," in text, f"JS {cls}: {name}"


def test_the_skill_names_every_mcp_tool_and_resource():
    server = (SDK / "mcp/src/lamp_mcp/server.py").read_text()
    text = skill_text()
    for tool in re.findall(r"async def (lamp_\w+)\(", server):
        assert f"`{tool}" in text, tool
    for resource in re.findall(r'resource\("(lamp://[^"{]+)"', server):
        assert resource in text, resource


def test_the_skill_names_every_event_state_and_error_code():
    text = skill_text()
    for event in (*schema.events(), "lease", "done"):
        assert f"`{event}`" in text, event
    for state in schema.states():
        assert f"`{state}`" in text, state
    errors = SKILL.joinpath("references/errors.md").read_text()
    for code in schema.error_codes():
        assert f"| `{code}` |" in errors, code


def test_the_skill_names_every_lamp_command():
    from lampos import cli

    text = skill_text()
    commands = [action for action in cli.parser()._actions if hasattr(action, "choices") and action.choices]
    for command in commands[0].choices:
        assert f"lamp {command}" in text, command


def test_the_plugin_points_at_the_skill():
    plugin = json.loads((SDK / "agent-skill/.claude-plugin/plugin.json").read_text())
    market = json.loads((SDK / "agent-skill/.claude-plugin/marketplace.json").read_text())
    assert plugin["name"] == market["plugins"][0]["name"] == "lampos-sdk"
    for path in plugin["skills"]:
        assert (SDK / "agent-skill" / path / "lampos-sdk" / "SKILL.md").exists()


def test_check_pairing_says_hello_grants_and_limits(tmp_path):
    async def go():
        async with FakeLamp(grants=("light", "speak")) as fake:
            await Lamp.pair(fake.code, fake.host, pin=fake.pin, port=fake.port, keyring=Keyring(tmp_path))
            ran = await asyncio.create_subprocess_exec(
                sys.executable, str(SKILL / "scripts/check_pairing.py"),
                env={**os.environ, "LAMPOS_HOME": str(tmp_path)},
                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.STDOUT,
            )
            said = (await ran.communicate())[0].decode()
            assert ran.returncode == 0, said
            assert "light: light the ring" in said and "speak:" in said
            assert "not granted" in said and "body" in said
            assert "rates:" in said and "quiet hours:" in said

    asyncio.run(asyncio.wait_for(go(), 30))


@pytest.mark.skipif(not shutil.which("lamp") or not shutil.which("bash"), reason="needs the lamp command and bash")
def test_try_fake_runs_a_program_against_a_fresh_fake(tmp_path):
    program = tmp_path / "hello.py"
    program.write_text(
        "import asyncio\nfrom lampos import Lamp\n"
        "async def main():\n"
        "    async with await Lamp.connect() as lamp:\n"
        "        print('answered', (await lamp.light('warm', 2, 5000))['to'])\n"
        "asyncio.run(main())\n"
    )
    ran = subprocess.run(
        ["bash", str(SKILL / "scripts/try_fake.sh"), str(program)], capture_output=True, text=True, timeout=60
    )
    assert ran.returncode == 0, ran.stdout + ran.stderr
    assert "answered ring" in ran.stdout
