"""Marks each cut that the lamp's own speech overlaps (28 Nov: lamp-voice answered the played clips
through much of session 1), from the lamp's speaking intervals and the cut's frame timeline.

    python -I overlap.py CUT_DIR INTERVALS.tsv [--out lamp-manifest-overlap.tsv]

INTERVALS.tsv: one interval a line, a header naming a start and an end column (`start_unix_ms`/
`end_unix_ms`, `start`/`end`, or `from`/`to`) and perhaps a `kind` (speech, wait, ring: each kind's
overlap with the cut in a column `lamp_<kind>_s` of its own), each an ISO time (2026-11-28T08:24:13.123+07:00, or with a space for the T; local
+07:00 if none is given), or unix seconds or milliseconds. CUT_DIR is lampcut.py's output
(lamp-manifest.tsv, timeline.npz). Writes the manifest again with these columns more: `condition`
(the strongest of the lamp's sounds in the cut: "over the lamp's speech", "... ring", "... wait", or
"no lamp sound"), `lamp_sound_cut_s` (every kind within the cut: 0.3 s before the clip to 0.5 s
after it), `lamp_sound_clip_s` (within the clip itself) and `lamp_sound_share` (of the clip). Also reads the
cut's own gaps: `lead_dbfs`, the power of its 0.3 s lead (the speaker's hiss alone is about -75).
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lampcut as L  # noqa: E402

RATE = 16_000
TZ = dt.timezone(dt.timedelta(hours=7))


def to_unix_ms(v: str) -> float:
    v = v.strip()
    try:
        x = float(v)
        return x if x > 1e11 else x * 1000.0
    except ValueError:
        pass
    t = dt.datetime.fromisoformat(v.replace(" ", "T", 1) if "T" not in v else v)
    if t.tzinfo is None:
        t = t.replace(tzinfo=TZ)
    return t.timestamp() * 1000.0


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("cuts")
    p.add_argument("intervals")
    p.add_argument("--out", default="lamp-manifest-overlap.tsv")
    p.add_argument("--audible-until", default="", help="the lamp's speech after this time (muted) is not counted, e.g. 2026-11-28T10:12:35")
    a = p.parse_args()
    cuts = Path(a.cuts)
    tl = np.load(cuts / "timeline.npz")
    sample, unix = tl["track_sample"].astype(np.float64), tl["at_unix_ms"].astype(np.float64)
    rows = list(csv.DictReader(open(a.intervals, encoding="utf-8"), delimiter="\t"))
    keys = rows[0].keys() if rows else []
    s_key = next(k for k in ("start_unix_ms", "start", "from", "begin") if k in keys)
    e_key = next(k for k in ("end_unix_ms", "end", "to", "stop") if k in keys)
    kinds = sorted({r.get("kind", "speech") for r in rows}) or ["speech"]
    until = to_unix_ms(a.audible_until) if a.audible_until else float("inf")
    by_kind = {}
    for kind in kinds:
        sel = [r for r in rows if r.get("kind", "speech") == kind]
        spans = [(to_unix_ms(r[s_key]), min(to_unix_ms(r[e_key]), until)) for r in sel]
        by_kind[kind] = np.array(sorted(x for x in spans if x[1] > x[0]), np.float64).reshape(-1, 2)
    iv = np.concatenate(list(by_kind.values())) if by_kind else np.zeros((0, 2))
    man = list(csv.DictReader(open(cuts / "lamp-manifest.tsv", encoding="utf-8"), delimiter="\t"))
    out = []
    for r in man:
        r = dict(r)
        if r.get("lamp_file"):
            a0 = float(r["cut_start_s"]) * RATE
            c0 = a0 + 0.3 * RATE
            c1 = c0 + float(r["duration_s"]) * RATE
            a1 = c1 + 0.5 * RATE
            # Track samples to the lamp's wall clock by the frames' stamps.
            u = np.interp([a0, c0, c1, a1], sample, unix)

            def over(lo: float, hi: float, spans=None) -> float:
                spans = iv if spans is None else spans
                if not len(spans):
                    return 0.0
                return float(np.clip(np.minimum(spans[:, 1], hi) - np.maximum(spans[:, 0], lo), 0, None).sum() / 1000.0)

            cut_s, clip_s = over(u[0], u[3]), over(u[1], u[2])
            per = {kind: over(u[0], u[3], spans) for kind, spans in by_kind.items()}
            for kind, v in per.items():
                r[f"lamp_{kind}_s"] = round(v, 2)
            # One condition a cut, the strongest sound of the lamp's in it.
            r["condition"] = next((f"over the lamp's {k}" for k in ("speech", "ring", "wait") if per.get(k, 0) > 0), "no lamp sound")
            lead = L.read_wav(cuts / r["lamp_file"])[: int(0.3 * RATE)].astype(np.float64)
            r.update(lamp_sound_cut_s=round(cut_s, 2), lamp_sound_clip_s=round(clip_s, 2),
                     lamp_sound_share=round(clip_s / max(float(r["duration_s"]), 1e-3), 3), lead_dbfs=round(L.power_db(lead), 1))
        out.append(r)
    cols = list(man[0].keys()) + ["condition", "lamp_sound_cut_s", "lamp_sound_clip_s", "lamp_sound_share"] + [f"lamp_{k}_s" for k in kinds] + ["lead_dbfs"]
    with open(cuts / a.out, "w", encoding="utf-8") as f:
        w = csv.DictWriter(f, cols, delimiter="\t", lineterminator="\n", extrasaction="ignore", restval="")
        w.writeheader()
        w.writerows(out)
    got = [r for r in out if r.get("lamp_file")]
    from collections import Counter
    hit = [r for r in got if float(r["lamp_sound_cut_s"]) > 0]
    print(f"{len(iv)} intervals; {len(hit)} of {len(got)} cuts overlap a sound of the lamp's "
          f"({sum(float(r['lamp_sound_clip_s']) > 0 for r in got)} within the clip itself); by condition "
          f"{dict(Counter(r['condition'] for r in got))}; {cuts / a.out}")


if __name__ == "__main__":
    main()
