#!/usr/bin/env python3
"""The four quality rows of docs/build-rows.md, scored from one repeatable played run on whatever build the lamp runs.

usage: quality_rows.py run LABEL [--stops N]   (night.py, cutin.sh, the stop scene, then score; plays sound, ~70 min)
       quality_rows.py score LABEL             (scores the three outputs already in $PLAYED_OUT)
       quality_rows.py selftest                (the scorer on built-in fixtures: no lamp, nothing played)
       quality_rows.py level SRC DST           (one clip trimmed and levelled to -20 LUFS, as every clip of a run)

`run` plays, in order: night.py LABEL-night (every scene), cutin.sh LABEL-cutin 1 1, and the stop scene below, then
scores. Every clip is trimmed and levelled to -20 LUFS first: night.py's kit (from NIGHT_WAV, default
~/lamp-played/desk-kit-2/wav, into $PLAYED_OUT/levelled-kit, given to night.py as its NIGHT_WAV), cutin.sh's say lines
(into scenario.py's cache) and the stop scene's lines; the Mac is at volume 45 for all but the stop lines. Nothing on the
lamp is changed (no switches, no drop-ins). It takes no lamp lock: on a shared lamp run it under
lamp-lock.sh <lamp> run, as the kit's other scripts. The stop scene, N trials (default 10): a quiet lamp (as night.py),
"Hey Lamp, tell me a long story about <topic>." at Mac volume 45, the journal's "first answer audio" line (none in 14 s:
void), 1.5 s, then a stop line at volume 90 (Seventh's clips in STOP_KIT when present, else say). Stopped: the story's turn_finished as user_interrupted within 2.0 s of the stop clip's
end (the gap is from its start). Not stopped and still speaking: the next stop phrasing once more, "stopped on the second",
still a miss. Saves $PLAYED_OUT/quality/LABEL-stop.json, and the score as LABEL.txt and LABEL.json. cutin.sh runs with
CUTIN_ENV unset and its scenes through CUTIN_SCENE (default cutin_scene.py beside it), which plays its closing stop
line at Mac 90 after the journal's first answer audio + 1.5 s; without that file cutin.sh is not run. Cutting in has a
handful of trials: it is reported as an indication, not a score."""
import hashlib, json, os, re, shutil, signal, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.environ.get('PLAYED_OUT') or os.path.expanduser('~/lamp-played')
ROWS = [('Cutting in', 0.75), ('Stop words', 0.75), ('Knows when you have finished', 0.85), ('Not spoken to', 0.90)]
TOPICS = ['a lighthouse keeper', 'a lost kite', 'a fox who learned to swim', 'a clockmaker', 'a sailing trip',
          'a baker who could not sleep', 'a snowy mountain village', 'a robot gardener', 'a river otter', 'a night train']
STOPS = ['Lamp, stop.', 'Stop, stop.', 'Stop now.', 'Okay, stop.']
STOP_WINDOW = 2.0   # seconds after the stop clip's end for the story's user_interrupted


def volume(v): subprocess.run(['/usr/bin/osascript', '-e', f'set volume output volume {v}'], check=False)


# Seventh's levelled stop clips (one person, varied phrasings), used for the stop lines when present: re-levelled
# here like every other clip. Absent, the line is spoken by say as below.
STOP_KIT = os.environ.get('STOP_KIT', os.path.expanduser('~/seventh-watch/kit/clips'))
STOP_CLIPS = {'Lamp, stop.': 'dan-lamp-stop-5ea099', 'Stop, stop.': 'dan-stop-stop-fbf2ac', 'Stop now.': 'dan-stop-now-545057',
              'Okay, stop.': 'dan-okay-stop-b23ce0'}
LUFS = -20.0
TRIM = 'silenceremove=start_periods=1:start_threshold=-50dB'


def loudness(path, pre=''):
    """Integrated loudness (EBU R128, ffmpeg's loudnorm measure) of path, after the filters `pre`."""
    err = subprocess.run(['ffmpeg', '-nostats', '-hide_banner', '-i', path, '-af', (pre + ',' if pre else '') + 'loudnorm=print_format=json',
                          '-f', 'null', '-'], capture_output=True, text=True).stderr
    return float(json.loads(err[err.rindex('{'):err.rindex('}') + 1])['input_i'])


def level(src, dst, lufs=LUFS):
    """dst = src with its silence trimmed at both ends, at `lufs` integrated loudness, peak-limited, mono 48 kHz 16-bit.
    The one levelling for every clip of a run (night.py's kit, cutin.sh's lines, the stop scene, the session's wake),
    so all are equally loud. A second and third pass correct what the limiter moved; kept if a file is already there."""
    if os.path.exists(dst): return dst
    os.makedirs(os.path.dirname(dst) or '.', exist_ok=True)
    pre = f'{TRIM},areverse,{TRIM},areverse'
    gain = lufs - loudness(src, pre); tmp = dst + '.tmp.wav'
    for _ in range(3):
        subprocess.run(['ffmpeg', '-loglevel', 'error', '-y', '-i', src, '-af', f'{pre},volume={gain:.2f}dB,alimiter=limit=0.89',
                        '-ar', '48000', '-ac', '1', '-c:a', 'pcm_s16le', tmp], check=True)
        miss = lufs - loudness(tmp)
        if abs(miss) <= 0.5: break
        gain += miss
    if abs(miss) > 1.0: print(f'quality_rows.py: {src} levelled to {lufs - miss:.1f} LUFS, not {lufs:g}', file=sys.stderr)
    os.rename(tmp, dst); return dst


def clip(text):
    """The line levelled to -20 LUFS, cached by its text's sha: Seventh's stop clip for a stop line when there is one,
    else spoken by say (Daniel)."""
    d = f'{OUT}/quality/clips'; os.makedirs(d, exist_ok=True)
    wav = f'{d}/{hashlib.sha256(text.encode()).hexdigest()[:16]}.wav'
    if os.path.exists(wav): return wav
    kit = f'{STOP_KIT}/{STOP_CLIPS.get(text, "-")}.wav'
    if os.path.exists(kit): return level(kit, wav)
    aiff = wav[:-4] + '.aiff'
    subprocess.run(['/usr/bin/say', '-v', 'Daniel', '-o', aiff, text], check=True)
    try: return level(aiff, wav)
    finally: os.remove(aiff)


def level_kit(src, dst):
    """night.py's kit WAVs (src) levelled into dst, for NIGHT_WAV: desk-kit-2's renders are -16 to -18 LUFS as made."""
    names = sorted(f for f in os.listdir(src) if f.endswith('.wav'))
    if not names: sys.exit(f'quality_rows.py: no WAVs in {src}')
    for f in names: level(f'{src}/{f}', f'{dst}/{f}')
    return dst


# cutin.sh's lines as scenario.py says them (voice, text): written levelled into scenario.py's cache under the key it
# looks up (sha256 of voice|rate|text), so cutin.sh plays them at the same loudness as the rest. Keep in step with
# cutin.sh's ASK, STOP and episode lines.
CUTIN_ASK = 'Hey Lamp, tell me a long story about a lighthouse keeper.'
CUTIN_LINES = [('Daniel', CUTIN_ASK + ' Make it a long one.'), ('Daniel', CUTIN_ASK), ('Daniel', "Lamp, stop. That's enough."),
               ('Samantha', 'Hmm, interesting.'), ('Samantha', "Wait, what was the keeper's name?")]


def level_cutin_lines(out):
    rate = os.environ.get('LAMP_SAY_RATE', '185'); gen = f'{out}/prompts/generated'; os.makedirs(gen, exist_ok=True)
    for voice, text in CUTIN_LINES:
        path = f"{gen}/{hashlib.sha256(f'{voice}|{rate}|{text}'.encode()).hexdigest()[:16]}.wav"
        if os.path.exists(path):
            got = loudness(path)
            if abs(got - LUFS) > 1.0: print(f'quality_rows.py: {path} is {got:.1f} LUFS (not levelled here; left as it is)', file=sys.stderr)
            continue
        aiff = path[:-4] + '.aiff'
        subprocess.run(['/usr/bin/say', '-v', voice, '-r', rate, '-o', aiff, text], check=True)
        try: level(aiff, path)
        finally: os.remove(aiff)
        with open(f'{gen}/index.tsv', 'a') as f: f.write(f"{os.path.basename(path)[:-4]}\t{voice}\t{rate}\t{text}\n")


CUTIN_SCENE = os.environ.get('CUTIN_SCENE') or os.path.join(HERE, 'cutin_scene.py')


class FirstAudio:
    """lamp-voice's journal followed over ONE ssh for the whole scene: each "first answer audio" line with this Mac's time
    it came. A new ssh to a lamp can take 5-6 s to connect (52e6, 30 Oct 01:0x), so a poll per check misses every story."""
    def __init__(self, ssh):
        import queue, threading
        self.q = queue.Queue()
        self.p = subprocess.Popen(ssh + ["exec journalctl -u lamp-voice -f -n 0 -o cat"], stdout=subprocess.PIPE,
                                  stdin=subprocess.DEVNULL, text=True, bufsize=1)
        def read():
            for line in self.p.stdout:
                if 'first answer audio' in line: self.q.put((time.time(), line.strip()))
        threading.Thread(target=read, daemon=True).start()

    def after(self, t, longest):
        """The Mac time of the first such line that came at or after t, waiting up to `longest` s, or None."""
        import queue
        end = time.time() + longest
        while True:
            try: at, _ = self.q.get(timeout=max(0.0, end - time.time()))
            except queue.Empty: return None
            if at >= t: return at

    def close(self):
        self.p.terminate()


def play(path):
    start = time.time(); subprocess.run(['/usr/bin/afplay', path], check=True); return start, time.time()


def stopped_after(ev, story, start, end):
    """The gap from the stop clip's start to the story's user_interrupted, if it came within STOP_WINDOW of the end."""
    deadline = end + STOP_WINDOW
    while True:
        ev.pump()
        for r in ev.rows:
            if (start <= r['_mac'] <= deadline and r.get('kind') == 'turn_finished' and r.get('outcome') == 'user_interrupted'
                    and (story is None or r.get('turn') == story)):
                return round(r['_mac'] - start, 2)
        if time.time() > deadline: return None
        time.sleep(0.05)


def stop_scene(label, n):
    sys.path.insert(0, HERE); import night
    ev = night.Events(); fa = FirstAudio(night.SSH); time.sleep(8); trials, played = [], []
    try:
        for i in range(n):
            # night.run_scene's quiet wait: nothing playing, and QUIET_BEFORE s since anything played or was said.
            while True:
                ev.pump()
                last = max([r['_mac'] for r in ev.rows if r.get('kind') in ('speaker_first_write', 'speech_final_sample_retired')]
                           + played + [0])
                if not ev.speaking() and time.time() - last >= night.QUIET_BEFORE: break
                time.sleep(0.5)
            topic, line = TOPICS[i % len(TOPICS)], STOPS[i % len(STOPS)]
            t = {'topic': topic, 'stop_line': line, 'result': 'no story, void', 'gap_s': None}
            volume(45); _, ask_end = play(clip(f'Hey Lamp, tell me a long story about {topic}.')); played.append(ask_end)
            # The stop is timed from the journal's "first answer audio"; the event file only names the story's turn.
            first = fa.after(ask_end, 14.0)
            if first is not None:
                ev.pump()
                w = next((r for r in reversed(ev.rows) if r['_mac'] >= ask_end and r.get('kind') == 'speaker_first_write'), {})
                story = (w.get('owner') or {}).get('turn', w.get('turn'))
                time.sleep(1.5); volume(90); s, e = play(clip(line)); volume(45); played.append(e)
                t['gap_s'] = stopped_after(ev, story, s, e)
                t['result'] = 'stopped' if t['gap_s'] is not None else 'not stopped'
                if t['gap_s'] is None and ev.speaking():
                    volume(90); s, e = play(clip(STOPS[(i + 1) % len(STOPS)])); volume(45); played.append(e)
                    if stopped_after(ev, story, s, e) is not None: t['result'] = 'stopped on the second'
            trials.append(t); print(f"stop {i + 1}/{n}: {topic}; {line!r}: {t['result']} {t['gap_s'] or ''}", flush=True)
    finally:
        volume(45); fa.close(); ev.close(); night.stop_on_lamp()
    os.makedirs(f'{OUT}/quality', exist_ok=True)
    json.dump({'label': label, 'at': time.strftime('%Y-%m-%d %H:%M'), 'trials': trials}, open(f'{OUT}/quality/{label}-stop.json', 'w'), indent=1)


def read(path, how):
    try:
        with open(path) as f: return how(f)
    except (OSError, ValueError): return None


def row(name, bar, passes, trials, detail, indication=False):
    if not trials: return {'row': name, 'bar': bar, 'measured': False, 'line': f'{name}  not measured  bar {bar}', 'detail': detail}
    sc = round(passes / trials, 2)
    if indication:  # too few trials to hold against the bar
        return {'row': name, 'bar': bar, 'measured': True, 'indication': True, 'passes': passes, 'trials': trials, 'score': sc,
                'pass': None, 'line': f'{name}  {passes}/{trials} = {sc:.2f}  indication ({trials} trials), not a score  bar {bar}',
                'detail': detail}
    return {'row': name, 'bar': bar, 'measured': True, 'passes': passes, 'trials': trials, 'score': sc, 'pass': sc >= bar,
            'line': f"{name}  {passes}/{trials} = {sc:.2f}  bar {bar}  {'PASS' if sc >= bar else 'below'}", 'detail': detail}


def score(label, out=OUT):
    night = read(f'{out}/night/{label}-night.json', json.load)
    cutin = read(f'{out}/scenarios/cutin-{label}-cutin.txt', lambda f: f.read().splitlines())
    stop = read(f'{out}/quality/{label}-stop.json', json.load)
    bars = dict(ROWS); rows = []
    # Cutting in: each summary entry's first word is its decision; a cut-in should commit, an aside resume.
    got, detail = [], []
    for l in cutin or []:
        m = re.match(r'^(aside|cutin)\s+(\d+) s: (.*)', l)
        if m:
            for e in m.group(3).split('; '):
                d = e.split()[0] if e.split() else ''
                got.append(d == {'cutin': 'commit', 'aside': 'resume'}[m.group(1)]); detail.append(f'{m.group(1)} {m.group(2)} s: {d}')
        elif l.startswith('idle:'): detail.append(l)
    rows.append(row('Cutting in', bars['Cutting in'], sum(got), len(got), detail if cutin is not None else ['no cutin report'],
                    indication=True))
    # Stop words: stopped on the first stop line, of the trials where a story began.
    real = [t for t in (stop or {}).get('trials', []) if t['result'] != 'no story, void']
    gaps = [t['gap_s'] for t in real if t['result'] == 'stopped']
    rows.append(row('Stop words', bars['Stop words'], len(gaps), len(real),
                    [f'gaps {gaps}', f"stopped on the second {sum(t['result'] == 'stopped on the second' for t in real)}",
                     f"void {len((stop or {}).get('trials', [])) - len(real)}"] if stop is not None else ['no stop scene']))
    nr = (night or {}).get('rows', [])
    ends = [r for r in nr if r.get('kind') in ('pause', 'long')]
    ok = [r for r in ends if r.get('answered') and len(r.get('pieces') or []) == 1]
    rows.append(row('Knows when you have finished', bars['Knows when you have finished'], len(ok), len(ends),
                    [f"missed {r['scene']} {r['id']} answered={r.get('answered')} pieces={r.get('pieces')}" for r in ends if r not in ok]
                    if night is not None else ['no night round']))
    desk = [r for r in nr if r.get('kind') == 'desk']
    held = [r for r in desk if not r.get('answered')]
    rows.append(row('Not spoken to', bars['Not spoken to'], len(held), len(desk),
                    [f"answered {r['scene']} {r['id']}" for r in desk if r.get('answered')] if night is not None else ['no night round']))
    build = next((l[len('build: '):] for l in cutin or [] if l.startswith('build: ')), 'not read')
    first = ((night or {}).get('summary') or {}).get('first_ms_median')
    txt = [f'quality {label}  build: {build}  time to first sound: {f"{first} ms" if first is not None else "not measured"}']
    for r in rows: txt += [r['line']] + ['    ' + d for d in r['detail']]
    os.makedirs(f'{out}/quality', exist_ok=True)
    open(f'{out}/quality/{label}.txt', 'w').write('\n'.join(txt) + '\n')
    json.dump({'label': label, 'build': build, 'first_ms_median': first, 'rows': rows}, open(f'{out}/quality/{label}.json', 'w'), indent=1)
    print('\n'.join(txt)); return rows


def selftest():
    d = tempfile.mkdtemp(); os.makedirs(f'{d}/night'); os.makedirs(f'{d}/scenarios'); os.makedirs(f'{d}/quality')
    def n(kind, answered, pieces, scene='s'): return {'scene': scene, 'id': 'x', 'kind': kind, 'answered': answered, 'pieces': pieces}
    json.dump({'summary': {'first_ms_median': 1420},
               'rows': [n('pause', True, [1]), n('pause', True, [1, 2]), n('long', True, [3]), n('long', False, []), n('ask', True, [4]),
                        n('desk', False, []), n('desk', True, [5]), n('desk', False, [], 'after1')]}, open(f'{d}/night/t-night.json', 'w'))
    open(f'{d}/scenarios/cutin-t-cutin.txt', 'w').write(
        'cutin t-cutin  2026-10-29T00:00:00Z  lamp x\nbuild: /opt/lampOS/voice/current/lamp-voice sha256 0123456789abcdef\n'
        'idle: 1 of 1 stories; 0 false yields in 1.50 min of reply heard: 0.00 per min\n'
        'aside  3 s: resume (onset->yield 300 ms, ->decision 900 ms, answer accepted - / acoustic - ms)\n'
        'cutin  3 s: commit (onset->yield 280 ms, ->decision 700 ms, answer accepted 1200 / acoustic 1300 ms)\n'
        'aside  8 s: commit (onset->yield 310 ms, ->decision 650 ms, answer accepted - / acoustic - ms)\n'
        'cutin  8 s: none (onset->yield - ms, ->decision - ms, answer accepted - / acoustic - ms)\n')
    trial = lambda res, gap=None: {'topic': 't', 'stop_line': 'Stop now.', 'result': res, 'gap_s': gap}
    json.dump({'trials': [trial('stopped', 0.9), trial('stopped', 1.4), trial('stopped on the second'), trial('not stopped'),
                          trial('no story, void')]}, open(f'{d}/quality/t-stop.json', 'w'))
    r = {x['row']: x for x in score('t', d)}
    assert (r['Cutting in']['passes'], r['Cutting in']['trials']) == (2, 4) and r['Cutting in']['pass'] is None
    assert r['Cutting in']['indication'] and 'indication (4 trials), not a score' in r['Cutting in']['line']
    assert (r['Stop words']['passes'], r['Stop words']['trials'], r['Stop words']['score']) == (2, 4, 0.5)
    assert 'stopped on the second 1' in r['Stop words']['detail'] and 'gaps [0.9, 1.4]' in r['Stop words']['detail']
    assert (r['Knows when you have finished']['passes'], r['Knows when you have finished']['trials']) == (2, 4)
    assert (r['Not spoken to']['passes'], r['Not spoken to']['trials']) == (2, 3)
    out = open(f'{d}/quality/t.txt').read()
    assert 'build: /opt/lampOS/voice/current/lamp-voice sha256 0123456789abcdef' in out and 'time to first sound: 1420 ms' in out
    assert 'idle: 1 of 1 stories' in out
    os.remove(f'{d}/quality/t-stop.json')
    r = {x['row']: x for x in score('t', d)}
    assert not r['Stop words']['measured'] and 'Stop words  not measured' in r['Stop words']['line'] and r['Cutting in']['measured']
    # level(): a quiet tone with silence around it comes out trimmed and at -20 LUFS.
    src = f'{d}/tone.wav'
    subprocess.run(['ffmpeg', '-loglevel', 'error', '-y', '-f', 'lavfi', '-i', 'anullsrc=r=16000:cl=mono:d=1', '-f', 'lavfi', '-i',
                    'sine=f=300:r=16000:d=1.5', '-f', 'lavfi', '-i', 'anullsrc=r=16000:cl=mono:d=1', '-filter_complex',
                    '[1]volume=-15dB[t];[0][t][2]concat=n=3:v=0:a=1', src], check=True)
    out = level(src, f'{d}/tone-l.wav')
    import wave
    with wave.open(out) as w: secs = w.getnframes() / w.getframerate()
    assert abs(loudness(out) - LUFS) <= 0.5 and 1.3 < secs < 1.7, (loudness(out), secs)
    shutil.rmtree(d); print('quality_rows.py selftest: ok')


def main():
    a = sys.argv[1:]
    if a[:1] == ['selftest']: return selftest()
    if a[:1] == ['level'] and len(a) == 3: return print(level(a[1], a[2]))
    if len(a) < 2 or a[0] not in ('run', 'score'): sys.exit(__doc__)
    label = a[1]
    if a[0] == 'score': return score(label)
    n = int(a[a.index('--stops') + 1]) if '--stops' in a else 10
    if not os.environ.get('LAMP_HOST'):
        print('quality_rows.py: LAMP_HOST is not set (README.md); nothing was played', file=sys.stderr); sys.exit(2)
    t = time.localtime()
    if t.tm_wday < 5 and 12 <= t.tm_hour < 14 and os.environ.get('LAMP_PLAY_AT_LUNCH') != 'yes':
        print('quality_rows.py: not playing between 12:00 and 14:00 local, when the office rests', file=sys.stderr); sys.exit(2)
    # A TERM ends the run through the finally blocks (the stop scene's events tail and taps on the lamp, volume 45);
    # subprocess.run kills a child it is waiting for on the way out.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    kit = level_kit(os.environ.get('NIGHT_WAV') or os.path.expanduser('~/lamp-played/desk-kit-2/wav'), f'{OUT}/levelled-kit')
    level_cutin_lines(OUT)
    env = {**os.environ, 'PLAYED_OUT': OUT, 'NIGHT_WAV': kit}
    env.pop('CUTIN_ENV', None)   # cutin.sh never adds a drop-in or restarts lamp-voice
    env['LAMP_SSH'] = HERE + '/lamp_ssh'   # cutin.sh runs "$LAMP_SSH" as one program, never a command with options
    volume(45)
    # A failed part leaves its row "not measured"; the rest still run.
    cmds = [[sys.executable, HERE + '/night.py', label + '-night']]
    if os.path.exists(CUTIN_SCENE):
        cmds.append([HERE + '/cutin.sh', label + '-cutin', '1', '1'])
        env.update(CUTIN_SCENARIO=CUTIN_SCENE, CV_SCENARIO=HERE + '/scenario.py')
    else: print(f'quality_rows.py: no {CUTIN_SCENE}: cutin.sh not run (its closing stop would play on a timer)', file=sys.stderr)
    for cmd in cmds:
        rc = subprocess.run(cmd, env=env).returncode
        if rc: print(f'quality_rows.py: {os.path.basename(cmd[1] if cmd[0] == sys.executable else cmd[0])} exited {rc}', file=sys.stderr)
        volume(45)
    stop_scene(label, n)
    score(label)


if __name__ == '__main__':
    main()
