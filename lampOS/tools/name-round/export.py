"""Write a trained head as ONNX: the shipped name.onnx with its six weight
tensors' raw bytes replaced in place (same names, shapes, graph and opset),
so lamp-name loads it as it loads name.onnx."""
import sys; sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np, hashlib
from onnxmini import _varint, Model

SHIPPED = '/Users/autonomous/lamp-voice-data/name-models/name.onnx'

def _spans(b, start, end):
    """(field, wire, value_start, value_end) of each field in b[start:end]."""
    i = start; out = []
    while i < end:
        key, i = _varint(b, i); f, w = key >> 3, key & 7
        if w == 0:
            _, j = _varint(b, i); out.append((f, w, i, j)); i = j
        elif w == 2:
            l, i = _varint(b, i); out.append((f, w, i, i + l)); i += l
        elif w == 1: out.append((f, w, i, i + 8)); i += 8
        elif w == 5: out.append((f, w, i, i + 4)); i += 4
    return out

def export(P, path):
    b = bytearray(open(SHIPPED, 'rb').read())
    names = ['w0', 'b0', 'w1', 'b1', 'w2', 'b2']
    want = {n: np.ascontiguousarray(p, dtype='<f4').tobytes() for n, p in zip(names, P)}
    done = set()
    for f, w, s, e in _spans(b, 0, len(b)):
        if f != 7: continue
        for f2, w2, s2, e2 in _spans(b, s, e):
            if f2 != 5: continue
            fields = _spans(b, s2, e2)
            name = next(bytes(b[a:z]).decode() for ff, ww, a, z in fields if ff == 8)
            raw = [(a, z) for ff, ww, a, z in fields if ff == 9]
            assert raw, f'{name}: no raw_data'
            a, z = raw[0]
            assert z - a == len(want[name]), (name, z - a, len(want[name]))
            b[a:z] = want[name]; done.add(name)
    assert done == set(names), done
    open(path, 'wb').write(bytes(b))
    m = Model(path)
    for n, p in zip(names, P):
        assert np.array_equal(m.init[n].reshape(p.shape), p.astype(np.float32)), n
    x = np.random.default_rng(0).normal(0, 1, (1, 16, 96)).astype(np.float32)
    from train import forward
    z = forward([p.astype(np.float32) for p in P], x.reshape(1, -1))[0]
    got = m.run(x)[0].reshape(-1)[0]
    assert abs(got - 1 / (1 + np.exp(-z[0]))) < 1e-5
    return hashlib.sha256(bytes(b)).hexdigest()

if __name__ == '__main__':
    from frontend import load_head
    P = load_head(SHIPPED)
    print(export(P, '/Users/autonomous/lamp-name-round/work/roundtrip.onnx'))
    print(hashlib.sha256(open(SHIPPED, 'rb').read()).hexdigest())
