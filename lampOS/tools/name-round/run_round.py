"""Train the candidates, pick each one's epoch on the rP validation runs, and
score them on the live test. Usage: run_round.py SET_TAG NAME [options]
  --drop G1,G2   leave these groups (ElevenLabs voices, 'say', 'sayx', 'v1', ...) out
  --ft           start from the shipped head's weights
  --epochs N --lr X --pos W --hard H --seed S --noise SD
"""
import sys, json, time, argparse
sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np
from train import Data, train
from evaluate import LiveSet, ValSet
from export import export

W = '/Users/autonomous/lamp-name-round/work/'
ap = argparse.ArgumentParser()
ap.add_argument('set'); ap.add_argument('name')
ap.add_argument('--drop', default=''); ap.add_argument('--ft', action='store_true')
ap.add_argument('--epochs', type=int, default=10); ap.add_argument('--lr', type=float, default=1e-3)
ap.add_argument('--pos', type=float, default=1.0); ap.add_argument('--hard', type=float, default=2.0)
ap.add_argument('--seed', type=int, default=0); ap.add_argument('--noise', type=float, default=0.0)
ap.add_argument('--wd', type=float, default=1e-4)
a = ap.parse_args()
drop = [g for g in a.drop.split(',') if g]
D = Data([W + f'set_{t}.npz' for t in a.set.split('+')], drop)
print(a.name, 'windows', len(D.lab), 'bags', len(D.bag_ids), 'neg', len(D.neg), 'dropped', drop, flush=True)
V = ValSet()
val = lambda P: V.score([p for p in P], (0.9,))
P, hist = train(D, epochs=a.epochs, lr=a.lr, wd=a.wd, pos_weight=a.pos, seed=a.seed, from_shipped=a.ft,
                hard_neg=a.hard, val=val, noise=a.noise, log=lambda m: print(m, flush=True))
# the epoch with the best validation: calls heard at 0.9 less false calls (both per run of 10 calls / 0.1 h)
def util(r):
    x = r[0.9]; return x['hits'] - x['false_h'] * V.hours
ep, r, Pb = max(hist, key=lambda h: util(h[1]))
print('chosen epoch', ep, r, flush=True)
sha = export(Pb, W + f'cand_{a.name}.onnx')
L = LiveSet()
bars = (0.5, 0.7, 0.8, 0.9, 0.95, 0.98)
res = L.score(Pb, bars)
vres = V.score(Pb, bars)
out = dict(name=a.name, args=vars(a), epoch=ep, sha256=sha,
           live={str(b): {k: dict(hits=v['hits'], false_h=v['false_h'], per=list(map(float, v['per']))) for k, v in r.items()} for b, r in res.items()},
           val={str(b): v for b, v in vres.items()})
json.dump(out, open(W + f'res_{a.name}.json', 'w'), indent=1)
for b in bars:
    x = res[b]; v = vres[b]
    print(f"bar {b}: live80 {x['live80']['hits']:.0f}/59 fa/h {x['live80']['false_h']:.1f} | avg80 {x['avg80']['hits']:.1f} fa/h {x['avg80']['false_h']:.1f} | step10 {x['step10']['hits']:.0f} fa/h {x['step10']['false_h']:.1f} || val {v['hits']:.1f}/{v['of']} fa/h {v['false_h']:.1f}")
