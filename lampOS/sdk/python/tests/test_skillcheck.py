"""`lamp skill check`: the repo's own skills pass it, and each thing the
format refuses off the lamp is refused, with a line that says where."""

from __future__ import annotations

import json
import os
import wave
from pathlib import Path

import pytest

from lampos.cli import main
from lampos.skillcheck import check

REPO = Path(__file__).resolve().parents[3]


def skill(folder: Path, *, name: str | None = None, rules: str = 'version = "1.0.0"\n', md: str | None = None) -> Path:
    folder.mkdir(parents=True, exist_ok=True)
    name = folder.name if name is None else name
    (folder / "SKILL.md").write_text(md if md is not None else f"---\nname: {name}\ndescription: A test skill.\n---\nNotes.\n")
    (folder / "skill.toml").write_text(rules)
    return folder


def sound(path: Path, *, seconds: float, rate: int = 24_000, channels: int = 1) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as out:
        out.setnchannels(channels)
        out.setsampwidth(2)
        out.setframerate(rate)
        out.writeframes(b"\0\0" * channels * int(rate * seconds))


def move(name: str, *, step_ms: int = 500, frames: list | None = None) -> dict:
    """A move lamp-body takes: 2 s, from rest to a small nod and back."""
    return {"name": name, "step_ms": step_ms, "frames": frames or [[0] * 5, [40, 0, 0, 0, 0], [40, 0, 0, 0, 0], [0, 0, 0, 0, 0], [0] * 5]}


def test_every_skill_in_the_repo_passes():
    # Outside the repository (an installed copy) there are no skills to read.
    folders = [p.parent for root in ("skills", "examples") for p in (REPO / root).glob("*/skill.toml")]
    if not folders:
        pytest.skip("not in the repository")
    failed = {f.name: check(f).errors for f in folders if not check(f).ok}
    assert not failed, failed
    assert len(folders) >= 32


def test_a_plain_skill_passes(tmp_path):
    report = check(skill(tmp_path / "say-hello", rules='version = "1.2.3"\n'))
    assert report.ok, report.errors
    assert report.warnings == []


def test_skill_md_must_name_the_folder_and_describe_it(tmp_path):
    empty = tmp_path / "empty"
    empty.mkdir()
    assert "SKILL.md: missing" in check(empty).errors
    other = check(skill(tmp_path / "say-hello", name="say-goodbye"))
    assert any("isn't the folder's name" in line for line in other.errors)
    bare = check(skill(tmp_path / "no-matter", md="# no front matter\n"))
    assert any("begins with front matter" in line for line in bare.errors)
    nameless = check(skill(tmp_path / "nameless", md="---\nname: nameless\n---\n"))
    assert any(line.startswith("SKILL.md: the front matter has no description") for line in nameless.errors)


def test_a_misspelt_key_or_a_bad_version_is_refused_and_located(tmp_path):
    report = check(skill(tmp_path / "typo", rules='version = "1.0"\nverison = "1.0.0"\n'))
    assert any(line.startswith("skill.toml: version:") for line in report.errors), report.errors
    assert any("verison" in line for line in report.errors), report.errors
    broken = check(skill(tmp_path / "broken", rules="version = \n"))
    assert any(line.startswith("skill.toml: not TOML") for line in broken.errors)


def test_a_script_is_refused(tmp_path):
    folder = skill(tmp_path / "scripted")
    (folder / "run.py").write_text("print('hi')\n")
    assert "run.py: a script; script skills are a later format" in check(folder).errors


def test_sounds_and_moves_are_held_to_the_lamps_bounds(tmp_path):
    folder = skill(tmp_path / "noisy")
    sound(folder / "sounds" / "ok.wav", seconds=1.0)
    sound(folder / "sounds" / "long.wav", seconds=3.5)
    sound(folder / "sounds" / "stereo.wav", seconds=1.0, channels=2)
    (folder / "moves").mkdir()
    (folder / "moves" / "sway.json").write_text(json.dumps(move("shake")))
    errors = check(folder).errors
    assert "sounds/long.wav: 3.5 s, longer than 3 s" in errors
    assert "sounds/stereo.wav: must be 16-bit mono PCM at 24 kHz" in errors
    assert "moves/sway.json: its name 'shake' isn't 'sway'" in errors
    assert not any(line.startswith("sounds/ok.wav") for line in errors)


def test_format_1s_look_warns_that_it_is_the_picture(tmp_path):
    report = check(skill(tmp_path / "seer", rules='version = "1.0.0"\n[permissions]\nlook = true\n'))
    assert report.ok, report.errors
    assert any("full picture" in line and "camera" in line for line in report.warnings)
    asked = check(skill(tmp_path / "asked", rules='version = "1.0.0"\n[permissions]\nlook = true\ncamera = true\n'))
    assert asked.ok and asked.warnings == [], (asked.errors, asked.warnings)


def test_the_command_says_each_folder_and_fails_on_an_error(tmp_path, capsys):
    good = skill(tmp_path / "good")
    bad = skill(tmp_path / "bad", rules="nope = 1\n")
    assert main(["skill", "check", str(good)]) == 0
    assert "good: ok, format 1" in capsys.readouterr().out
    assert main(["skill", "check", str(good), str(bad)]) == 1
    out = capsys.readouterr().out
    assert "good: ok, format 1" in out and "bad: error: skill.toml:" in out


def test_the_packaged_format_schema_is_the_repositorys():
    docs = REPO / "docs" / "skill-format.schema.json"
    if not docs.is_file():
        pytest.skip("not in the repository")
    packaged = Path(__file__).resolve().parents[1] / "src" / "lampos" / "skill-format.schema.json"
    assert packaged.read_bytes() == docs.read_bytes()


# The lamp's own front-matter reading (lamp-skills' `front_matter`): what it
# refuses is refused here, never a false pass (the reviewer-lead's P2 on #3840).
@pytest.mark.parametrize(
    "md, says",
    [
        ('---\nname: "quoted"\ndescription: A.\n---\n', "name must be 1 to 64"),
        ("--- \nname: spaced\ndescription: A.\n---\n", "begins with front matter"),
        ("---\r\nname: crlf\r\ndescription: A.\r\n---\r\n", "begins with front matter"),
        ("---\nname : gap\ndescription: A.\n---\n", "has no name"),
        ("---\nname: open\ndescription: A.\n", "begins with front matter"),
    ],
)
def test_front_matter_is_read_as_the_lamp_reads_it(tmp_path, md, says):
    report = check(skill(tmp_path / "quoted", md=md))
    assert any(says in line for line in report.errors), report.errors


def test_nothing_in_a_folder_makes_the_check_raise(tmp_path):
    folder = skill(tmp_path / "odd")
    (folder / "SKILL.md").write_bytes("---\nname: odd\n---\n".encode("utf-16"))
    (folder / "moves").mkdir()
    (folder / "moves" / "dir.json").mkdir()
    (folder / "moves" / "gone.json").symlink_to(tmp_path / "nowhere.json")
    (folder / "sounds").mkdir()
    (folder / "sounds" / "dir.wav").mkdir()
    errors = check(folder).errors
    assert "SKILL.md: not UTF-8 text (saved as UTF-16, say)" in errors
    assert "moves/gone.json: a link; the lamp never reads through one" in errors
    assert "sounds/dir.wav: not a file" in errors
    bom = skill(tmp_path / "bom", md="\ufeff---\nname: bom\ndescription: A.\n---\n")
    assert any("byte-order mark" in line for line in check(bom).errors)


def test_an_unreadable_file_is_an_error_not_a_raise(tmp_path):
    folder = skill(tmp_path / "locked")
    (folder / "moves").mkdir()
    locked = folder / "moves" / "locked.json"
    locked.write_text(json.dumps(move("locked")))
    locked.chmod(0)
    try:
        if os.access(locked, os.R_OK):
            pytest.skip("running as a user who reads every file")
        assert any(line.startswith("moves/locked.json: can't be read") for line in check(folder).errors)
    finally:
        locked.chmod(0o644)


def test_the_command_goes_on_past_a_folder_it_cant_read(tmp_path, capsys):
    odd = skill(tmp_path / "odd")
    (odd / "SKILL.md").write_bytes(b"\xff\xfe---")
    good = skill(tmp_path / "good")
    assert main(["skill", "check", str(odd), str(good)]) == 1
    assert "good: ok, format 1" in capsys.readouterr().out


def test_moves_are_held_as_the_body_holds_them(tmp_path):
    folder = skill(tmp_path / "mover")
    (folder / "moves").mkdir()
    cases = {
        "ok": move("ok"),
        "extra": {**move("extra"), "speed": 2},
        "nameless": {"step_ms": 500, "frames": move("x")["frames"]},
        "nod": move("nod"),
        "fast": move("fast", step_ms=20),
        "short": move("short", frames=[[0] * 5, [0] * 5]),
        "far": move("far", frames=[[0] * 5, [300, 0, 0, 0, 0], [0] * 5, [0] * 5, [0] * 5]),
        "rest": move("rest", frames=[[0] * 5, [10, 0, 0, 0, 0], [10, 0, 0, 0, 0], [10, 0, 0, 0, 0], [10, 0, 0, 0, 0]]),
    }
    for name, body in cases.items():
        (folder / "moves" / f"{name}.json").write_text(json.dumps(body))
    errors = check(folder).errors
    said = lambda name: [line for line in errors if line.startswith(f"moves/{name}.json")]  # noqa: E731
    assert said("ok") == []
    for name in ("extra", "nameless"):
        assert said(name) == [f"moves/{name}.json: a move is JSON of exactly name, step_ms and frames"]
    assert said("nod") == ["moves/nod.json: 'nod' is one of the body's own gestures"]
    assert said("fast") == ["moves/fast.json: step_ms 20 is not from 40 to 1000"]
    assert said("short") == ["moves/short.json: it lasts 500 ms, not 1500 to 10000"]
    assert any("farther than 200" in line for line in said("far"))
    assert said("rest") == ["moves/rest.json: it does not begin and end at nothing"]


def test_a_script_without_a_suffix_is_refused(tmp_path):
    folder = skill(tmp_path / "hidden")
    (folder / "notes").mkdir()
    (folder / "notes" / "run").write_text("#!/bin/sh\necho hi\n")
    tool = folder / "notes" / "tool"
    tool.write_text("plain\n")
    tool.chmod(0o755)
    errors = check(folder).errors
    assert "notes/run: a script; script skills are a later format" in errors
    assert "notes/tool: a script; script skills are a later format" in errors


def test_links_the_lamp_reads_are_refused_and_others_warned(tmp_path):
    source = skill(tmp_path / "elsewhere" / "linked")
    folder = tmp_path / "linked"
    folder.mkdir()
    (folder / "SKILL.md").symlink_to(source / "SKILL.md")
    (folder / "skill.toml").write_text('version = "1.0.0"\n')
    (folder / "notes.md").symlink_to(source / "SKILL.md")
    report = check(folder)
    assert "SKILL.md: a link; the lamp never reads through one" in report.errors
    assert any(line.startswith("notes.md: a link") for line in report.warnings)


def test_an_unknown_top_level_key_is_named_plainly(tmp_path):
    report = check(skill(tmp_path / "typo", rules='version = "1.0.0"\n[[triggerz]]\n'))
    assert "skill.toml: [triggerz] isn't a key of format 1" in report.errors
