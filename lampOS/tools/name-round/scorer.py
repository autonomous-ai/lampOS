"""Offline scorer for the live mic2 recording (lamp-4ace, 31 Oct, #3923).

Replays the recording's embeddings (work/live_E.npy, from embed_live.py)
through a head, makes lamp-name's 80 ms step scores (--step-ms 10: the
highest of the eight 10 ms grids), runs crates/name/src/said.rs's hearing
rule (a run at or over the bar is one hearing; said when it falls or after
0.8 s; then 1 s held off), and counts:
  hits: plays with a hearing begun from the play's start to 1.5 s after its end
  false: hearings begun outside every play window [start - 0.5 s, end + 15 s
         is NOT used]; see `stretches` for what counts as a non-call stretch.
"""
import re, sys
import numpy as np
sys.path.insert(0, '/Users/autonomous/lamp-name-round')
from frontend import head_scores, step_scores, load_head

W = '/Users/autonomous/lamp-name-round/work/'
REC = '/Users/autonomous/lamp-tools/namerec/'
LONGEST, HOLD_OFF = 800.0, 1000.0

def plays():
    out = []
    for l in open(REC + 'plays.tsv'):
        a, b, i, v = l.rstrip('\n').split('\t')
        out.append((int(a), int(b), i, v))
    return out

def clock():
    """Sample index -> unix ms (lamp clock), a line fitted to the bus blocks' at_unix_ms."""
    m = np.load(W + 'mic2_t.npy')
    idx = m[:, 0].astype(float) + 320; t = m[:, 1].astype(float)
    A = np.stack([idx, np.ones_like(idx)], 1)
    k, c = np.linalg.lstsq(A, t, rcond=None)[0]
    return lambda i: k * np.asarray(i, float) + c

def hearings(scores, times, bar):
    """said.rs Hearings: list of (began_ms, said_ms, best)."""
    out = []; open_ = None; quiet_until = -1e18
    for s, t in zip(scores, times):
        if t < quiet_until:
            continue
        if open_ is not None:
            if s >= bar:
                open_[1] = max(open_[1], min(s, 1.0))
                if t - open_[0] >= LONGEST:
                    out.append((open_[0], t, open_[1])); open_ = None; quiet_until = t + HOLD_OFF
            else:
                out.append((open_[0], t, open_[1])); open_ = None; quiet_until = t + HOLD_OFF
        elif s >= bar:
            open_ = [t, min(s, 1.0)]
    return out

class Live:
    def __init__(self):
        self.E = np.load(W + 'live_E.npy')
        self.plays = plays()
        self.to_ms = clock()

    def steps(self, head, mode='10'):
        S = head_scores(self.E, *head)
        st = step_scores(S, mode)
        times = self.to_ms(32480 + 1280 * np.arange(len(st)))
        st = np.nan_to_num(st, nan=0.0)
        return st, times

    def windows(self, after_ms=1500):
        return [(a, b + after_ms) for a, b, _, _ in self.plays]

    def score(self, head, bars=(0.9,), mode='10', st=None):
        if st is None:
            st, times = self.steps(head, mode)
        else:
            st, times = st
        res = {}
        hit_win = self.windows(1500)
        # Non-call stretches: outside [start - 1 s, end + 1.5 s] of every play.
        # The lamp's own answers fall inside these stretches (it answered each
        # call), so they are counted as false calls: its own voice must never
        # call it.
        excl = [(a - 1000, b + 1500) for a, b, _, _ in self.plays]
        rec0, rec1 = times[0], times[-1]
        excl_ms = sum(min(b, rec1) - max(a, rec0) for a, b in excl if b > rec0 and a < rec1)
        free_h = ((rec1 - rec0) - excl_ms) / 3.6e6
        for bar in bars:
            hs = hearings(st, times, bar)
            per = []
            for (a, b), p in zip(hit_win, self.plays):
                h = [x for x in hs if a <= x[0] <= b]
                per.append(bool(h))
            fa = [x for x in hs if not any(a <= x[0] <= b for a, b in excl)]
            res[bar] = dict(hits=sum(per), per=per, false=len(fa), false_per_h=len(fa) / free_h,
                            false_list=fa, free_h=free_h)
        return res

    def best_in_plays(self, st, times, after_ms=1500):
        out = []
        for a, b, i, v in self.plays:
            m = (times >= a) & (times <= b + after_ms)
            out.append(float(st[m].max()) if m.any() else 0.0)
        return np.array(out)

def logged():
    ev = []
    for l in open(REC + 'name.log'):
        m = re.match(r'(\d+\.\d+).*lamp-name: (heard its name|under its bars): (?:score|best)=([\d.]+) began_ms_ago=(\d+)', l)
        if m:
            ev.append((float(m.group(1)) * 1000, m.group(2), float(m.group(3)), int(m.group(4))))
    return ev
