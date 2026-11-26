"""Store v1 holds the grants that ship today: `hear`, `speech`, `device` and `notes`. The manifest never holds `net`,
and not `turns`, `conversation`, `model` or `report` (Ten's, not built) until a later version asks for them."""

import json
from pathlib import Path

MANIFEST = json.loads((Path(__file__).resolve().parent.parent / "app.json").read_text())


def test_net_is_off():
    assert MANIFEST.get("net", False) is False


def test_net_is_not_among_the_grants():
    assert "net" not in MANIFEST["grants"]


def test_v1_grants_are_the_four_that_ship():
    assert sorted(MANIFEST["grants"]) == ["device", "hear", "notes", "speech"]


def test_min_build_is_the_first_build_with_the_apps_panel():
    # 14268 is the lowest `lampos-lamp-voice-N` whose commit holds 5fcede282c (app.panel.set), read on 26 Nov.
    assert MANIFEST["min_build"] == 14268


def test_the_entry_module_exists():
    here = Path(__file__).resolve().parent.parent / "src"
    module = MANIFEST["runtime"]["entry"].replace(".", "/")
    assert (here / f"{module}.py").is_file()
