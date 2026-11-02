"""The lamp-name front end in NumPy, at every 10 ms position at once.

Mirrors lampOS/crates/name/src/hear.rs:
  - melspectrogram.onnx on chunks of 1,760 samples, a step of 1,280: eight
    frames of 32 per chunk, each frame 512 samples at a hop of 160. The model
    clips each chunk at (its own max - 80 dB); then x/10 + 2.
  - embedding_model.onnx on 76 frames, a new one every 8 frames (80 ms).
  - the head on the newest 16 embeddings.
A grid with offset g*160 samples (--step-ms 10 makes g = 0..7) sees the same
frame sequence shifted by g frames, so embedding E[f] (frames f..f+75) for
f = g (mod 8) is grid g's. The embedding net is valid in time with three
time pools of 2, so it runs fully convolutionally over each residue class.
"""
import numpy as np
from onnxmini import Model

MODELS = '/Users/autonomous/lamp-voice-data/name-models/'
CHUNK, STEP, HOP, WIN = 1760, 1280, 160, 512

class Front:
    def __init__(self, models=MODELS):
        mel = Model(models + 'melspectrogram.onnx')
        self.re = mel.init['0.stft.conv_real.weight'][:, 0, :].astype(np.float32)   # 257 x 512
        self.im = mel.init['0.stft.conv_imag.weight'][:, 0, :].astype(np.float32)
        self.melW = mel.init['1.melW'].astype(np.float32)                         # 257 x 32
        self.mel_model = mel
        emb = Model(models + 'embedding_model.onnx')
        self.emb_model = emb
        # ordered conv layers from the graph
        self.layers = []
        for n in emb.nodes:
            if n['op'] == 'Conv':
                w = emb.init[n['ins'][1]].astype(np.float32)
                b = emb.init[n['ins'][2]].astype(np.float32) if len(n['ins']) > 2 else None
                self.layers.append(('conv', w, b, n['attrs'].get('pads', [0, 0, 0, 0])))
            elif n['op'] == 'LeakyRelu':
                self.layers.append(('act', None, None, None))
            elif n['op'] == 'MaxPool':
                self.layers.append(('pool', tuple(n['attrs']['kernel_shape']), None, None))

    # ---------- mel ----------
    def logmel(self, x):
        """Raw 10*log10 mel power of every frame f (samples 160f..160f+512), before clipping."""
        x = np.asarray(x, dtype=np.float32)
        nf = (len(x) - WIN) // HOP + 1
        idx = np.arange(WIN)[None, :] + HOP * np.arange(nf)[:, None]
        out = np.empty((nf, 32), dtype=np.float32)
        for a in range(0, nf, 20000):
            fr = x[idx[a:a+20000]]
            p = (fr @ self.re.T) ** 2 + (fr @ self.im.T) ** 2
            m = p @ self.melW
            out[a:a+20000] = 10 * np.log10(np.maximum(m, 1e-10))
        return out

    def frames_for_residue(self, lm, r):
        """Grid r's frames (from frame r on), each chunk of 8 clipped at its max - 80, scaled."""
        f = lm[r:]
        n = (len(f) // 8) * 8
        f = f[:n].reshape(-1, 8, 32)
        mx = f.max(axis=(1, 2), keepdims=True)
        f = np.maximum(f, mx - 80.0)
        return (f.reshape(-1, 32) / 10.0 + 2.0).astype(np.float32)

    # ---------- embedding ----------
    def _net(self, x):
        """x: (T, 32) frames -> ((T-76)//8 + 1, 96) embeddings at an 8-frame stride."""
        h = x[None, :, :]                     # C, T, W
        for kind, w, b, pads in self.layers:
            if kind == 'conv':
                M, C, kh, kw = w.shape
                pw = pads[1] if pads else 0
                if pw:
                    h = np.pad(h, ((0, 0), (0, 0), (pw, pads[3])))
                T = h.shape[1] - kh + 1; W = h.shape[2] - kw + 1
                cols = np.empty((C, kh, kw, T, W), dtype=np.float32)
                for i in range(kh):
                    for j in range(kw):
                        cols[:, i, j] = h[:, i:i+T, j:j+W]
                h = np.tensordot(w.reshape(M, -1), cols.reshape(C*kh*kw, T*W), axes=1).reshape(M, T, W)
                if b is not None:
                    h += b[:, None, None]
            elif kind == 'act':
                h = np.maximum(np.where(h > 0, h, h * np.float32(0.2)), np.float32(-0.4))
            else:
                kt, kw = w
                C, T, W = h.shape
                h = h[:, :T - T % kt, :W - W % kw].reshape(C, T // kt, kt, W // kw, kw).max(axis=(2, 4))
        # C=96, T=K, W=1
        return h[:, :, 0].T.copy()

    def embed_frames(self, fr, seg=512):
        """Embeddings of fr[8k : 8k+76] for every k."""
        K = (len(fr) - 76) // 8 + 1
        if K <= 0:
            return np.zeros((0, 96), np.float32)
        out = np.empty((K, 96), dtype=np.float32)
        for k0 in range(0, K, seg):
            k1 = min(K, k0 + seg)
            out[k0:k1] = self._net(fr[8*k0: 8*(k1-1) + 76])
        return out

    def embeddings_all(self, x, residues=range(8)):
        """E[f] for every 10 ms frame position f (grid f % 8's), as an (F, 96) array; NaN where absent."""
        lm = self.logmel(x)
        F = len(lm) - 75
        E = np.full((max(F, 0), 96), np.nan, dtype=np.float32)
        for r in residues:
            fr = self.frames_for_residue(lm, r)
            e = self.embed_frames(fr)
            pos = r + 8 * np.arange(len(e))
            ok = pos < F
            E[pos[ok]] = e[ok]
        return E

def head_scores(E, w0, b0, w1, b1, w2, b2, stride=8, ctx=16):
    """S[f] = head(E[f-120], ..., E[f]) (16 embeddings 80 ms apart, newest at f); NaN before."""
    F = len(E)
    S = np.full(F, np.nan, dtype=np.float32)
    span = stride * (ctx - 1)
    if F <= span:
        return S
    idx = np.arange(span, F)[:, None] - stride * np.arange(ctx - 1, -1, -1)[None, :]
    for a in range(0, len(idx), 20000):
        X = E[idx[a:a+20000]].reshape(-1, ctx * 96)
        h = np.maximum(X @ w0 + b0, 0)
        h = np.maximum(h @ w1 + b1, 0)
        z = (h @ w2 + b2)[:, 0]
        S[span + a: span + a + len(z)] = 1 / (1 + np.exp(-z))
    return S

def load_head(path):
    m = Model(path)
    i = m.init
    return [i['w0'], i['b0'], i['w1'], i['b1'], i['w2'], i['b2']]

def step_scores(S, mode='10'):
    """Each 80 ms step's score as lamp-name gives it. Step s is whole at sample 32480 + 1280 s.
    mode '80': S[8s+120]; mode '10' (--step-ms 10): max of S[8s+113 .. 8s+120]."""
    F = len(S)
    ns = (F - 121) // 8 + 1
    if ns <= 0:
        return np.zeros(0, np.float32)
    s = np.arange(ns)
    if mode == '80':
        return S[8 * s + 120]
    W = np.stack([S[8 * s + 113 + k] for k in range(8)], axis=1)
    return np.nanmax(W, axis=1)

def step_sample(s):
    return 32480 + 1280 * np.asarray(s)
