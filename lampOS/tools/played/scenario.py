#!/usr/bin/env python3
"""Play a scripted scene to the lamp from this Mac's speakers and print what the lamp heard, said and when.

usage: scenario.py NAME STEP [STEP ...]
  STEP is  VOICE|TEXT|WAIT_AFTER_SECONDS      e.g.  "Daniel|Lamp, how are you today?|9"
           or  pause|SECONDS
           or  wav|PATH|WAIT_AFTER_SECONDS|LABEL   a wav played as it is, its PLAY line "wav: LABEL"
Speech is synthesised once with macOS `say`, kept in $PLAYED_OUT/prompts/generated/ and reused. A played voice is a smoke
test: the lamp's voice microphone hears replayed speech worse than a person. Output is also saved under
$PLAYED_OUT/scenarios/ (PLAYED_OUT defaults to ~/lamp-played). The lamp is reached with lamp_ssh beside this file
(LAMP_HOST, LAMP_SSH_KEY, LAMP_USER: see README.md). The lamp's always-on service must be running; nothing on the
lamp is changed."""
import hashlib, json, os, subprocess, sys, time
HOME = os.path.expanduser('~'); V = os.environ.get('PLAYED_OUT') or HOME + '/lamp-played'
# The lamp, through the kit's lamp_ssh (its settings come from the environment), with a 5 s connect time unless
# LAMP_SSH_TIMEOUT says otherwise.
SSH = [os.path.join(os.path.dirname(os.path.abspath(__file__)), 'lamp_ssh')]
SSH_ENV = dict(os.environ, LAMP_SSH_TIMEOUT=os.environ.get('LAMP_SSH_TIMEOUT') or '5')
RATE = os.environ.get('LAMP_SAY_RATE', '185')

def wav_for(voice, text):
    key = hashlib.sha256(f'{voice}|{RATE}|{text}'.encode()).hexdigest()[:16]
    path = f'{V}/prompts/generated/{key}.wav'
    if not os.path.exists(path):
        subprocess.run(['/usr/bin/say', '-v', voice, '-r', RATE, '-o', path, '--data-format=LEI16@16000', text], check=True)
        with open(f'{V}/prompts/generated/index.tsv', 'a') as f: f.write(f'{key}\t{voice}\t{RATE}\t{text}\n')
    return path

def lamp(command, timeout=25):
    return subprocess.run(SSH + [command], capture_output=True, text=True, timeout=timeout, env=SSH_ENV).stdout

READ = r'''S=$(sudo -n ls -t /var/lib/lampOS/voice/sessions | head -1); sudo -n sh -c "cat /var/lib/lampOS/voice/sessions/$S/metadata/events-*.jsonl"'''
KEEP = ('listening_ready', 'input_candidate', 'input_admitted', 'local_endpoint', 'transcript', 'speaker_first_write', 'speech_final_sample_retired', 'turn_finished',
        'provider_turn_failed', 'provider_unavailable', 'provider_ready', 'input_candidate_rejected', 'duplex_yield', 'duplex_commit', 'duplex_resume', 'barrier_assumed',
        'provider_ownership_assumed', 'provider_turn_complete', 'provider_terminal', 'service_provider_ended', 'provider_first_audio')

def main():
    if not os.environ.get('LAMP_HOST'):
        sys.exit("scenario.py: LAMP_HOST is not set. Set it to your own lamp's address (see README.md). Nothing was played.")
    os.makedirs(V + '/prompts/generated', exist_ok=True); os.makedirs(V + '/scenarios', exist_ok=True)
    # The office naps from noon to two (the owner, 2026-10-15): no played audio then.
    hour = time.localtime().tm_hour
    # Weekdays only: on weekends nobody is in the office (the owner, 16 Oct 18:2x).
    if time.localtime().tm_wday < 5 and 12 <= hour < 14 and os.environ.get("LAMP_PLAY_AT_LUNCH") != "yes":
        sys.exit("scenario.py: not playing between 12:00 and 14:00 local, when the office rests")
    name, steps = sys.argv[1], sys.argv[2:]
    start = int(lamp("python3 -c 'import time; print(time.monotonic_ns()//1000)'").strip())
    played = []
    for step in steps:
        parts = step.split('|')
        if parts[0] == 'pause':
            time.sleep(float(parts[1])); continue
        if parts[0] == 'wav':
            voice, path, wait = 'wav', parts[1], float(parts[2])
            text = parts[3] if len(parts) > 3 else os.path.basename(path)
        else:
            voice, text, wait = parts[0], parts[1], float(parts[2])
            path = wav_for(voice, text)
        at = int(lamp("python3 -c 'import time; print(time.monotonic_ns()//1000)'").strip())
        subprocess.run(['/usr/bin/afplay', path], check=True)
        done = time.monotonic()
        played.append((at, voice, text))
        time.sleep(wait)
    rows = []
    # Turns whose audio reached the speaker, from the whole session: a turn cut before its first write (the person went
    # on talking, 17 Oct 16:2x) has a transcript of words never heard, which read as the lamp saying its opening twice.
    played_turns = set()
    for line in lamp(READ, timeout=40).splitlines():
        try: r = json.loads(line)
        except ValueError: continue
        if r.get('kind') == 'speaker_first_write': played_turns.add((r.get('owner') or {}).get('turn'))
        if r.get('kind') in KEEP and r.get('at_us', 0) >= start: rows.append(r)
    rows.sort(key=lambda r: r.get('service_sequence', 0))
    out = [f'scenario {name}  {time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}  say rate {RATE}']
    events = [(at, 'PLAY ', f'{voice}: {text}') for at, voice, text in played]
    ep = None; waits = []
    for r in rows:
        k = r['kind']; at = r['at_us']
        if k == 'local_endpoint':
            ep = at; how = r.get('endpoint') or {}
            why = (' (%s%s)' % (how.get('basis'), '' if how.get('probability') is None else ', p=%.2f' % how['probability'])) if how else ''
            events.append((at, '     ', '[end of speech decided after %s ms of quiet%s]' % (r.get('includes_silence_wait_ms'), why)))
        elif k == 'transcript':
            lamp_said = r.get('turn') is not None and r.get('role', 'output') != 'input'
            who = ('LAMP ' if r.get('turn') in played_turns else 'UNPLD') if lamp_said else 'HEARD'
            events.append((at, who, (r.get('text') or '').strip()))
        elif k == 'speaker_first_write':
            if ep is not None: waits.append((at - ep) / 1e6)
            events.append((at, '     ', '[lamp starts speaking%s]' % (' %.2f s after that decision' % ((at - ep) / 1e6) if ep is not None else '')))
        elif k == 'turn_finished': events.append((at, '     ', '[turn finished: %s]' % r.get('outcome')))
        elif k in ('provider_turn_failed', 'provider_unavailable', 'input_candidate_rejected'): events.append((at, '     ', '[%s %s]' % (k, json.dumps(r.get('reason') or r.get('link', {}).get('state', {}))[:90])))
        elif k in ('input_admitted', 'duplex_yield', 'duplex_commit', 'duplex_resume', 'barrier_assumed', 'provider_ready'): events.append((at, '     ', '[%s]' % k))
    events.sort(key=lambda e: e[0])
    for at, who, text in events: out.append('%7.2fs %s %s' % ((at - start) / 1e6, who, text))
    if waits: out.append('lamp started speaking %s s after its end-of-speech decision (%d replies)' % (', '.join('%.2f' % w for w in waits), len(waits)))
    # The lamp's clock just before each clip was started here: the clip itself begins 0.1 to 0.3 s later.
    for at, voice, said in played: out.append('#PLAY_US %d %s: %s' % (at, voice, said))
    text = '\n'.join(out); print(text)
    with open(f'{V}/scenarios/{time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())}-{name}.txt', 'w') as f: f.write(text + '\n')

main()
