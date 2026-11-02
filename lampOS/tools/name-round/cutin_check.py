"""False calls on the lamp's own voice, raw on mic2: the test half of the cut-in
runs' snippets (work/cutin_test.txt from build_cutin.py: the stop-word buffer,
about 10 s of what mic2 heard while the lamp told a story, with a played
interruption that has no name). Any hearing in them is false. Eight phases,
averaged; per hour of snippet audio. Usage: cutin_check.py [cand names...]"""
import sys, os, pickle; sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np
from frontend import Front, head_scores, load_head, MODELS
from clean_data import readwav
from scorer import hearings
W = '/Users/autonomous/lamp-name-round/work/'
CACHE = W + 'cutin_test_E.pkl'
if os.path.exists(CACHE):
    Es = pickle.load(open(CACHE, 'rb'))
else:
    fe = Front(); Es = []
    for p in open(W + 'cutin_test.txt').read().split():
        x = readwav(p)
        if len(x) < 16000 * 3: continue
        Es.append((len(x), fe.embeddings_all(x).astype(np.float16)))
    pickle.dump(Es, open(CACHE, 'wb'))
secs = sum(n for n, _ in Es) / 16000
def score(H, bars=(0.7, 0.8, 0.9, 0.95)):
    out = {}
    Ss = [np.nan_to_num(head_scores(E.astype(np.float32), *H)) for _, E in Es]
    for bar in bars:
        n = 0
        for S in Ss:
            for ph in range(8):
                st = S[120 + ph::8]
                n += len(hearings(st, 80.0 * np.arange(len(st)), bar))
        out[bar] = n / 8 / (secs / 3600)
    return out
if __name__ == '__main__':
    heads = {'shipped': MODELS + 'name.onnx'}
    for n in sys.argv[1:]: heads[n] = W + f'cand_{n}.onnx'
    print(f'{len(Es)} snippets, {secs / 60:.1f} min of the lamp speaking (raw mic2); false calls per hour at bars 0.7 / 0.8 / 0.9 / 0.95:')
    for k, p in heads.items():
        r = score(load_head(p)); print(f'  {k}: ' + ' / '.join(f'{v:.0f}' for v in r.values()), flush=True)
