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


def plays(*names: str) -> str:
    """Rules whose `[[do]]` play these moves: the lamp loads them with the
    skill, so a move that fails refuses the skill."""
    return 'version = "0.1.0"\n[permissions]\nlight = true\n' + "".join(f'[[do]]\nbody = {{ move = "{name}" }}\n' for name in names)


def test_sounds_and_moves_are_held_to_the_lamps_bounds(tmp_path):
    folder = skill(tmp_path / "noisy", rules=plays("sway"))
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
    folder = skill(tmp_path / "locked", rules=plays("locked"))
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
    folder = skill(tmp_path / "mover", rules=plays("ok", "extra", "nameless", "nod", "fast", "short", "far", "rest"))
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


def test_a_recorded_move_the_skill_does_not_play_is_a_warning_never_stricter_than_the_lamp(tmp_path):
    """The lamp installs a skill whose unplayed moves/ files fail the body's
    check and leaves them out of `play`: the check says so, and goes on."""
    folder = skill(tmp_path / "recorded")
    (folder / "moves").mkdir()
    (folder / "moves" / "ok.json").write_text(json.dumps(move("ok")))
    (folder / "moves" / "far.json").write_text(json.dumps(move("far", frames=[[0] * 5, [300, 0, 0, 0, 0], [0] * 5, [0] * 5, [0] * 5])))
    report = check(folder)
    assert report.ok, report.errors
    (said,) = [line for line in report.warnings if line.startswith("moves/far.json")]
    assert "farther than" in said and "leaves it out" in said
    assert not [line for line in report.warnings if line.startswith("moves/ok.json")]
    # Played by the skill's own rules, the same file refuses the skill.
    played = check(skill(tmp_path / "played", rules=plays("far")))
    assert played.ok  # no file: the rule's move is missing is the lamp's load to say
    (tmp_path / "played" / "moves").mkdir()
    (tmp_path / "played" / "moves" / "far.json").write_text(json.dumps(move("far", frames=[[0] * 5, [300, 0, 0, 0, 0], [0] * 5, [0] * 5, [0] * 5])))
    assert any(line.startswith("moves/far.json") and "farther than" in line for line in check(tmp_path / "played").errors)


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


def test_the_first_name_line_is_the_name_even_empty(tmp_path):
    # The lamp's `find_map` stops at the first `name:` line.
    report = check(skill(tmp_path / "e", md="---\nname:\nname: e\ndescription: A.\n---\n"))
    assert any("has no name" in line for line in report.errors), report.errors


def test_a_directory_named_as_a_move_is_not_a_file(tmp_path):
    folder = skill(tmp_path / "dirmove", rules=plays("x"))
    (folder / "moves" / "x.json").mkdir(parents=True)
    assert "moves/x.json: not a file" in check(folder).errors


def test_a_dot_named_move_is_passed_over_as_the_body_does(tmp_path):
    # A Mac's AppleDouble `._wave.json`: lamp-body's `moves::load` skips it.
    folder = skill(tmp_path / "mac")
    (folder / "moves").mkdir()
    (folder / "moves" / "._wave.json").write_bytes(b"\0\0mac")
    assert check(folder).ok, check(folder).errors


# Format 2 (#3878), as lamp-skills reads it: `look` is presence alone, the
# picture is `camera` only, and a format other than 1 or 2 is refused.
def test_format_2s_look_is_presence_and_warns_nothing(tmp_path):
    report = check(skill(tmp_path / "f2", rules='format = 2\nversion = "1.0.0"\n[permissions]\nlook = true\n'))
    assert report.ok and report.warnings == [] and report.format == 2, (report.errors, report.warnings)


def test_a_look_step_needs_the_picture_as_the_lamp_reads_it(tmp_path):
    step = '[[do]]\nlook = true\nsay = "What do you see?"\n'
    f2 = check(skill(tmp_path / "f2", rules=f'format = 2\nversion = "1.0.0"\n[permissions]\nlook = true\nspeak = true\n{step}'))
    assert any("look = true step" in line for line in f2.errors), f2.errors
    with_camera = check(skill(tmp_path / "cam", rules=f'format = 2\nversion = "1.0.0"\n[permissions]\ncamera = true\nspeak = true\n{step}'))
    assert not any("look = true step" in line for line in with_camera.errors), with_camera.errors
    f1 = check(skill(tmp_path / "f1", rules=f'version = "1.0.0"\n[permissions]\nlook = true\nspeak = true\n{step}'))
    assert not any("look = true step" in line for line in f1.errors), f1.errors


def test_a_format_the_lamp_doesnt_read_is_refused_by_name(tmp_path):
    report = check(skill(tmp_path / "f3", rules='format = 3\nversion = "1.0.0"\n'))
    assert any("format 3" in line for line in report.errors), report.errors
    # The lamp reads format as a u32: 2.0 is "invalid type: floating point" there (Fourteen's P3).
    float_two = check(skill(tmp_path / "f20", rules='format = 2.0\nversion = "1.0.0"\n'))
    assert any("format 2.0" in line for line in float_two.errors), float_two.errors


def test_the_command_names_the_format(tmp_path, capsys):
    folder = skill(tmp_path / "two", rules='format = 2\nversion = "1.0.0"\n')
    assert main(["skill", "check", str(folder)]) == 0
    assert "two: ok, format 2" in capsys.readouterr().out


# A skill whose task hands a script to the lamp's agent (`do.task`, format 2): the lamp takes it from a pushed
# package (lamp-75c9, 13 Nov, stable 12587: `blue-task`, asks light, on), so the check off the lamp must never
# refuse what the lamp accepts, and must refuse what the lamp refuses in the lamp's own words
# (crates/skills lib.rs `do.task` rules, task.rs `run_ok` and `grants_of`).
TASK_RULES = (
    'format = 2\nversion = "1.0.0"\ngrants = ["light"]\ntool = "blue_task"\n\n[permissions]\nlight = true\n\n'
    '[[do]]\ntask = { task = "Breathe the light blue for a few seconds", run = "scripts/main.py" }\n'
)


def task_skill(folder: Path, *, rules: str = TASK_RULES, run: str = "scripts/main.py", script: str | None = "print('x')\n") -> Path:
    folder = skill(folder, rules=rules)
    if script is not None:
        target = folder / run
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(script)
    return folder


def test_a_skill_with_a_task_and_its_script_passes(tmp_path):
    report = check(task_skill(tmp_path / "blue-task"))
    assert report.ok, report.errors
    assert report.format == 2


def test_a_script_beside_a_task_is_taken_under_scripts_only(tmp_path):
    folder = task_skill(tmp_path / "blue-task")
    (folder / "scripts" / "helper.py").write_text("print('h')\n")
    assert check(folder).ok
    (folder / "extra.py").write_text("print('e')\n")
    assert any("extra.py: a script" in e for e in check(folder).errors)


def test_a_script_with_no_task_is_still_refused(tmp_path):
    folder = skill(tmp_path / "plain")
    (folder / "scripts").mkdir()
    (folder / "scripts" / "main.py").write_text("print('x')\n")
    assert any("scripts/main.py: a script" in e for e in check(folder).errors)


@pytest.mark.parametrize(
    "rules, says",
    [
        (TASK_RULES + '[[do]]\ntask = { task = "again", run = "scripts/main.py" }\n', "one task"),
        (TASK_RULES.replace('grants = ["light"]\n', ""), None),  # no grants listed: the lamp takes a task with none
        ('version = "1.0.0"\ngrants = ["light"]\n[[do]]\nsay = "Hi."\n', "only beside one"),
        (TASK_RULES.replace('["light"]', '["skills"]'), "never a script's"),
        (TASK_RULES.replace('["light"]', '["telepathy"]'), "is not a grant"),
        (TASK_RULES.replace('["light"]', '["light", "light"]'), "listed twice"),
        (TASK_RULES.replace("Breathe the light blue for a few seconds", ""), "task is 1 to 500 bytes"),
        (TASK_RULES.replace("Breathe the light blue for a few seconds", "x" * 501), "task is 1 to 500 bytes"),
        (TASK_RULES.replace("Breathe the light blue for a few seconds", "a\\u0007b"), "task is 1 to 500 bytes"),
        (TASK_RULES.replace('task = {', 'light = { colour = "blue" }\ntask = {'), "alone in its [[do]] table"),
        (TASK_RULES.replace('version = "1.0.0"', 'version = "1.0.0"\nkind = "addon"'), "only an installable skill"),
    ],
)
def test_what_the_lamp_refuses_of_a_task_is_refused_in_its_words(tmp_path, rules, says):
    report = check(task_skill(tmp_path / "blue-task", rules=rules))
    if says is None:
        assert report.ok, report.errors
    else:
        assert any(says in e for e in report.errors), report.errors


@pytest.mark.parametrize(
    "run",
    ["main.py", "scripts", "../main.py", "scripts/../main.py", "scripts/.hidden.py", "/scripts/main.py", "scripts\\\\main.py", "scripts/a/b/c/d.py", "other/main.py"],
)
def test_a_run_outside_scripts_or_hidden_or_too_deep_is_refused(tmp_path, run):
    report = check(task_skill(tmp_path / "blue-task", rules=TASK_RULES.replace("scripts/main.py", run), script=None))
    assert any("do.task: run" in e for e in report.errors), report.errors


def test_a_run_missing_or_a_link_is_refused(tmp_path):
    missing = check(task_skill(tmp_path / "missing", script=None))
    assert any("is not in the skill's folder" in e for e in missing.errors), missing.errors
    folder = task_skill(tmp_path / "linked")
    (folder / "scripts" / "real.py").write_text("print('r')\n")
    (folder / "scripts" / "main.py").unlink()
    os.symlink("real.py", folder / "scripts" / "main.py")
    assert any("a link" in e for e in check(folder).errors)


MORNING_MAIL = (
    'version = "1.0.0"\ngrants = ["gmail-read", "gmail-write"]\ntool = "morning_mail"\n[permissions]\ntopics = ["clock/local"]\n'
    '[[do]]\ntask = { task = "Sum up this morning\'s mail and send it to me", run = "scripts/main.py" }\n'
)


def test_a_linked_services_grants_pass_the_check_with_a_warning_and_a_push_of_them_is_refused(tmp_path):
    from lampos.push import asks_beyond

    folder = task_skill(tmp_path / "morning-mail", rules=MORNING_MAIL)
    report = check(folder)
    assert report.ok, report.errors
    assert sum("only a Store or made skill carries one" in w for w in report.warnings) == 2
    # The lamp refuses a push that names one, whatever the pairing holds (package.rs, #6432), in its words.
    code, why = asks_beyond(folder, ["skills", "light", "body", "speak", "look"])
    assert code == "no_grant"  # package.rs gives no_grant to a refusal ending ", which this computer was not granted"
    assert why == "not installed: it asks gmail-read, a linked service's grant (a skill run's alone), which this computer was not granted"
    for bad in ("myspace-read", "gmail-app-read", "gmail-delete", "-read"):
        refused = check(task_skill(tmp_path / f"b{abs(hash(bad))}", rules=MORNING_MAIL.replace("gmail-write", bad)))
        assert any("is not a grant" in e for e in refused.errors), (bad, refused.errors)
    twice = check(task_skill(tmp_path / "twice", rules=MORNING_MAIL.replace("gmail-write", "gmail-read")))
    assert any("listed twice" in e for e in twice.errors)


@pytest.mark.parametrize("run", ["scripts//main.py", "scripts/./main.py", "scripts/main.py/"])
def test_a_run_the_lamp_folds_to_a_plain_path_passes(tmp_path, run):
    # Rust's Path::components drops empty and "." parts, so the lamp takes these as scripts/main.py.
    report = check(task_skill(tmp_path / "blue-task", rules=TASK_RULES.replace("scripts/main.py", run)))
    assert report.ok, report.errors


def test_a_leading_dot_slash_run_is_still_refused(tmp_path):
    report = check(task_skill(tmp_path / "blue-task", rules=TASK_RULES.replace("scripts/main.py", "./scripts/main.py")))
    assert any("do.task: run" in e for e in report.errors), report.errors


def test_a_push_of_a_task_skill_holds_each_grant_its_script_asks_of_the_pairing(tmp_path):
    from lampos.push import asks_beyond

    folder = task_skill(tmp_path / "blue-task")
    assert asks_beyond(folder, ["skills", "light"]) is None
    code, why = asks_beyond(folder, ["skills"])
    assert code == "no_grant" and "light" in why
    camera = task_skill(tmp_path / "pics", rules=TASK_RULES.replace('["light"]', '["camera"]'))
    assert asks_beyond(camera, ["skills", "light"]) is None, "a picture is held for the owner's yes, never refused here"


def test_a_task_skill_packs_and_the_fake_lamp_takes_the_push(tmp_path):
    import asyncio

    from lampos import Lamp
    from lampos.testing import FakeLamp

    folder = task_skill(tmp_path / "blue-task")

    async def go():
        async with FakeLamp(grants=("light", "skills")) as fake:
            async with await fake.connect() as lamp:
                return await lamp.push_skill(folder)

    assert asyncio.run(go()).get("name") == "blue-task"
