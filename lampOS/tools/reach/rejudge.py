"""Judge a map again with other masses, another tip limit or another margin, without redoing the CAD part.

    python3 rejudge.py IN.npz OUT.npz [--lean FILE] [--tip-mm 22] [--margin-mm 2]

reach.py's slow part (parts against parts, 15 minutes) does not depend on the masses or the limits, and it keeps
each pose's least clearance and lowest point in map.npz; only the centre of mass (a second or two) and the
rules that read these numbers are done again here. Margins can only be made wider than the clearance the map was built
with allows (the map's own least clearance is the floor: a smaller margin than it was built with changes nothing for
poses it refused, which `reach.py` cannot give back).
"""
from __future__ import annotations

import argparse
import json

import numpy as np

import body
import reach


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("dst")
    ap.add_argument("--lean", default=body.LEAN_JSON)
    ap.add_argument("--tip-mm", type=float, default=reach.CLIP_MM)
    ap.add_argument("--margin-mm", type=float, default=2.0)
    args = ap.parse_args()
    m = np.load(args.src)
    sh, el, pi = m["shoulder"], m["elbow"], m["pitch"]
    S, E, P = np.meshgrid(sh, el, pi, indexing="ij")
    counts = np.tile(reach.STAND, (S.size, 1))
    counts[:, 1], counts[:, 2], counts[:, 4] = S.ravel(), E.ravel(), P.ravel()
    lean = body.Lean(args.lean)
    off = lean.off_axis_mm(counts).reshape(S.shape)
    back = lean.lean_back_mm(counts).reshape(S.shape)
    servo = np.zeros(S.shape, np.uint8)  # reach.py's span never refused a pose on this grid
    assert not (m["why"] & reach.SERVO_BIT).any()
    why = (servo | np.where(off <= args.tip_mm, 0, reach.TIP_BIT) | np.where(m["clearance_mm"] >= args.margin_mm, 0, reach.HIT_BIT)
           | np.where(m["above_table_mm"] >= args.margin_mm, 0, reach.TABLE_BIT)).astype(np.uint8)
    np.savez_compressed(args.dst, shoulder=sh, elbow=el, pitch=pi, why=why, off_axis_mm=off.astype(np.float32),
                        lean_back_mm=back.astype(np.float32), clearance_mm=m["clearance_mm"], above_table_mm=m["above_table_mm"])
    print(json.dumps({"share_safe": float((why == 0).mean()), "tip": float(((why & reach.TIP_BIT) > 0).mean()),
                      "hit": float(((why & reach.HIT_BIT) > 0).mean()), "table": float(((why & reach.TABLE_BIT) > 0).mean())}))


if __name__ == "__main__":
    main()
