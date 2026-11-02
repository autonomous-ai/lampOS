"""Negatives from the cut-in runs' raw mic2 snippets (scenarios/cutin-*-mic2.wav:
the stop-word buffer, about the last 10 s of what mic2 heard while the lamp
told a story, with a played interruption). Snippets whose lines carry the
name are left out. Runs are split by label: even-numbered labels train
(work/set_z.npz), odd-numbered ones are the test (work/cutin_test.txt)."""
import sys, glob, re; sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np
from frontend import Front
from clean_data import readwav
W = '/Users/autonomous/lamp-name-round/work/'; D = '/Users/autonomous/lamp-voice-data/scenarios/'
fe = Front()
items = []
for p in sorted(glob.glob(D + 'cutin-*-mic2.wav')):
    m = re.match(r'.*/cutin-(.*)-(\d+)-mic2\.wav', p); lab = m.group(1); n = m.group(2)
    txt = glob.glob(D + f'*-cutin-{lab}-{n}.txt')
    if not txt: continue
    plays = [re.sub(r'^\s*[\d.]+s PLAY\s+\w+:\s*', '', l.strip()) for l in open(txt[0]) if ' PLAY ' in l and not l.startswith('#')]
    if not plays or any('lamp' in t.lower() for t in plays[1:]): continue
    items.append((lab, p))
labels = sorted({l for l, _ in items})
train_l = set(labels[0::2]); test = [p for l, p in items if l not in train_l]
open(W + 'cutin_test.txt', 'w').write('\n'.join(test) + '\n')
Es = []; win = []; n = 0
for l, p in items:
    if l not in train_l: continue
    x = readwav(p)
    if len(x) < 16000 * 3: continue
    lm = fe.logmel(x)
    for r in range(8):
        e = fe.embed_frames(fe.frames_for_residue(lm, r))
        if len(e) < 16: continue
        Es.append(e.astype(np.float16)); win.append(n + np.arange(len(e) - 15)); n += len(e)
win = np.concatenate(win)
np.savez(W + 'set_z.npz', EMB=np.concatenate(Es), win=win, lab=np.zeros(len(win), np.int8), bag=np.full(len(win), -1),
         grp=np.array(['cutin'] * len(win)), src=np.array(['cutin'] * len(win)))
print('labels', len(labels), 'train labels', len(train_l), 'train snippets', sum(1 for l, _ in items if l in train_l), 'test snippets', len(test), 'windows', len(win))
