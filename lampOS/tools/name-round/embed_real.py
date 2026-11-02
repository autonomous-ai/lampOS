"""Embeddings (every 10 ms position) of the real recordings: rP (validation) and V1 (training)."""
import sys, time; sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np, pickle
from frontend import Front
from real_data import rp_sessions, v1_sessions
fe = Front(); W = '/Users/autonomous/lamp-name-round/work/'
t = time.time()
out = []
for s in rp_sessions():
    E = fe.embeddings_all(s['audio'].astype(np.float32)).astype(np.float16)
    out.append(dict(name=s['name'], E=E, plays=s['plays'], n=len(s['audio'])))
pickle.dump(out, open(W + 'rp_E.pkl', 'wb'))
print('rP done', time.time() - t)
out = []
for s in v1_sessions():
    E = fe.embeddings_all(s['audio'].astype(np.float32)).astype(np.float16)
    out.append(dict(name=s['name'], E=E, plays=s['plays'], n=len(s['audio'])))
pickle.dump(out, open(W + 'v1_E.pkl', 'wb'))
print('V1 done', time.time() - t)
