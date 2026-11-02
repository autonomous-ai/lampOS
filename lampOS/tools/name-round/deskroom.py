"""Desk lines through simulated mic2 rooms: how often a head fires on them.
16 room copies of each ElevenLabs line without the name (desk d01-d12, follow-ups,
conversation), d09 ("That lamp is cute") apart. For heads trained on these lines'
voices this is not a test; the fold heads' held-out voices are.
Usage: deskroom.py name=head.onnx[:dropped,voices] ..."""
import sys; sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np, json
from frontend import Front, head_scores, load_head
from augment import Room, augment
from clean_data import eleven
from real_data import rp_sessions
W = '/Users/autonomous/lamp-name-round/work/'
fe = Front()
pool = []
for r in rp_sessions():
    if r['name'] not in ('rP-0017', 'rP-0141', 'rP-0355', 'rP-2221'): continue
    ps = sorted((p['s0'], p['s1']) for p in r['plays'])
    for i in range(len(ps) - 1):
        a, b = int(ps[i][1] + 24000), int(ps[i + 1][0] - 16000)
        if b - a > 48000: pool.append(r['audio'][a:b].astype(np.float32))
room = Room(999, pool, [])
import os, pickle
CACHE = W + 'desk_cache.pkl'
if os.path.exists(CACHE):
    copies = pickle.load(open(CACHE, 'rb'))
else:
    lines = [d for d in eleven() if not d['call']]
    copies = {}
    for d in lines:
        Es = []
        for j in range(16):
            y, off = augment(room, d['audio'])
            e = fe.embed_frames(fe.frames_for_residue(fe.logmel(y), 0))
            E = np.full((len(e) * 8, 96), np.nan, np.float32); E[8 * np.arange(len(e))] = e
            Es.append(E.astype(np.float16))
        copies[d['id']] = (dict(voice=d['voice'], text=d['text']), Es)
    pickle.dump(copies, open(CACHE, 'wb'))
heads = []
for arg in sys.argv[1:]:
    name, rest = arg.split('=', 1)
    path, _, held = rest.partition(':')
    heads.append((name, load_head(path), set(held.split(',')) if held else None))
out = {}
for name, H, held in heads:
    for bar in (0.5, 0.7, 0.9):
        fired = []; d05 = []; d09 = []
        for cid, (d, Es) in copies.items():
            if held is not None and d['voice'] not in held: continue
            f = []
            for E in Es:
                S = head_scores(E.astype(np.float32), *H)
                f.append(np.nanmax(S[120::8]) >= bar)     # one phase per copy is enough: copies differ
            if cid == 'd09': d09.append(np.mean(f)); continue
            fired.append(np.mean(f))
            if cid == 'd05': d05.append(np.mean(f))
        out[(name, bar)] = (np.mean(fired), len(fired), d05[0] if d05 else None, d09[0] if d09 else None)
        print(f"{name} bar {bar}: room copies firing {100*np.mean(fired):.1f}% of {len(fired)} lines x16;"
              f" d05 {'-' if not d05 else '%.0f%%' % (100*d05[0])}; d09 'That lamp is cute' {'-' if not d09 else '%.0f%%' % (100*d09[0])}", flush=True)
