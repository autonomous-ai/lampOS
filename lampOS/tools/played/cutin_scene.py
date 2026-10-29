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


def volume(v): subprocess.run(['/usr/bin/osascript', '-e', f'set volume output volume {v}'], check=False)


def main():
    if sys.argv[1:] == ['selftest']:
        assert split(['n', 'Daniel|ask|6', 'Samantha|Hmm|20', f'Daniel|{STOP}|8']) == ('n', ['Daniel|ask|6', 'Samantha|Hmm|20'], 8.0)
        assert split(['n', 'Daniel|ask|6', 'Samantha|Hmm|20']) == ('n', ['Daniel|ask|6', 'Samantha|Hmm|20'], None)
        assert split(['n', f'Samantha|{STOP}|8'])[2] is None and split(['n'])[2] is None
        w, f = {'kind': 'speaker_first_write'}, {'kind': 'turn_finished'}
        assert speaking([dict(f, at_us=1), dict(w, at_us=2)]) and not speaking([dict(w, at_us=2), dict(f, at_us=3)]) and not speaking([])
        return print('cutin_scene.py selftest: ok')
    if not os.path.isfile(REAL): sys.exit('cutin_scene.py: CV_SCENARIO is not scenario.py')
    name, steps, wait = split(sys.argv[1:])
    if wait is None: os.execv(sys.executable, [sys.executable, REAL, *sys.argv[1:]])
    since = lamp('date +%s')
    rc = subprocess.run([sys.executable, REAL, name, *steps]).returncode
    if rc: sys.exit(rc)
    end = time.time() + 30
    while True:
        got = lamp(f"journalctl -u lamp-voice --since @{since} --no-pager -o short-unix | grep 'first answer audio' | tail -n 1")
        if got: break
        if time.time() > end: return print(f'cutin_scene.py: {name}: no first answer audio since the scene began; no stop played', file=sys.stderr)
        time.sleep(0.5)
    age = float(lamp('date +%s.%N') or 0) - float(got.split()[0])
    if age < 1.5: time.sleep(1.5 - age)
    if not speaking():
        print(f'cutin_scene.py: {name}: the lamp had stopped speaking before the stop was due; void, no stop played', file=sys.stderr)
        sys.exit(7)
    wav = f"{OUT}/prompts/generated/{hashlib.sha256(f'Daniel|{RATE}|{STOP}'.encode()).hexdigest()[:16]}.wav"
    if not os.path.exists(wav): sys.exit(f'cutin_scene.py: {wav} missing (quality_rows.py levels it); no stop played')
    at = lamp("python3 -c 'import time; print(time.monotonic_ns()//1000)'")
    volume(90)
    try: subprocess.run(['/usr/bin/afplay', wav], check=True)
    finally: volume(45)
    time.sleep(wait)
    scenes = sorted(glob.glob(f'{OUT}/scenarios/*-{name}.txt'), key=os.path.getmtime)
    if not scenes or not at.isdigit(): sys.exit(f'cutin_scene.py: {name}: no scene file or lamp time; the stop line is not recorded')
    with open(scenes[-1], 'a') as f: f.write(f'#PLAY_US {at} Daniel: {STOP}\n')


main()
