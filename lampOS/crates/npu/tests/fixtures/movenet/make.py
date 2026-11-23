"""Makes lamp-npu's MoveNet fixtures from MoveNet's own decode and mn.py's crops.

usage: make.py MN_DIR SIM_DIR TFLITE OUT_DIR FRAME.jpg ...

MN_DIR holds mn.py (its crop helpers; the decode is this file's own,
below); SIM_DIR the converter's simulated int8 head dumps
(iter_N_*.qnt.tensor); TFLITE the float MoveNet whose output is 17 x
[y, x, score]. Writes heads.json (int8 maps of two runs, sparse, and the
keypoints `decode` below reads from them), track.json (each frame's keypoints on
the whole frame and the crop mn.crop_region takes next) and crop.json
(mn.crop_image of a made picture, sampled).
"""
import glob, json, os, sys
import numpy as np, cv2

mn_dir, sim, tfl, out = sys.argv[1:5]
frames = sys.argv[5:]
sys.path.insert(0, mn_dir)
import mn  # noqa: E402

G, EPS = 48, 1.7998046875
ys, xs = np.mgrid[0:G, 0:G].astype(np.float32)
cw = 1.0 / (np.sqrt((ys - G / 2) ** 2 + (xs - G / 2) ** 2) + EPS)
c = np.load(os.path.join(mn_dir, "ml_consts.npz"))
assert np.abs(c["cw"].reshape(G, G) - cw).max() < 2e-4

QUANT = {"Sigmoid_1_142": (0.003435490420088172, -128), "Sigmoid_143": (0.003266087267547846, -128),
         "kpt_offset": (0.1634388267993927, -10), "kpt_regress": (0.6235443949699402, -17)}

def load(it):
    maps = []
    for key in ["Sigmoid_1_142", "Sigmoid_143", "kpt_offset", "kpt_regress"]:
        (f,) = [f for f in glob.glob(f"{sim}/iter_{it}_*.qnt.tensor") if os.path.basename(f).startswith(f"iter_{it}_{key}")]
        q = np.loadtxt(f, dtype=np.int64)
        maps.append((q, *QUANT[key]))
    return maps

def decode(heads):
    """MoveNet's own decode, as its float graph does it: the person is the
    centre map times the centre weight cw = 1 / (distance from the middle
    + EPS), so the one nearest the middle wins among equals; each
    keypoint is the heat divided by (its distance from where the centre's
    regression puts it + EPS). Checked against the float model's own
    output on 13 frames: within 2e-4 (mn.decode's centre / cw was off by
    up to 0.03 on 6 of them)."""
    cm, h, off, reg = [np.asarray(a, np.float32).reshape(G, G, -1) for a in heads]
    cy, cx = divmod(int(np.argmax(cm[..., 0] * cw)), G)
    r = reg[cy, cx].reshape(17, 2)
    ry, rx = cy + r[:, 0], cx + r[:, 1]
    d = np.sqrt((ys[..., None] - ry) ** 2 + (xs[..., None] - rx) ** 2) + EPS
    ky, kx = np.divmod((h / d).reshape(-1, 17).argmax(0), G)
    o = off[ky, kx].reshape(17, 17, 2)[np.arange(17), np.arange(17)]
    return np.stack([(ky + o[:, 0]) / G, (kx + o[:, 1]) / G, h[ky, kx, np.arange(17)]], 1)

def deq(maps):
    return [((q - z) * s).astype(np.float32) for q, s, z in maps]

heads = []
for it in [0, 5]:
    maps = load(it)
    dense = decode(deq(maps))
    # Keep what the decode reads: centre and heat whole, offset and regress
    # only at the cells read; the rest at their zero point (a real 0).
    cmap, heat, off, reg = deq(maps)
    ci = int(np.argmax(cmap.reshape(G, G) * cw)); cy, cx = divmod(ci, G)
    keep_reg = {cy * G + cx}
    kept = [[q.copy(), s, z] for q, s, z in maps]
    # offset cells: the keypoints' argmax cells
    r = reg.reshape(G, G, 34)[cy, cx].reshape(17, 2)
    ry, rx = cy + r[:, 0], cx + r[:, 1]
    d = np.sqrt((ys[..., None] - ry) ** 2 + (xs[..., None] - rx) ** 2) + EPS
    ki = (heat.reshape(G, G, 17) / d).reshape(-1, 17).argmax(0)
    keep_off = set(int(i) for i in ki)
    for n, keep in [(2, keep_off), (3, keep_reg)]:
        q, s, z = kept[n]
        q = q.reshape(G * G, 34)
        mask = np.ones(G * G, bool); mask[list(keep)] = False
        q[mask] = z
        kept[n][0] = q.reshape(-1)
    sparse = decode(deq(kept))
    assert np.abs(sparse - dense).max() == 0, "the sparse maps decode the same"
    entry = {"maps": [], "keypoints": dense.round(6).tolist()}
    for q, s, z in kept:
        vals, cnt = np.unique(q, return_counts=True)
        fill = int(vals[cnt.argmax()])
        at = np.nonzero(q != fill)[0]
        entry["maps"].append({"channels": q.size // (G * G), "scale": s, "zero": z, "fill": fill,
                              "at": np.stack([at, q[at]], 1).reshape(-1).tolist()})
    heads.append(entry)

from ai_edge_litert.interpreter import Interpreter  # noqa: E402
net = Interpreter(tfl); net.allocate_tensors()
inp, outp = net.get_input_details()[0]["index"], net.get_output_details()[0]["index"]
def run(rgb, region):
    net.set_tensor(inp, mn.crop_image(rgb, region, 192)[None]); net.invoke()
    H, W = rgb.shape[:2]
    return mn.to_frame(net.get_tensor(outp)[0, 0], region, H, W)

track = []
for f in frames:
    rgb = cv2.cvtColor(cv2.imread(f), cv2.COLOR_BGR2RGB); H, W = rgb.shape[:2]
    k1 = run(rgb, mn.full_region(H, W))
    r2 = mn.crop_region(k1, H, W)
    track.append({"width": W, "height": H, "keypoints": k1.round(3).tolist(), "next": list(map(float, mn.crop_region(k1.round(3), H, W)))})
    assert np.abs(np.array(track[-1]["next"]) - np.array(r2)).max() < 1e-3
# A frame where the torso is not seen: the whole frame again.
low = np.array(track[0]["keypoints"]); low[[5, 6], 2] = 0.1
track.append({"width": 640, "height": 480, "keypoints": low.tolist(), "next": list(mn.crop_region(low, 480, 640))})

# A smooth made picture, so OpenCV's fixed-point weights stay within 1.
def made(W, H):
    y, x = np.mgrid[0:H, 0:W]
    return np.stack([x * 255 // (W - 1), y * 255 // (H - 1), (x + y) * 255 // (W + H - 2)], -1).astype(np.uint8)
crops = []
for (W, H, region) in [(640, 480, mn.full_region(480, 640)), (480, 640, mn.full_region(640, 480)),
                       (640, 480, tuple(track[0]["next"])), (640, 480, (-0.2, 0.7, 0.6, 1.3))]:
    crop = mn.crop_image(made(W, H), region, 192)
    idx = np.arange(0, 192 * 192, 101)
    crops.append({"width": W, "height": H, "region": list(map(float, region)), "size": 192,
                  "at": idx.tolist(), "rgb": crop.reshape(-1, 3)[idx].reshape(-1).tolist()})

os.makedirs(out, exist_ok=True)
for name, value in [("heads.json", heads), ("track.json", track), ("crop.json", crops)]:
    with open(os.path.join(out, name), "w") as fh:
        json.dump(value, fh, separators=(",", ":"))
    print(name, os.path.getsize(os.path.join(out, name)))
