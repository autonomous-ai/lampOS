"""Ask a map (reach.py's map.npz) where a joint may go with the others held.

    python3 query.py MAP.npz SHOULDER ELBOW PITCH [--shrink COUNTS]
    python3 query.py MAP.npz --export OUT.json [--shrink COUNTS]
    python3 query.py MAP.npz --export OUT.json --unit lamp-75c9 --folded 2076,2045 --head-home 1772

Poses are servo counts (the bus's). `--shrink` takes that many counts off each
end of a run of safe poses: a margin in position on top of the margin in the
rules. An unsafe pose answers None for every joint. `--export` writes the whole
map as plain JSON for a page or a test: the three axes and, for each
shoulder x elbow, the safe pitch runs as [low, high] pairs.

A per-lamp file (`--unit`, `--folded SHOULDER,ELBOW`, `--head-home PITCH`, the lamp's own folded arm and head home in its
counts) keeps the map's axes as they are and adds `unit` and `frame`: the lamp's counts minus `frame.offset` are the map's
counts (`to_map_counts`). The CAD, the masses and the geometry stay lamp-4ace's and V1's until a lamp's own are measured; only the
encoder zero moves. A consumer takes the file only for the unit it names (`for_unit`).
"""
from __future__ import annotations

import argparse
import json

import numpy as np


def runs(values: np.ndarray, ok: np.ndarray) -> list[tuple[float, float]]:
    """The maximal runs of safe grid points along one axis, as (low, high) values."""
    out, start = [], None
    for i, good in enumerate(ok):
        if good and start is None:
            start = i
        if start is not None and (not good or i == len(ok) - 1):
            end = i if good else i - 1
            out.append((float(values[start]), float(values[end])))
            start = None
    return out


def range_at(map_npz, pose: tuple[float, float, float], shrink: float = 0.0) -> dict[str, tuple[float, float] | None]:
    """Each of shoulder, elbow and pitch: (lowest, highest) counts it may be at, the others held at `pose`
    (a grid's nearest point), or None when the pose itself is not safe or the run is shorter than the margin."""
    axes = {"shoulder": map_npz["shoulder"], "elbow": map_npz["elbow"], "pitch": map_npz["pitch"]}
    ok = map_npz["why"] == 0
    at = [int(np.argmin(np.abs(axes[name] - value))) for name, value in zip(axes, pose)]
    lines = {"shoulder": ok[:, at[1], at[2]], "elbow": ok[at[0], :, at[2]], "pitch": ok[at[0], at[1], :]}
    answer: dict[str, tuple[float, float] | None] = {}
    for k, (name, values) in enumerate(axes.items()):
        mine = None
        for low, high in runs(values, lines[name]):
            if low - 1e-9 <= values[at[k]] <= high + 1e-9:
                mine = (low + shrink, high - shrink)
        answer[name] = mine if mine and mine[0] <= pose[k] <= mine[1] else None
    return answer


# The lamp the map's counts are laid out from: lamp-4ace's folded arm and head home (reach.py FOLDED / HOME).
MAP_FRAME = {"shoulder": 2048.0, "elbow": 2047.0, "pitch": 1772.0}


def frame_of(unit: str, folded: tuple[float, float], head_home: float) -> dict:
    """A lamp's frame: where its own counts sit against the map's. Same sign, same scale; only the zero moves."""
    own = {"shoulder": float(folded[0]), "elbow": float(folded[1]), "pitch": float(head_home)}
    return {"unit": unit, "map": dict(MAP_FRAME), "lamp": own, "offset": {k: own[k] - MAP_FRAME[k] for k in MAP_FRAME}}


def to_map_counts(frame: dict | None, pose: dict) -> dict:
    """A lamp's pose (`shoulder`, `elbow`, `pitch` in its counts) in the map's counts; no frame = the map's own lamp."""
    off = (frame or {}).get("offset", {})
    return {k: pose[k] - off.get(k, 0.0) for k in ("shoulder", "elbow", "pitch")}


def for_unit(map_json: dict, unit: str) -> bool:
    """Whether a per-lamp file may be used on `unit`: only the lamp it names (a file with no unit is the page's lamp-4ace map)."""
    return map_json.get("unit") == unit


def export(map_npz, shrink: float, label: str = "", unit: str = "", folded=None, head_home=None) -> dict:
    shoulder, elbow, pitch = map_npz["shoulder"], map_npz["elbow"], map_npz["pitch"]
    ok = map_npz["why"] == 0
    pitch_runs = [[[list(r) for r in runs(pitch, ok[i, j, :])] for j in range(len(elbow))] for i in range(len(shoulder))]
    out = {"label": label, "axes": {"shoulder": shoulder.tolist(), "elbow": elbow.tolist(), "pitch": pitch.tolist()},
           "shrink_counts": shrink, "safe_pitch_runs": pitch_runs,
           "note": "safe_pitch_runs[i][j] = runs of safe head pitch (counts) at shoulder[i], elbow[j]; empty: no pitch is safe there"}
    if unit:
        if folded is None or head_home is None:
            raise ValueError("a per-lamp export needs the lamp's folded arm and head home")
        out["unit"] = unit
        out["frame"] = frame_of(unit, folded, head_home)
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("map")
    ap.add_argument("pose", nargs="*", type=float)
    ap.add_argument("--shrink", type=float, default=0.0)
    ap.add_argument("--export")
    ap.add_argument("--unit", default="", help="a per-lamp export: the lamp this file is for (needs --folded and --head-home)")
    ap.add_argument("--folded", help="the lamp's folded SHOULDER,ELBOW counts")
    ap.add_argument("--head-home", type=float, help="the lamp's head pitch home, counts")
    ap.add_argument("--label", default="", help="what the map stands for (the masses it was judged with), kept in the export")
    args = ap.parse_args()
    data = np.load(args.map)
    if args.export:
        json.dump(export(data, args.shrink, args.label, args.unit, tuple(float(v) for v in args.folded.split(",")) if args.folded else None, args.head_home), open(args.export, "w"), separators=(",", ":"))
        return
    if len(args.pose) != 3:
        ap.error("give SHOULDER ELBOW PITCH, or --export")
    print(json.dumps(range_at(data, tuple(args.pose), args.shrink)))


if __name__ == "__main__":
    main()
