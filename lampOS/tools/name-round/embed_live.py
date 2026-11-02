"""Embeddings of the whole live mic2 recording at every 10 ms position."""
import sys, time; sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np
from frontend import Front
fe = Front()
a = np.load('/Users/autonomous/lamp-name-round/work/mic2.npy')
t = time.time()
E = fe.embeddings_all(a.astype(np.float32))
np.save('/Users/autonomous/lamp-name-round/work/live_E.npy', E)
print(E.shape, 'nan rows', np.isnan(E[:, 0]).sum(), 'secs', time.time() - t)
