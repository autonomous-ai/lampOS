"""A minimal ONNX reader and NumPy interpreter for the three lamp-name models.

No onnx/onnxruntime on this Mac: this decodes the protobuf by hand and runs
the handful of ops the openWakeWord front end and the heads use.
"""
import struct
import numpy as np

# ---------- protobuf wire format ----------
def _varint(b, i):
    r = 0; s = 0
    while True:
        c = b[i]; i += 1
        r |= (c & 0x7F) << s
        if c < 0x80:
            return r, i
        s += 7

def fields(b):
    """Yield (field_number, wire_type, value) for a message's bytes."""
    i = 0; n = len(b)
    while i < n:
        key, i = _varint(b, i)
        f, w = key >> 3, key & 7
        if w == 0:
            v, i = _varint(b, i)
        elif w == 1:
            v = b[i:i+8]; i += 8
        elif w == 2:
            l, i = _varint(b, i); v = b[i:i+l]; i += l
        elif w == 5:
            v = b[i:i+4]; i += 4
        else:
            raise ValueError(f"wire type {w}")
        yield f, w, v

def _packed_varints(v):
    out = []; i = 0
    while i < len(v):
        x, i = _varint(v, i); out.append(x)
    return out

def _signed(x):
    return x - (1 << 64) if x >= (1 << 63) else x

DT = {1: np.float32, 2: np.uint8, 3: np.int8, 5: np.int16, 6: np.int32, 7: np.int64, 9: np.bool_, 10: np.float16, 11: np.float64}

def tensor(b):
    dims = []; dt = 1; raw = None; fl = []; i64 = []; i32 = []; name = ''; dbl = []
    for f, w, v in fields(b):
        if f == 1:
            if w == 2: dims += [_signed(x) for x in _packed_varints(v)]
            else: dims.append(_signed(v))
        elif f == 2: dt = v
        elif f == 4:
            if w == 2: fl += list(struct.unpack(f'<{len(v)//4}f', v))
            else: fl.append(struct.unpack('<f', v)[0])
        elif f == 5:
            if w == 2: i32 += [_signed(x) for x in _packed_varints(v)]
            else: i32.append(_signed(v))
        elif f == 7:
            if w == 2: i64 += [_signed(x) for x in _packed_varints(v)]
            else: i64.append(_signed(v))
        elif f == 8: name = v.decode()
        elif f == 9: raw = bytes(v)
        elif f == 10:
            if w == 2: dbl += list(struct.unpack(f'<{len(v)//8}d', v))
    t = DT[dt]
    if raw is not None:
        a = np.frombuffer(raw, dtype=t).copy()
    elif fl: a = np.array(fl, dtype=t)
    elif i64: a = np.array(i64, dtype=t)
    elif i32: a = np.array(i32, dtype=t)
    elif dbl: a = np.array(dbl, dtype=t)
    else: a = np.zeros(0, dtype=t)
    return name, a.reshape(dims) if dims else (a.reshape(()) if a.size == 1 else a)

def attribute(b):
    name = None; val = None; ints = []; floats = []; typ = None
    for f, w, v in fields(b):
        if f == 1: name = v.decode()
        elif f == 2: val = struct.unpack('<f', v)[0]
        elif f == 3: val = _signed(v)
        elif f == 4: val = bytes(v).decode(errors='replace')
        elif f == 5: val = tensor(v)[1]
        elif f == 7:
            if w == 2: floats += list(struct.unpack(f'<{len(v)//4}f', v))
            else: floats.append(struct.unpack('<f', v)[0])
        elif f == 8:
            if w == 2: ints += [_signed(x) for x in _packed_varints(v)]
            else: ints.append(_signed(v))
        elif f == 20: typ = v
    if typ == 7 or (val is None and ints): val = ints
    if typ == 6 or (val is None and floats): val = floats
    return name, val

def node(b):
    ins = []; outs = []; op = None; attrs = {}; name = ''
    for f, w, v in fields(b):
        if f == 1: ins.append(v.decode())
        elif f == 2: outs.append(v.decode())
        elif f == 3: name = v.decode()
        elif f == 4: op = v.decode()
        elif f == 5:
            k, a = attribute(v); attrs[k] = a
    return dict(op=op, ins=ins, outs=outs, attrs=attrs, name=name)

def value_info_name(b):
    for f, w, v in fields(b):
        if f == 1: return v.decode()

class Model:
    def __init__(self, path):
        data = open(path, 'rb').read()
        graph = None; self.opset = None
        for f, w, v in fields(data):
            if f == 7: graph = v
            if f == 8:
                for f2, w2, v2 in fields(v):
                    if f2 == 2: self.opset = v2
        self.nodes = []; self.init = {}; self.inputs = []; self.outputs = []
        for f, w, v in fields(graph):
            if f == 1: self.nodes.append(node(v))
            elif f == 5:
                k, a = tensor(v); self.init[k] = a
            elif f == 11: self.inputs.append(value_info_name(v))
            elif f == 12: self.outputs.append(value_info_name(v))
        self.inputs = [i for i in self.inputs if i not in self.init]

    def ops(self):
        from collections import Counter
        return Counter(n['op'] for n in self.nodes)

    def run(self, *xs):
        env = dict(self.init)
        for k, x in zip(self.inputs, xs):
            env[k] = x
        for n in self.nodes:
            args = [env[i] if i else None for i in n['ins']]
            res = OPS[n['op']](n['attrs'], *args)
            if not isinstance(res, tuple): res = (res,)
            for k, r in zip(n['outs'], res):
                env[k] = r
        return [env[o] for o in self.outputs]

# ---------- ops ----------
def _conv(a, x, w, b=None):
    # x: N,C,H,W (2D) or N,C,L (1D); w: M,C/g,kh,kw
    group = a.get('group', 1)
    nd = x.ndim - 2
    if nd == 1:
        x = x[:, :, None, :]; w = w[:, :, None, :]
        strides = [1] + list(a.get('strides', [1])); dil = [1] + list(a.get('dilations', [1]))
        p = a.get('pads', [0, 0]); pads = [0, p[0], 0, p[1]]
    else:
        strides = list(a.get('strides', [1, 1])); dil = list(a.get('dilations', [1, 1]))
        p = a.get('pads', [0, 0, 0, 0]); pads = [p[0], p[1], p[2], p[3]]
    ap = a.get('auto_pad', 'NOTSET')
    N, C, H, W = x.shape; M, Cg, kh, kw = w.shape
    if ap in ('SAME_UPPER', 'SAME_LOWER'):
        res = []
        for d, (sz, k) in enumerate(((H, kh), (W, kw))):
            out = -(-sz // strides[d]); need = max(0, (out - 1) * strides[d] + (k - 1) * dil[d] + 1 - sz)
            lo = need // 2 if ap == 'SAME_UPPER' else need - need // 2
            res.append((lo, need - lo))
        pads = [res[0][0], res[1][0], res[0][1], res[1][1]]
    x = np.pad(x, ((0, 0), (0, 0), (pads[0], pads[2]), (pads[1], pads[3])))
    H2, W2 = x.shape[2], x.shape[3]
    oh = (H2 - (kh - 1) * dil[0] - 1) // strides[0] + 1
    ow = (W2 - (kw - 1) * dil[1] - 1) // strides[1] + 1
    out = np.zeros((N, M, oh, ow), dtype=np.float32)
    mg = M // group
    for g in range(group):
        xs = x[:, g*Cg:(g+1)*Cg]
        ws = w[g*mg:(g+1)*mg]
        # im2col
        cols = np.empty((N, Cg, kh, kw, oh, ow), dtype=np.float32)
        for i in range(kh):
            for j in range(kw):
                hi = i * dil[0]; wj = j * dil[1]
                cols[:, :, i, j] = xs[:, :, hi:hi + strides[0]*(oh-1)+1:strides[0], wj:wj + strides[1]*(ow-1)+1:strides[1]]
        cols = cols.reshape(N, Cg*kh*kw, oh*ow)
        out[:, g*mg:(g+1)*mg] = np.einsum('mk,nkp->nmp', ws.reshape(mg, -1), cols).reshape(N, mg, oh, ow)
    if b is not None:
        out += b.reshape(1, -1, 1, 1)
    if nd == 1:
        out = out[:, :, 0, :]
    return out

def _maxpool(a, x):
    k = a['kernel_shape']; s = a.get('strides', [1]*len(k)); p = a.get('pads', [0]*(2*len(k)))
    nd = len(k)
    if nd == 1:
        x = x[:, :, None, :]; k = [1]+list(k); s=[1]+list(s); p=[0,p[0],0,p[1]]
    x = np.pad(x, ((0,0),(0,0),(p[0],p[2]),(p[1],p[3])), constant_values=-np.inf)
    H, W = x.shape[2:]
    oh = (H - k[0])//s[0] + 1; ow = (W - k[1])//s[1] + 1
    out = np.full(x.shape[:2]+(oh, ow), -np.inf, dtype=x.dtype)
    for i in range(k[0]):
        for j in range(k[1]):
            out = np.maximum(out, x[:, :, i:i+s[0]*(oh-1)+1:s[0], j:j+s[1]*(ow-1)+1:s[1]])
    if nd == 1: out = out[:, :, 0, :]
    return out

def _reshape(a, x, shape):
    shape = [int(v) for v in shape]
    shape = [x.shape[i] if v == 0 and not a.get('allowzero', 0) else v for i, v in enumerate(shape)]
    return x.reshape(shape)

def _slice(a, x, starts=None, ends=None, axes=None, steps=None):
    if starts is None:
        starts, ends, axes = a['starts'], a['ends'], a.get('axes')
    starts = [int(v) for v in np.atleast_1d(starts)]; ends = [int(v) for v in np.atleast_1d(ends)]
    axes = list(range(len(starts))) if axes is None else [int(v) for v in np.atleast_1d(axes)]
    steps = [1]*len(starts) if steps is None else [int(v) for v in np.atleast_1d(steps)]
    sl = [slice(None)]*x.ndim
    for st, en, ax, sp in zip(starts, ends, axes, steps):
        en = None if (sp > 0 and en >= 2**62) else en
        if sp < 0 and en <= -2**62: en = None
        sl[ax] = slice(st, en, sp)
    return x[tuple(sl)]

def _unsqueeze(a, x, axes=None):
    axes = a['axes'] if axes is None else [int(v) for v in np.atleast_1d(axes)]
    for ax in sorted([ax if ax >= 0 else ax + x.ndim + len(axes) for ax in axes]):
        x = np.expand_dims(x, ax)
    return x

def _squeeze(a, x, axes=None):
    axes = a.get('axes') if axes is None else [int(v) for v in np.atleast_1d(axes)]
    return np.squeeze(x, axis=tuple(axes)) if axes is not None else np.squeeze(x)

def _gemm(a, A, B, C=None):
    if a.get('transA', 0): A = A.T
    if a.get('transB', 0): B = B.T
    y = a.get('alpha', 1.0) * (A @ B)
    if C is not None: y = y + a.get('beta', 1.0) * C
    return y.astype(np.float32)

def _pad(a, x, pads=None, value=None, axes=None):
    pads = a.get('pads') if pads is None else [int(v) for v in pads]
    mode = a.get('mode', 'constant')
    n = x.ndim
    pw = [(pads[i], pads[i+n]) for i in range(n)]
    if mode == 'reflect':
        return np.pad(x, pw, mode='reflect')
    v = a.get('value', 0.0) if value is None else float(np.asarray(value).reshape(-1)[0]) if np.asarray(value).size else 0.0
    return np.pad(x, pw, constant_values=v)

def _bn(a, x, s, b, m, v):
    eps = a.get('epsilon', 1e-5)
    sh = (1, -1) + (1,)*(x.ndim-2)
    return ((x - m.reshape(sh)) / np.sqrt(v.reshape(sh) + eps) * s.reshape(sh) + b.reshape(sh)).astype(np.float32)

def _constant(a):
    return a['value']

def _cast(a, x):
    return x.astype(DT[a['to']])

def _shape(a, x):
    return np.array(x.shape, dtype=np.int64)

def _gather(a, x, idx):
    return np.take(x, idx.astype(np.int64), axis=a.get('axis', 0))

def _concat(a, *xs):
    return np.concatenate([np.atleast_1d(x) if x.ndim == 0 else x for x in xs], axis=a['axis'])

def _transpose(a, x):
    return np.transpose(x, a.get('perm'))

def _clip(a, x, lo=None, hi=None):
    lo = a.get('min', -np.inf) if lo is None else lo
    hi = a.get('max', np.inf) if hi is None else hi
    return np.clip(x, lo, hi)

def _reducemean(a, x, axes=None):
    axes = a.get('axes') if axes is None else [int(v) for v in axes]
    return np.mean(x, axis=tuple(axes) if axes is not None else None, keepdims=bool(a.get('keepdims', 1)))

def _flatten(a, x):
    ax = a.get('axis', 1)
    return x.reshape(int(np.prod(x.shape[:ax])), -1)

OPS = {
    'Conv': _conv, 'MaxPool': _maxpool, 'Reshape': _reshape, 'Slice': _slice,
    'Unsqueeze': _unsqueeze, 'Squeeze': _squeeze, 'Gemm': _gemm, 'Pad': _pad,
    'BatchNormalization': _bn, 'Constant': _constant, 'Cast': _cast, 'Shape': _shape,
    'Gather': _gather, 'Concat': _concat, 'Transpose': _transpose, 'Clip': _clip,
    'ReduceMean': _reducemean, 'Flatten': _flatten,
    'Relu': lambda a, x: np.maximum(x, 0),
    'LeakyRelu': lambda a, x: np.where(x > 0, x, x * a.get('alpha', 0.01)).astype(np.float32),
    'Sigmoid': lambda a, x: (1 / (1 + np.exp(-x))).astype(np.float32),
    'Add': lambda a, x, y: x + y, 'Sub': lambda a, x, y: x - y,
    'Mul': lambda a, x, y: x * y, 'Div': lambda a, x, y: x / y,
    'Pow': lambda a, x, y: np.power(x, y).astype(x.dtype),
    'Sqrt': lambda a, x: np.sqrt(x), 'Log': lambda a, x: np.log(x),
    'Max': lambda a, *xs: __import__('functools').reduce(np.maximum, xs),
    'Min': lambda a, *xs: __import__('functools').reduce(np.minimum, xs),
    'MatMul': lambda a, x, y: np.matmul(x, y),
    'Identity': lambda a, x: x,
    'Tanh': lambda a, x: np.tanh(x),
    'Softmax': lambda a, x: (lambda e: e / e.sum(axis=a.get('axis', -1), keepdims=True))(np.exp(x - x.max(axis=a.get('axis', -1), keepdims=True))),
}

if __name__ == '__main__':
    import sys
    for p in sys.argv[1:]:
        m = Model(p)
        print(p, 'opset', m.opset, 'inputs', m.inputs, 'outputs', m.outputs)
        print('  ops', dict(m.ops()))
        for k, v in m.init.items():
            if v.size < 50: print('  init', k, v.shape, v.dtype, v.reshape(-1)[:10])
            else: print('  init', k, v.shape, v.dtype)

def _reducemax(a, x, axes=None):
    axes = a.get('axes') if axes is None else [int(v) for v in axes]
    return np.max(x, axis=tuple(axes) if axes is not None else None, keepdims=bool(a.get('keepdims', 1)))
OPS['ReduceMax'] = _reducemax
