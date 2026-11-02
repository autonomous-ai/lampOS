"""Build the training set: embeddings of room copies of clean speech, plus the
real V1 recordings and the live recording's even gaps.

Never in it: anything of the live recording (the 59 calls are the test, its
stretches between calls the false-call test), the rP calls, and the four rP
validation runs. The four rP training runs give their stretches between calls
(lamp-4ace's own answers and its room, on mic2) as negatives and room sound.

Writes work/set_<tag>.npz with
  EMB   (N, 96) float16 embeddings, each item's run 80 ms apart
  win   (M,)  index into EMB of a window's oldest embedding (16 in a row)
  lab   (M,)  1 positive-bag window, 0 negative
  bag   (M,)  bag id for positives (-1 for negatives)
  grp   (M,)  item group: the ElevenLabs voice, or 'say', 'v1', 'live', 'own', 'sayx'
  src   (M,)  item id
"""
import sys, glob, pickle, time
sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np
from scipy.signal import resample_poly
from frontend import Front
from augment import Room, augment, SR
from clean_data import eleven, generated, readwav

W = '/Users/autonomous/lamp-name-round/work/'
tag = sys.argv[1] if len(sys.argv) > 1 else 'a'
extra = sys.argv[2:]          # e.g. 'sayx' adds work/sayx/*.wav renders
rng = np.random.default_rng(7)
fe = Front()

class Store:
    def __init__(self):
        self.E = []; self.n = 0; self.win = []; self.lab = []; self.bag = []; self.grp = []; self.src = []; self.nbag = 0
    def add_seq(self, e, ends, labels, grp, src, bag_mask=None):
        """e: (K, 96) at 80 ms; ends: window end per embedding k; labels per k (1, 0, -1)."""
        K = len(e)
        if K < 16: return
        base = self.n; self.E.append(e.astype(np.float16)); self.n += K
        ks = np.arange(15, K)
        lb = labels[ks]
        keep = lb >= 0
        ks, lb = ks[keep], lb[keep]
        if (lb == 1).any():
            b = self.nbag; self.nbag += 1
        else:
            b = -1
        self.win.append(base + ks - 15); self.lab.append(lb)
        self.bag.append(np.where(lb == 1, b, -1)); self.grp += [grp] * len(ks); self.src += [src] * len(ks)
    def save(self, path):
        np.savez(path, EMB=np.concatenate(self.E), win=np.concatenate(self.win), lab=np.concatenate(self.lab).astype(np.int8),
                 bag=np.concatenate(self.bag), grp=np.array(self.grp), src=np.array(self.src))

def emb_of(y, r):
    lm = fe.logmel(y); fr = fe.frames_for_residue(lm, r); e = fe.embed_frames(fr)
    ends = 160 * (r + 8 * np.arange(len(e)) + 75) + 512      # window end sample of embedding k
    return e, ends

t0 = time.time()
# ---- pools: room sound from the live even gaps, the lamp's own voice ----
RP_TRAIN = ('rP-0017', 'rP-0141', 'rP-0355', 'rP-2221')     # the other four rP runs are validation
from real_data import rp_sessions
rp_train = [r for r in rp_sessions() if r['name'] in RP_TRAIN]
def rp_gaps(r):
    ps = sorted((p['s0'], p['s1']) for p in r['plays'])
    g = [(2.5 * SR, ps[0][0] - 1.0 * SR)] + [(ps[i][1] + 1.5 * SR, ps[i + 1][0] - 1.0 * SR) for i in range(len(ps) - 1)] + [(ps[-1][1] + 1.5 * SR, r['n'] if 'n' in r else len(r['audio']))]
    return [(int(a), int(b)) for a, b in g if b - a > 3 * SR]
noise_pool = [r['audio'][a:b].astype(np.float32) for r in rp_train for a, b in rp_gaps(r)]
own_pool = []
for f in glob.glob('/Users/autonomous/lamp-voice-data/sessions/*/native/runtime/render-audio/render_accepted.pcm16le') + \
         glob.glob('/Users/autonomous/lamp-voice-data/sessions/*/render-audio/render_accepted.pcm16le'):
    x = np.fromfile(f, '<i2').astype(np.float32)
    if len(x) < 24000 * 5: continue
    x = resample_poly(x, 2, 3).astype(np.float32)
    # keep the voiced part only
    n = len(x) // 1600 * 1600; r = np.sqrt(np.mean(x[:n].reshape(-1, 1600) ** 2, 1))
    on = np.repeat(r > 30, 1600)
    if on.sum() > 16000 * 3: own_pool.append(x[:n][on])
print('pools: room', len(noise_pool), sum(map(len, noise_pool)) / SR, 's; own voice', len(own_pool), sum(map(len, own_pool)) / SR, 's')
room = Room(11, noise_pool, own_pool)
S = Store()

def clean_items(items, n_pos, n_neg, grp_of):
    for it in items:
        if it['mention']: continue
        n = n_pos if it['call'] else n_neg
        for j in range(n):
            y, off = augment(room, it['audio'], it['onset'] if it['call'] else None)
            e, ends = emb_of(y, int(rng.integers(8)))
            t = (ends - off) / SR                  # window end, s from the clip's start
            if it['call']:
                lo, hi = (0.55, 1.65) if it['hey'] else (0.35, 1.45)
                o = it['onset']
                lab = np.full(len(e), -1); lab[(t >= o + lo) & (t <= o + hi)] = 1
                lab[(t < o + 0.1)] = 0; lab[t > o + 2.9] = 0
            else:
                lab = np.zeros(len(e), int)
            S.add_seq(e, ends, lab, grp_of(it), it['src'] + ':' + it['id'])

ONLY = 'only' in extra
if not ONLY:
    el = eleven()
    clean_items(el, 16, 8, lambda it: it['voice'])
    print('eleven done', time.time() - t0, 'windows', sum(map(len, S.lab)), flush=True)
    clean_items(generated(), 4, 3, lambda it: 'say')
    print('say done', time.time() - t0, 'windows', sum(map(len, S.lab)), flush=True)
for sub, npos, nneg in (('sayx', 3, 2), ('sayy', 3, 2)):
    if sub not in extra: continue
    from clean_data import onset, hey
    from real_data import is_call, mentions
    items = []
    for l in open(W + sub + '/index.tsv'):
        f = l.rstrip('\n').split('\t')
        if len(f) < 3: continue
        try:
            x = readwav(W + sub + '/' + f[0] + '.wav')
        except Exception:
            continue
        items.append(dict(src=sub, id=f[0], voice=f[1], text=f[2], audio=x, call=is_call(f[2]),
                          mention=mentions(f[2]) and not is_call(f[2]), onset=onset(x), hey=hey(f[2])))
    clean_items(items, npos, nneg, lambda it, sub=sub: sub)
    print(sub, 'done', time.time() - t0, 'windows', sum(map(len, S.lab)), flush=True)
if not ONLY:
    # the lamp's own voice as negatives, through the room
    for x in own_pool:
        for s in range(0, len(x) - SR * 6, SR * 8):
            y, off = augment(room, x[s: s + SR * 8])
            e, ends = emb_of(y, int(rng.integers(8)))
            S.add_seq(e, ends, np.zeros(len(e), int), 'own', 'own')
    print('own done', time.time() - t0, 'windows', sum(map(len, S.lab)))
    # ---- real: V1 recordings (calls as bags, the rest negatives) ----
    for s in pickle.load(open(W + 'v1_E.pkl', 'rb')):
        E = s['E'].astype(np.float32); F = len(E)
        for r in range(8):
            e = E[r::8]; ok = ~np.isnan(e[:, 0]); e = e[ok]
            if len(e) < 16: continue
            ends = 160 * (r + 8 * np.arange(len(E[r::8]))[ok] + 75) + 512
            lab = np.zeros(len(e), int)
            for p in s['plays']:
                if not (0 <= p['s0'] < s['n']): continue
                if p['call']:
                    lab[(ends >= p['s0'] - 0.3 * SR) & (ends <= p['s1'] + 2.5 * SR)] = -1
                    lab[(ends >= p['s0'] + 0.35 * SR) & (ends <= p['s0'] + 1.9 * SR)] = 1
                elif p['mention']:
                    lab[(ends >= p['s0'] - 0.3 * SR) & (ends <= p['s1'] + 2.5 * SR)] = -1
            if r % 4 != 0:
                lab[lab == 0] = -1            # negatives from two residues are enough
            # one bag per call: split the sequence at each call so bags stay apart
            for p in s['plays']:
                pass
            cut = [0]
            for p in s['plays']:
                if p['call'] and 0 <= p['s0'] < s['n']:
                    k = int(np.searchsorted(ends, p['s0'] - 0.3 * SR)); cut.append(k)
            cut.append(len(e)); cut = sorted(set(cut))
            for a, b in zip(cut[:-1], cut[1:]):
                a0 = max(0, a - 15)
                l = lab[a0:b].copy(); l[: a - a0] = -1
                S.add_seq(e[a0:b], ends[a0:b], l, 'v1', 'v1:' + s['name'])
    print('v1 done', time.time() - t0, 'windows', sum(map(len, S.lab)))
    # ---- real: the rP training runs' gaps (negatives: lamp-4ace's own answers and room on mic2) ----
    RPE = {r['name']: r for r in pickle.load(open(W + 'rp_E.pkl', 'rb'))}
    for r in rp_train:
        E = RPE[r['name']]['E'].astype(np.float32)
        for a, b in rp_gaps(r):
            for ph in range(8):
                e = E[ph::8]; ends = 160 * (ph + 8 * np.arange(len(e)) + 75) + 512
                m = (ends - 32480 >= a) & (ends <= b) & ~np.isnan(e[:, 0])
                if m.sum() < 16: continue
                idx = np.where(m)[0]
                S.add_seq(e[idx[0]: idx[-1] + 1], ends[idx[0]: idx[-1] + 1], np.zeros(idx[-1] + 1 - idx[0], int), 'rpgap', 'rp:' + r['name'])
    print('rP gaps done', time.time() - t0)

S.save(W + f'set_{tag}.npz')
lab = np.concatenate(S.lab)
print('saved', W + f'set_{tag}.npz', 'windows', len(lab), 'pos windows', (lab == 1).sum(), 'bags', S.nbag, 'embeddings', S.n)
