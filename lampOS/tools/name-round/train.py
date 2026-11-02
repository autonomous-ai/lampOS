"""Train a name head (the shipped shape: 16x96 -> 96 -> 48 -> 1, ReLU, sigmoid)
on frozen embeddings, in NumPy. Positives are bags (one per call copy: the
windows ending from about 0.35 s to 1.45 s after the call's onset); each pass
takes each bag's best window as its positive (multiple-instance learning).
"""
import sys, time
sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np
from frontend import load_head

W = '/Users/autonomous/lamp-name-round/work/'
OFF = np.arange(16)

class Data:
    def __init__(self, paths, drop_groups=()):
        if isinstance(paths, str): paths = [paths]
        Es = []; win = []; lab = []; bag = []; grp = []; base = 0; bbase = 0
        for path in paths:
            d = np.load(path)
            keep = ~np.isin(d['grp'], list(drop_groups))
            Es.append(d['EMB']); win.append(d['win'][keep] + base); lab.append(d['lab'][keep])
            b = d['bag'][keep]; bag.append(np.where(b >= 0, b + bbase, -1)); grp.append(d['grp'][keep])
            base += len(d['EMB']); bbase += int(d['bag'].max()) + 1
        self.E = np.concatenate(Es); self.win = np.concatenate(win); self.lab = np.concatenate(lab)
        self.bag = np.concatenate(bag); self.grp = np.concatenate(grp)
        self.pos = np.where(self.lab == 1)[0]; self.neg = np.where(self.lab == 0)[0]
        # bags -> their window rows
        b = self.bag[self.pos]; order = np.argsort(b, kind='stable')
        self.pos = self.pos[order]; b = b[order]
        self.bag_ids, self.bag_start = np.unique(b, return_index=True)
        self.bag_end = np.append(self.bag_start[1:], len(b))

    def X(self, rows):
        return self.E[self.win[rows][:, None] + OFF[None, :]].reshape(len(rows), -1).astype(np.float32)

def init(seed, from_shipped=False, hidden=(96, 48)):
    if from_shipped:
        w0, b0, w1, b1, w2, b2 = [a.astype(np.float32).copy() for a in load_head('/Users/autonomous/lamp-voice-data/name-models/name.onnx')]
        return [w0, b0, w1, b1, w2, b2]
    r = np.random.default_rng(seed)
    h1, h2 = hidden
    return [(r.normal(0, np.sqrt(2 / 1536), (1536, h1))).astype(np.float32), np.zeros(h1, np.float32),
            (r.normal(0, np.sqrt(2 / h1), (h1, h2))).astype(np.float32), np.zeros(h2, np.float32),
            (r.normal(0, np.sqrt(1 / h2), (h2, 1))).astype(np.float32), np.zeros(1, np.float32)]

def forward(P, X):
    w0, b0, w1, b1, w2, b2 = P
    a1 = X @ w0 + b0; h1 = np.maximum(a1, 0)
    a2 = h1 @ w1 + b1; h2 = np.maximum(a2, 0)
    z = (h2 @ w2 + b2)[:, 0]
    return z, (X, a1, h1, a2, h2)

def backward(P, cache, dz):
    w0, b0, w1, b1, w2, b2 = P
    X, a1, h1, a2, h2 = cache
    dz = dz[:, None]
    gw2 = h2.T @ dz; gb2 = dz.sum(0)
    dh2 = dz @ w2.T; da2 = dh2 * (a2 > 0)
    gw1 = h1.T @ da2; gb1 = da2.sum(0)
    dh1 = da2 @ w1.T; da1 = dh1 * (h1 > 0)
    gw0 = X.T @ da1; gb0 = da1.sum(0)
    return [gw0, gb0, gw1, gb1, gw2, gb2]

def logits(P, D, rows, bs=8192):
    out = np.empty(len(rows), np.float32)
    for i in range(0, len(rows), bs):
        out[i:i + bs] = forward(P, D.X(rows[i:i + bs]))[0]
    return out

def train(D, epochs=12, lr=1e-3, wd=1e-4, pos_weight=1.0, seed=0, from_shipped=False, hard_neg=2.0,
          batch=512, val=None, log=print, noise=0.0, top=1):
    rng = np.random.default_rng(seed)
    P = init(seed, from_shipped)
    m = [np.zeros_like(p) for p in P]; v = [np.zeros_like(p) for p in P]; t = 0
    best = None; hist = []
    for ep in range(epochs):
        t0 = time.time()
        # MIL: each bag's best window(s) under the current head
        zp = logits(P, D, D.pos)
        chosen = []
        for s, e in zip(D.bag_start, D.bag_end):
            k = np.argsort(-zp[s:e])[:top] + s
            chosen.append(D.pos[k])
        chosen = np.concatenate(chosen)
        # hard negatives: weight negatives by how high they score now
        negs = D.neg
        if hard_neg > 0 and ep > 0:
            zn = logits(P, D, negs)
            pn = 1 / (1 + np.exp(-zn))
            wneg = (1.0 + hard_neg * pn).astype(np.float32)
        else:
            wneg = np.ones(len(negs), np.float32)
        rows = np.concatenate([chosen, negs]); y = np.concatenate([np.ones(len(chosen)), np.zeros(len(negs))]).astype(np.float32)
        # balance: positives in all carry pos_weight x the negatives' total weight
        w = np.concatenate([np.full(len(chosen), pos_weight * wneg.sum() / len(chosen)), wneg]).astype(np.float32)
        w /= w.mean()
        perm = rng.permutation(len(rows))
        tot = 0.0
        for i in range(0, len(perm), batch):
            idx = perm[i:i + batch]
            X = D.X(rows[idx])
            if noise > 0:
                X = X + rng.normal(0, noise, X.shape).astype(np.float32)
            z, cache = forward(P, X)
            p = 1 / (1 + np.exp(-z))
            ww = w[idx]
            tot += float(np.sum(ww * (np.logaddexp(0, z) - y[idx] * z)))
            dz = ww * (p - y[idx]) / len(idx)
            G = backward(P, cache, dz)
            t += 1
            for j in range(len(P)):
                g = G[j] + (wd * P[j] if P[j].ndim == 2 else 0)
                m[j] = 0.9 * m[j] + 0.1 * g; v[j] = 0.999 * v[j] + 0.001 * g * g
                P[j] -= lr * (m[j] / (1 - 0.9 ** t)) / (np.sqrt(v[j] / (1 - 0.999 ** t)) + 1e-8)
        msg = f'epoch {ep} loss {tot / len(rows):.4f} {time.time() - t0:.0f}s'
        if val is not None:
            r = val(P)
            msg += ' val ' + ' '.join(f"{b}:{x['hits']:.1f}/{x['of']} fa{x['false_h']:.1f}" for b, x in r.items())
            hist.append((ep, r, [p.copy() for p in P]))
        log(msg)
    return P, hist
