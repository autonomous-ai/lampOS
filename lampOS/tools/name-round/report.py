"""The results table from work/res_*.json (and the shipped head, scored here)."""
import sys, json, glob
sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np
from evaluate import LiveSet, ValSet
from frontend import load_head
W = '/Users/autonomous/lamp-name-round/work/'
BARS = ['0.5', '0.7', '0.8', '0.9', '0.95', '0.98']
L = LiveSet()
voices = [v for _, _, _, v in L.plays]; ids = [i for _, _, i, _ in L.plays]
FOLD_A = {'alice', 'charlotte', 'george', 'chris'}

def shipped():
    p = W + 'res_shipped.json'
    try:
        return json.load(open(p))
    except FileNotFoundError:
        H = load_head('/Users/autonomous/lamp-voice-data/name-models/name.onnx')
        bars = tuple(float(b) for b in BARS)
        res = L.score(H, bars); V = ValSet(); vres = V.score(H, bars)
        out = dict(name='shipped', live={str(b): {k: dict(hits=v['hits'], false_h=v['false_h'], per=list(map(float, v['per']))) for k, v in r.items()} for b, r in res.items()},
                   val={str(b): v for b, v in vres.items()})
        json.dump(out, open(p, 'w'), indent=1); return out

R = {'shipped': shipped()}
for f in sorted(glob.glob(W + 'res_*.json')):
    n = f.split('res_')[1][:-5]
    if n != 'shipped': R[n] = json.load(open(f))
cfg = sys.argv[1] if len(sys.argv) > 1 else 'live80'
print(f'config {cfg}; live false calls per hour over {L.all_h * 60:.1f} min between calls (one false call = {1 / L.all_h:.1f}/h)')
print('%-14s' % 'head' + ''.join('%-16s' % ('bar ' + b) for b in BARS))
for n, r in R.items():
    print('%-14s' % n + ''.join('%-16s' % ('%4.1f  %5.1f/h' % (r['live'][b][cfg]['hits'], r['live'][b][cfg]['false_h'])) for b in BARS))
for pre in ['', 'C', 'E', 'F', 'G']:
    if pre + 'foldA' not in R or pre + 'foldB' not in R: continue
    row = []
    for b in BARS:
        pa = np.array(R[pre + 'foldA']['live'][b][cfg]['per']); pb = np.array(R[pre + 'foldB']['live'][b][cfg]['per'])
        held = sum(pa[i] if v in FOLD_A else pb[i] for i, v in enumerate(voices))
        fa = (R[pre + 'foldA']['live'][b][cfg]['false_h'] + R[pre + 'foldB']['live'][b][cfg]['false_h']) / 2
        row.append('%4.1f  %5.1f/h' % (held, fa))
    print('%-14s' % (pre + 'folds(held)') + ''.join('%-16s' % x for x in row))
print('\nper voice at bar 0.9 (%s):' % cfg)
vs = sorted(set(voices))
print('%-14s' % 'head' + ''.join('%-11s' % v for v in vs))
for n, r in R.items():
    per = np.array(r['live']['0.9'][cfg]['per'])
    print('%-14s' % n + ''.join('%-11s' % ('%.0f/%d' % (sum(per[i] for i in range(len(voices)) if voices[i] == v), voices.count(v))) for v in vs))
print('\nvalidation (rP val runs, 40 calls, 8 phases; false/h over %.2f h):' % 0.385)
for n, r in R.items():
    print('%-14s' % n + ''.join('%-16s' % ('%4.1f  %5.1f/h' % (r['val'][b]['hits'], r['val'][b]['false_h'])) for b in BARS))
