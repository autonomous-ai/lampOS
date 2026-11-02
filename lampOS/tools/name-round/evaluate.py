"""Score heads on the live test (31 Oct) and the rP validation (24-25 Oct).

Configurations:
  live80  lamp-4ace as it ran on 31 Oct: one 80 ms grid, at the phase that
          reproduces its journal (phase 2 of this recording, every line matched)
  avg80   the same at each of the eight 10 ms phases, averaged (a lamp's phase is chance)
  step10  the product unit (--step-ms 10): the highest of the eight grids each step
Hits: calls with a hearing begun from the play's start to 1.5 s after its end.
False calls: hearings begun in the stretches between calls ([end + 1.5 s,
next start - 1 s], and the tail), per hour of those stretches. The lamp's own
answers are in them. Nothing of the live recording is trained on.
"""
import sys, pickle
sys.path.insert(0, '/Users/autonomous/lamp-name-round')
import numpy as np
from frontend import head_scores
from scorer import hearings, plays as live_plays, clock

W = '/Users/autonomous/lamp-name-round/work/'
LIVE_PHASE = 2
VAL_RUNS = ('rP-0033', 'rP-0158', 'rP-0412', 'rP-2239')

def steps(S, to_ms, phase):
    F = len(S)
    if phase == 'all':
        ns = (F - 121) // 8 + 1; s = np.arange(ns)
        st = np.nanmax(np.stack([S[8 * s + 113 + k] for k in range(8)], 1), 1)
        return np.nan_to_num(st), to_ms(32480 + 1280 * s)
    ns = (F - 121 - phase) // 8 + 1; s = np.arange(ns)
    return np.nan_to_num(S[8 * s + 120 + phase]), to_ms(32480 + 160 * phase + 1280 * s)

def count(st, times, bar, calls, test_regions):
    hs = hearings(st, times, bar)
    hit = [any(a <= h[0] <= b for h in hs) for a, b in calls]
    fa = [h for h in hs if any(a <= h[0] <= b for a, b in test_regions)]
    return hit, fa

class LiveSet:
    def __init__(self):
        self.E = np.load(W + 'live_E.npy')
        self.to_ms = clock()
        ps = sorted(live_plays()); self.plays = ps
        self.calls = [(a, b + 1500) for a, b, _, _ in ps]
        m = np.load(W + 'mic2_t.npy'); t_end = m[-1, 1]
        gaps = [(ps[i][1] + 1500, ps[i + 1][0] - 1000, i) for i in range(len(ps) - 1)] + [(ps[-1][1] + 1500, t_end, len(ps) - 1)]
        self.test = [(a, b) for a, b, i in gaps]      # nothing of the live recording is trained on
        self.train = []
        self.test_h = sum(b - a for a, b in self.test) / 3.6e6
        self.all_h = sum(b - a for a, b in self.test + self.train) / 3.6e6

    def score(self, head, bars, all_gaps=False):
        S = head_scores(self.E, *head)
        regions = self.test + self.train if all_gaps else self.test
        hours = self.all_h if all_gaps else self.test_h
        out = {}
        for bar in bars:
            r = {}
            for cfg, ph in [('live80', [LIVE_PHASE]), ('avg80', list(range(8))), ('step10', ['all'])]:
                hits = []; fas = []; per = np.zeros(len(self.calls))
                for p in ph:
                    st, times = steps(S, self.to_ms, p)
                    hit, fa = count(st, times, bar, self.calls, regions)
                    hits.append(sum(hit)); fas.append(len(fa)); per += np.array(hit)
                r[cfg] = dict(hits=float(np.mean(hits)), false_h=float(np.mean(fas)) / hours, per=per / len(ph))
            out[bar] = r
        return out

class ValSet:
    """rP: lamp-4ace mic2, ten kit pause calls a run, eight runs (24-25 Oct)."""
    def __init__(self):
        self.runs = [r for r in pickle.load(open(W + 'rp_E.pkl', 'rb')) if r['name'] in VAL_RUNS]
        from real_data import decode_bus
        self.maps = []
        for r in self.runs:
            # sample index -> unix ms for this run
            import json, base64
            k_c = None
            path = f"/Users/autonomous/lamp-tools/onset/{r['name']}/rec/mic2_audio.jsonl"
            meta = []; n = 0
            for line in open(path):
                d = json.loads(line)
                if 'payload_b64' not in d: continue
                raw = base64.b64decode(d['payload_b64']); k = raw.index(b'\n'); n += (len(raw) - k - 1) // 2
                meta.append((n, d['header']['at_unix_ms']))
            m = np.array(meta, float); kk, cc = np.polyfit(m[:, 0], m[:, 1], 1)
            self.maps.append(lambda i, kk=kk, cc=cc: kk * np.asarray(i, float) + cc)
        self.hours = 0
        for r, f in zip(self.runs, self.maps):
            ps = sorted([(f(p['s0']), f(p['s1'])) for p in r['plays']])
            r['calls'] = [(a, b + 1500) for a, b in ps]
            t0, t1 = f(0), f(r['n'])
            edges = [(t0 + 2500, ps[0][0] - 1000)] + [(ps[i][1] + 1500, ps[i + 1][0] - 1000) for i in range(len(ps) - 1)] + [(ps[-1][1] + 1500, t1)]
            r['gaps'] = [(a, b) for a, b in edges if b > a]
            self.hours += sum(b - a for a, b in r['gaps']) / 3.6e6

    def score(self, head, bars, phases=range(8)):
        out = {}
        Ss = [head_scores(r['E'].astype(np.float32), *head) for r in self.runs]
        for bar in bars:
            hits = 0; fas = 0; n = 0
            for r, f, S in zip(self.runs, self.maps, Ss):
                for p in phases:
                    st, times = steps(S, f, p)
                    hit, fa = count(st, times, bar, r['calls'], r['gaps'])
                    hits += sum(hit); fas += len(fa)
                n += len(r['calls'])
            k = len(list(phases))
            out[bar] = dict(hits=hits / k, of=n, false_h=fas / k / self.hours)
        return out

if __name__ == '__main__':
    from frontend import load_head
    H = load_head('/Users/autonomous/lamp-voice-data/name-models/name.onnx')
    L = LiveSet()
    print('live test stretches %.3f h, all gaps %.3f h' % (L.test_h, L.all_h))
    for bar, r in L.score(H, (0.5, 0.7, 0.8, 0.9)).items():
        print('shipped live bar', bar, {k: (round(v['hits'], 1), round(v['false_h'], 1)) for k, v in r.items()})
    for bar, r in L.score(H, (0.5, 0.7, 0.9), all_gaps=True).items():
        print('shipped live ALL gaps bar', bar, {k: (round(v['hits'], 1), round(v['false_h'], 1)) for k, v in r.items()})
    V = ValSet()
    print('val hours %.3f' % V.hours)
    print('shipped val', V.score(H, (0.5, 0.7, 0.9)))
