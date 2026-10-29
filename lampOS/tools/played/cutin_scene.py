#!/usr/bin/env python3
"""cutin.sh's scene runner for the core-verifier (quality_rows.py passes it as CUTIN_SCENARIO): scenario.py's argv.

A scene whose last step is cutin.sh's closing "Daniel|Lamp, stop. That's enough.|WAIT" runs scenario.py with the other
steps, then plays that stop as RULES-52e6 asks: after lamp-voice's journal shows "first answer audio" since the scene
began and that line is at least 1.5 s old, at Mac volume 90 (back to 45 after), the levelled clip from scenario.py's
cache; then WAIT s, and its "#PLAY_US <lamp monotonic us> Daniel: ..." line is appended to the scene's file, so cutin.sh
times the scene to it as before. No first answer audio in 30 s: no stop is played and nothing is appended. The lamp no
longer speaking when the stop is due (its last speaker_first_write not after its last turn_finished): no stop, and the
episode is void (exit 7, cutin.sh reports it not measured). Any other
scene is scenario.py unchanged. Env: CV_SCENARIO (scenario.py), PLAYED_OUT, LAMP_SAY_RATE as scenario.py.
  cutin_scene.py selftest     the argv split only (no lamp, nothing played)"""
import glob, hashlib, os, subprocess, sys, time

STOP = "Lamp, stop. That's enough."
REAL = os.environ.get('CV_SCENARIO', '')
OUT = os.environ.get('PLAYED_OUT') or os.path.expanduser('~/lamp-played')
RATE = os.environ.get('LAMP_SAY_RATE', '185')


def split(argv):
    """(name, steps without the closing stop, the stop's wait or None)."""
    name, steps = argv[0], argv[1:]
    last = steps[-1].split('|') if steps else []
    if len(last) == 3 and last[:2] == ['Daniel', STOP]: return name, steps[:-1], float(last[2])
    return name, steps, None


def lamp(command, timeout=10):
    ssh = os.path.join(os.path.dirname(REAL), 'lamp_ssh')
    return subprocess.run([ssh, command], capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL,
                          env=dict(os.environ, LAMP_SSH_TIMEOUT='5')).stdout.strip()


# scenario.py's read of the running session's events.
READ = r'''S=$(sudo -n ls -t /var/lib/lampOS/voice/sessions | head -1); sudo -n sh -c "cat /var/lib/lampOS/voice/sessions/$S/metadata/events-*.jsonl"'''


def speaking(rows=None):
    """The lamp is answering now: its last speaker_first_write is later than its last turn_finished."""
    import json
    if rows is None:
        rows = []
        for l in lamp(READ, timeout=20).splitlines():
            try: rows.append(json.loads(l))
            except ValueError: pass
    last = lambda k: max([r.get('at_us', 0) for r in rows if isinstance(r, dict) and r.get('kind') == k] + [0])
    return last('speaker_first_write') > last('turn_finished')


def stop_us(rows, now):
    """The lamp's clock at the stop: the newest event's at_us (the session's clock, as cutin.sh times the scene) plus the
    Mac time since it came, so no ssh is opened at the stop; and that event's age in s. (None, None) with no event."""
    last = max((r for r in rows if isinstance(r.get('at_us'), int)), key=lambda r: r['_mac'], default=None)
    if last is None: return None, None
    return last['at_us'] + int((now - last['_mac']) * 1e6), now - last['_mac']


def volume(v): subprocess.run(['/usr/bin/osascript', '-e', f'set volume output volume {v}'], check=False)


def main():
    if sys.argv[1:] == ['selftest']:
        assert split(['n', 'Daniel|ask|6', 'Samantha|Hmm|20', f'Daniel|{STOP}|8']) == ('n', ['Daniel|ask|6', 'Samantha|Hmm|20'], 8.0)
        assert split(['n', 'Daniel|ask|6', 'Samantha|Hmm|20']) == ('n', ['Daniel|ask|6', 'Samantha|Hmm|20'], None)
        assert split(['n', f'Samantha|{STOP}|8'])[2] is None and split(['n'])[2] is None
        w, f = {'kind': 'speaker_first_write'}, {'kind': 'turn_finished'}
        assert speaking([dict(f, at_us=1), dict(w, at_us=2)]) and not speaking([dict(w, at_us=2), dict(f, at_us=3)]) and not speaking([])
        assert stop_us([{'at_us': 5_000_000, '_mac': 100.0}, {'kind': 'x', '_mac': 101.0}], 102.0) == (7_000_000, 2.0)
        assert stop_us([], 1.0) == (None, None) and stop_us([{'kind': 'x', '_mac': 1.0}], 2.0) == (None, None)
        return print('cutin_scene.py selftest: ok')
    if not os.path.isfile(REAL): sys.exit('cutin_scene.py: CV_SCENARIO is not scenario.py')
    name, steps, wait = split(sys.argv[1:])
    if wait is None: os.execv(sys.executable, [sys.executable, REAL, *sys.argv[1:]])
    # Held streams, opened before the scene: a new ssh to a lamp takes 5-6 s to connect (52e6, 30 Oct), so one ssh per
    # check let every short answer end before the stop was due (6 of 8 scenes void, 30 Oct 04:56).
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import night
    from quality_rows import FirstAudio
    fa, ev = FirstAudio(night.SSH), night.Events()
    try:
        time.sleep(6)
        began = time.time()
        rc = subprocess.run([sys.executable, REAL, name, *steps]).returncode
        if rc: sys.exit(rc)
        first = fa.after(began, 30.0)
        if first is None: return print(f'cutin_scene.py: {name}: no first answer audio since the scene began; no stop played', file=sys.stderr)
        if time.time() < first + 1.5: time.sleep(first + 1.5 - time.time())
        ev.pump()
        if not speaking(ev.rows):
            print(f'cutin_scene.py: {name}: the lamp had stopped speaking before the stop was due; void, no stop played', file=sys.stderr)
            sys.exit(7)
        wav = f"{OUT}/prompts/generated/{hashlib.sha256(f'Daniel|{RATE}|{STOP}'.encode()).hexdigest()[:16]}.wav"
        if not os.path.exists(wav): sys.exit(f'cutin_scene.py: {wav} missing (quality_rows.py levels it); no stop played')
        at, age = stop_us(ev.rows, time.time())
        if at is None:
            print(f'cutin_scene.py: {name}: no event to time the stop; void, no stop played', file=sys.stderr)
            sys.exit(7)
        print(f'cutin_scene.py: {name}: stop at lamp {at} us, from an event {age:.2f} s old', file=sys.stderr)
    finally:
        fa.close(); ev.p.terminate()
    volume(90)
    try: subprocess.run(['/usr/bin/afplay', wav], check=True)
    finally: volume(45)
    time.sleep(wait)
    scenes = sorted(glob.glob(f'{OUT}/scenarios/*-{name}.txt'), key=os.path.getmtime)
    if not scenes: sys.exit(f'cutin_scene.py: {name}: no scene file; the stop line is not recorded')
    with open(scenes[-1], 'a') as f: f.write(f'#PLAY_US {at} Daniel: {STOP}\n')


main()
