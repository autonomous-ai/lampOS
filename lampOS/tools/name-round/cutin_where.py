"""Where in the no-name cut-in snippets candidate heads fire, by snippet label."""
import sys, glob, re; sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np, collections
from frontend import Front, head_scores, load_head, MODELS
from clean_data import readwav
from real_data import is_call
fe = Front(); D = '/Users/autonomous/lamp-voice-data/scenarios/'
H = {k: load_head(MODELS + 'name.onnx' if k == 'shipped' else f'work/cand_{k}.onnx') for k in sys.argv[1:]}
rows = []
for p in glob.glob(D + 'cutin-*-mic2.wav'):
    m = re.match(r'.*/cutin-(.*)-(\d+)-mic2\.wav', p); lab, n = m.group(1), m.group(2)
    txt = glob.glob(D + f'*-cutin-{lab}-{n}.txt')
    if not txt: continue
    plays = [re.sub(r'^\s*[\d.]+s PLAY\s+', '', l.strip()) for l in open(txt[0]) if ' PLAY ' in l and not l.startswith('#')]
    if len(plays) < 2 or any('lamp' in t.lower() for t in plays[1:]): continue
    x = readwav(p)
    if len(x) < 16000 * 4: continue
    y = np.concatenate([np.zeros(32480, np.float32), x]); lm = fe.logmel(y)
    fr = fe.frames_for_residue(lm, 0); e = fe.embed_frames(fr)
    E = np.full((len(e) * 8, 96), np.nan, np.float32); E[8 * np.arange(len(e))] = e
    for k, h in H.items():
        S = np.nan_to_num(head_scores(E, *h)[120::8]); t = (32480 + 1280 * np.arange(len(S)) - 32480) / 16000
        if (S >= 0.9).any():
            rows.append((k, '-'.join(lab.split('-')[:2]), round(len(x) / 16000, 1), float(t[np.argmax(S >= 0.9)]), plays[0][:50], plays[1][:40]))
c = collections.Counter((r[0], r[1]) for r in rows)
for k in H:
    print(k, sorted([(lab, n) for (kk, lab), n in c.items() if kk == k], key=lambda z: -z[1])[:12])
for r in rows[:40]:
    if r[0] == sys.argv[-1]: print(r)
