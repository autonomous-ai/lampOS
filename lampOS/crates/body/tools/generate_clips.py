"""The body's clips: the seven ported on 14 Oct, unchanged, and the V1 moves kept on 16 Oct
(lampOS/docs/skills.md, "V1's moves and emotions: what we take"), each cut to its readable window.

A cut clip takes the most active stretch of its recording of the asked length, begun at the stillest
frame near its start; it is scaled as a whole into the body's reach (FARTHEST, at most SCALE_CAP of the
recording), brought back to nothing over its last half second, and slowed under FASTEST, as the first
seven were. Writes Rust source to stdout and a table to stderr.
usage: python3 lampOS/crates/body/tools/generate_clips.py > lampOS/crates/body/src/clips/data.rs
(run from the repository's root; the table of what each clip kept goes to stderr)
"""
import csv, hashlib, sys
import os
# V1's recordings, at the repository's root: lampOS/crates/body/tools/ is four levels down.
REC = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..', '..', 'hal', 'recordings')
RANGE = {'base_yaw': (847, 3275), 'base_pitch': (0, 4095), 'elbow_pitch': (0, 4095), 'wrist_roll': (621, 3254), 'wrist_pitch': (1464, 3071)}
ORDER = ['wrist_pitch', 'wrist_roll', 'base_yaw', 'base_pitch', 'elbow_pitch']
FASTEST = [900.0, 1300.0, 1000.0, 800.0, 800.0]
FARTHEST = [200, 200, 150, 350, 350]
SCALE_CAP = 0.8
# name: (recording, how much of its size, or None to fit FARTHEST; seconds kept, or None for all)
KEPT = {'nod': ('nod', 0.40, None), 'headshake': ('headshake', 0.35, None), 'greeting': ('greeting', 0.40, None),
        'laugh': ('laugh', 0.40, None), 'shy': ('shy', 0.60, None), 'stretching': ('stretching', 0.45, None),
        'idle': ('idle', 0.50, None)}
NEW = {'acknowledge': ('acknowledge', None, 1.6), 'curious': ('curious', None, 3.0), 'happy': ('happy_wiggle', None, 4.0),
       'caring': ('sad', None, 5.0), 'surprised': ('shock', None, 2.0), 'confused': ('confused', None, 3.0),
       'sleepy': ('sleepy', None, 6.0), 'excited': ('excited', None, 3.0), 'scanning': ('scanning', None, 9.0),
       'dance_chill': ('music_chill', None, 9.0), 'dance_groove': ('music_groove', None, 6.0),
       'dance_hype': ('music_hype', None, 7.0), 'dance_waltz': ('music_waltz', None, 9.0),
       'dance_jazz': ('music_jazz', None, 9.0)}

# Reworked on 16 Oct after a person judged every clip on lamp-06c1 (#995): these are not scaled as a whole.
# Each joint gets its own gain, as large as its reach and its speed allow (GAIN_CAP at most), times the
# clip's emphasis, so one joint leads and the rest support it, and none is slowed to fit the fastest.
# Head roll turns the round head about its own axis, which nobody sees, and the body drops a gesture's roll on a lamp
# whose head does not tilt standing (body.rs; read on lamp-06c1's body/joint_states, #995): so these carry none, and
# the roll a recording has is moved onto head pitch and base yaw before the gains.
# name: (recording, seconds kept, emphasis per joint, (roll to pitch, roll to yaw, roll kept))
GAIN_CAP = 1.6
REWORKED = {'curious': ('curious', 3.0, [1.0, 0.0, 1.0, 0.5, 0.6], (0.35, 0.5, 0.0)),
            'happy': ('happy_wiggle', 4.0, [1.0, 0.0, 1.0, 0.6, 1.0], (0.0, 0.0, 0.0)),
            'confused': ('confused', 3.0, [1.0, 0.0, 1.0, 0.5, 0.5], (0.5, 0.4, 0.0)),
            # The full head recoil, less shoulder: under V1's 22 mm tip line with a margin (#1072).
            'surprised': ('shock', 2.0, [1.0, 0.0, 0.0, 0.6, 0.6], (0.0, 0.0, 0.0)),
            'excited': ('excited', 2.0, [1.0, 0.0, 0.0, 0.75, 1.0], (0.0, 0.0, 0.0)),
            # Five dances, each led by its own joint and rhythm.
            'dance_chill': ('music_chill', 9.0, [0.25, 0.0, 1.0, 0.15, 0.12], (0.0, 0.0, 0.0)),  # base yaw sweeps
            'dance_groove': ('music_groove', 8.0, [0.5, 0.0, 0.3, 0.6, 0.7], (0.0, 0.0, 0.0)),  # shoulder bounce
            'dance_hype': ('music_hype', 7.0, [1.0, 0.0, 1.0, 1.0, 1.0], (0.0, 0.0, 0.0)),  # everything, big
            'dance_waltz': ('music_waltz', 9.0, [0.4, 0.0, 0.7, 0.6, 1.0], (0.0, 0.0, 0.0)),  # a slow lean
            'dance_jazz': ('music_jazz', 9.0, [1.0, 0.0, 0.6, 0.25, 0.25], (0.6, 0.4, 0.0))}  # a head bob
# Joints swung about the standing pose, not off to one side of it: the middle of the joint's swing is eased in
# over the first CENTRE_S and out over the last, before the gains (a sweep from -58 to 143 becomes about +-143).
CENTRED = {'dance_chill': [2], 'dance_groove': [3]}
CENTRE_S = 0.6
# Played more than once in a row: a person asked for excited to bounce twice (#995).
REPEATS = {'excited': 2}

def raw(joint, v):
    lo, hi = RANGE[joint]
    return (max(-100.0, min(100.0, v)) + 100.0) / 200.0 * (hi - lo) + lo

def load(rec):
    path = f'{REC}/{rec}.csv'
    rows = list(csv.DictReader(open(path)))
    t = [float(r['timestamp']) for r in rows]
    frames = [[raw(j, float(r[j + '.pos'])) for j in ORDER] for r in rows]
    return hashlib.sha256(open(path, 'rb').read()).hexdigest(), t, frames

def window(t, frames, seconds):
    """The most active stretch of `seconds`, begun at the stillest frame within half a second of its start."""
    dt = (t[-1] - t[0]) / (len(t) - 1)
    width = min(len(frames), int(round(seconds / dt)) + 1)
    move = [sum(abs(frames[k + 1][i] - frames[k][i]) / FARTHEST[i] for i in range(5)) for k in range(len(frames) - 1)] + [0.0]
    best = max(range(len(frames) - width + 1), key=lambda s: sum(move[s:s + width]))
    near = range(max(0, best - int(0.5 / dt)), min(len(frames) - width, best + int(0.5 / dt)) + 1)
    start = min(near, key=lambda s: move[s])
    return start, start + width

out = ["// Generated by crates/body/tools/generate_clips.py from the old runtime's recordings",
       '// (`hal/recordings/NAME.csv`, 20 frames a second, lerobot units on the shared calibration).',
       '// Each clip: offsets in encoder counts from the pose a standing lamp is in, for head pitch,',
       '// head roll, base yaw, shoulder and elbow; scaled as a whole, ended at nothing, and slowed',
       '// until no joint is asked to go faster than 900, 1,300, 1,000, 800 and 800 counts a second.',
       '// The V1 moves kept on 16 Oct are cut to their most active stretch (docs/skills.md); some are', '// reworked joint by joint after a person judged them (#995, the generator\'s REWORKED).',
       '// Do not edit by hand.', 'use super::{Clip, Whole};', '']
def offsets(name, t, frames, gains=None, scale=None):
    """The clip's offsets from its first frame, its gains (or its scale) and its frame time before slowing.
    Given gains or a scale (a whole recording, #8153), the cut's are used, so it moves at the cut's size."""
    if name in REWORKED:
        _, _, emphasis, (to_pitch, to_yaw, roll_kept) = REWORKED[name]
        off = [[fr[i] - frames[0][i] for i in range(5)] for fr in frames]
        for o in off:
            o[0], o[2], o[1] = o[0] + to_pitch * o[1], o[2] + to_yaw * o[1], o[1] * roll_kept
        dt = (t[-1] - t[0]) / (len(t) - 1)
        ease = max(1, int(round(CENTRE_S / dt)))
        for i in CENTRED.get(name, []):
            middle = (max(o[i] for o in off) + min(o[i] for o in off)) / 2
            for k, o in enumerate(off):
                share = min(1.0, k / ease, (len(off) - 1 - k) / ease)
                o[i] -= middle * share
        if gains is None:
            gains = []
            for i in range(5):
                peak = max(abs(o[i]) for o in off)
                fastest = max(abs(off[k + 1][i] - off[k][i]) / dt for k in range(len(off) - 1))
                room = min([GAIN_CAP] + ([FARTHEST[i] * 0.95 / peak] if peak > 0 else []) + ([FASTEST[i] * 0.95 / fastest] if fastest > 0 else []))
                gains.append(round(emphasis[i] * room, 2))
        off = [[o[i] * gains[i] for i in range(5)] for o in off]
        scale = max(gains)
    else:
        if scale is None:
            peak = [max(abs(f[i] - frames[0][i]) for f in frames) for i in range(5)]
            scale = round(min([SCALE_CAP] + [FARTHEST[i] / peak[i] * 0.95 for i in range(5) if peak[i] > 0]), 2)
        off = [[(fr[i] - frames[0][i]) * scale for i in range(5)] for fr in frames]
    n = len(off); tail = min(10, n - 1)
    for k in range(tail + 1):
        share = k / tail
        for i in range(5):
            off[n - 1 - tail + k][i] -= off[-1][i] * share if k < tail else 0.0
    for i in range(5):
        off[-1][i] = 0.0
    return off, gains, scale

def emit(const, name, step_ms, off, doc):
    out.extend(doc)
    out.append(f'pub const {const}: Clip = Clip {{')
    out.append(f'    name: "{name}",')
    out.append(f'    step_ms: {step_ms},')
    out.append('    frames: &[')
    for o in off:
        out.append('        [' + ', '.join(str(round(v)) for v in o) + '],')
    out.append('    ],')
    out.append('};')
    out.append('')

names, wholes, swings = [], [], []
for name, (rec, scale, seconds) in list(KEPT.items()) + list(NEW.items()):
    if name in REWORKED:
        rec, seconds = REWORKED[name][0], REWORKED[name][1]
    digest, t_all, frames_all = load(rec)
    a, b = window(t_all, frames_all, seconds) if seconds else (0, len(frames_all))
    t, frames = t_all[a:b], frames_all[a:b]
    off, gains, scale = offsets(name, t, frames, scale=scale)
    n = len(off)
    if REPEATS.get(name, 1) > 1:
        once = off
        off = once + [list(o) for _ in range(REPEATS[name] - 1) for o in once[1:]]
        n = len(off)
        t = [t[0] + k * (t[1] - t[0]) for k in range(n)]
    dt = (t[-1] - t[0]) / (n - 1)
    speed = [max(abs(off[k + 1][i] - off[k][i]) / dt for k in range(n - 1)) for i in range(5)]
    slow = max(1.0, max(s / f for s, f in zip(speed, FASTEST)))
    step_ms = round(dt * 1000 * slow)
    lo = [round(min(o[i] for o in off)) for i in range(5)]
    hi = [round(max(o[i] for o in off)) for i in range(5)]
    const = name.upper()
    names.append(const)
    # V1's swing of each joint, peak to peak, over all of its recording as V1 played it, before any gain or roll
    # moved: the most level 5 may swing it (#8153, the lead: level 5 is almost V1, `energy::livelier`).
    swings.append((name, [round(max(f[i] for f in frames_all) - min(f[i] for f in frames_all)) for i in range(5)]))
    cut = f', {t[0] - float(load(rec)[1][0]):.2f} s to {t[-1] - float(load(rec)[1][0]):.2f} s of it' if seconds else ''
    size = f'gains {gains} by joint' if name in REWORKED else f'{scale:.2f} of its size'
    emit(const, name, step_ms, off, [f'/// `{rec}.csv` (sha256 {digest[:16]}){cut}, {size}, {n} frames {step_ms} ms apart:',
                                     f'/// {n * step_ms / 1000:.1f} s. Offsets from {lo} to {hi}.'])
    print(f'{name:12} from {rec:13} scale {scale:.2f} {n * step_ms / 1000:5.1f} s  lo {lo} hi {hi}  slowed x{slow:.2f}', file=sys.stderr)
    # A clip cut from its recording also gets the whole recording (#8153), at the cut's gains (or scale) and
    # frame time, once (no repeat): what the energy level lengthens it toward (`energy::livelier`). Never played
    # as is, and only under LAMP_BODY_ENERGY, whose servo ceiling stretches any step past the servo's speed.
    if (a, b) != (0, len(frames_all)):
        whole, _, _ = offsets(name, t_all, frames_all, gains=gains if name in REWORKED else None,
                              scale=None if name in REWORKED else scale)
        wlo = [round(min(o[i] for o in whole)) for i in range(5)]
        whi = [round(max(o[i] for o in whole)) for i in range(5)]
        emit(const + '_WHOLE', name, step_ms, whole,
             [f'/// All of `{rec}.csv`, {len(whole)} frames, at {const}\'s size and frame time: {len(whole) * step_ms / 1000:.1f} s.',
              f'/// {const} is its frames {a} to {b - 1}. Offsets from {wlo} to {whi}.'])
        wholes.append((const, a, b - 1))
        print(f'{"":12} whole {len(whole)} frames, the cut {a} to {b - 1}  lo {wlo} hi {whi}', file=sys.stderr)
out.append('/// Every clip there is.')
out.append('pub const ALL: [&Clip; ' + str(len(names)) + '] = [' + ', '.join('&' + c for c in names) + '];')
out.append('')
out.append("/// V1's swing of each clip's joints, peak to peak, counts, over all of its recording as V1 played it,")
out.append('/// before any gain or roll moved: the most level 5 swings it (#8153, `energy::livelier`).')
out.append('pub const V1_SWING: [(&str, [u16; 5]); ' + str(len(swings)) + '] = [')
for name, swing in swings:
    out.append(f'    ("{name}", [' + ', '.join(str(v) for v in swing) + ']),')
out.append('];')
out.append('')
out.append('/// Each clip cut from its recording, with all of the recording and where the cut lies in it (#8153).')
out.append('pub const WHOLE: [Whole; ' + str(len(wholes)) + '] = [')
for const, a, b in wholes:
    out.append(f'    Whole {{ cut: &{const}, whole: &{const}_WHOLE, from: {a}, to: {b} }},')
out.append('];')
print('\n'.join(out))
