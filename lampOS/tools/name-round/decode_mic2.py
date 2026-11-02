"""Decode the bus recording of mic2/audio into one int16 array plus timing.

Each line's payload is a JSON header line, a newline, then S16LE samples.
Writes work/mic2.npy (int16) and work/mic2_t.npy: for each 20 ms block,
(first sample index, at_unix_ms, seq).
"""
import base64, json, sys
import numpy as np
src = sys.argv[1] if len(sys.argv) > 1 else '/Users/autonomous/lamp-tools/namerec/mic2_audio.jsonl'
out = '/Users/autonomous/lamp-name-round/work/'
blocks = []; meta = []; n = 0; seqs = []
for line in open(src):
    d = json.loads(line)
    if 'payload_b64' not in d:
        continue
    raw = base64.b64decode(d['payload_b64'])
    k = raw.index(b'\n')
    head = json.loads(raw[:k]); pcm = np.frombuffer(raw[k+1:], dtype='<i2')
    assert head['sample_rate'] == 16000 and head['channels'] == 1
    meta.append((n, d['header']['at_unix_ms'], d['header']['seq'], head.get('sequence', -1), len(pcm)))
    blocks.append(pcm); n += len(pcm)
a = np.concatenate(blocks)
m = np.array(meta, dtype=np.int64)
np.save(out + 'mic2.npy', a); np.save(out + 'mic2_t.npy', m)
print('samples', len(a), 'seconds', len(a) / 16000, 'blocks', len(m))
print('block sizes', np.unique(m[:, 4], return_counts=True))
ds = np.diff(m[:, 3]); print('sequence steps', np.unique(ds, return_counts=True))
dt = np.diff(m[:, 1]); print('at_unix_ms steps', np.percentile(dt, [0, 1, 50, 99, 100]))
# drift: wall time vs sample time
print('wall span ms', m[-1, 1] - m[0, 1], 'sample span ms', (m[-1, 0] - m[0, 0]) / 16)
print('rms dBFS', 20*np.log10(np.sqrt(np.mean(a.astype(float)**2))/32768))
