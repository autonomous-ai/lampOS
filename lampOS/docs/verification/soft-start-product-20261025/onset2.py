# onset2.py DIR (24 Oct 22:3x, first builder): for a recording made by pause-rec.sh, each input candidate's lead-in
# (how long its kept sound began before the speech it triggered on, negative inside the speech), the speech segment's
# first 100 ms levels, and each voice/heard text, in time order on the voice's clock.
import base64, json, math, sys
d = sys.argv[1]
blocks = []
for line in open(f'{d}/rec/mic2_audio.jsonl'):
    o = json.loads(line)
    if 'payload_b64' not in o: continue
    p = base64.b64decode(o['payload_b64']); pcm = p[p.index(b'\n') + 1:]
    at = o['header']['at_us']; n = len(pcm) // 2
    s = [int.from_bytes(pcm[i*2:i*2+2], 'little', signed=True) for i in range(n)]
    for k, part in enumerate((s[:n//2], s[n//2:])):
        pw = sum((v / 32768) ** 2 for v in part) / max(1, len(part))
        blocks.append((at - (1 - k) * 10_000, 10 * math.log10(pw) if pw > 0 else -120))
blocks.sort(); t0 = blocks[0][0]; t1 = blocks[-1][0]
def onset_before(at):
    # the start of the sound run (blocks over -50 dBFS, gaps under 100 ms) that holds `at`
    i = max(j for j, (b, _) in enumerate(blocks) if b <= at)
    gap = 0; start = i
    while i > 0:
        i -= 1
        if blocks[i][1] > -50: start = i; gap = 0
        else:
            gap += 1
            if gap >= 10: break
    return blocks[start][0]
rows = []
for line in open(f'{d}/events.jsonl'):
    try: e = json.loads(line)
    except Exception: continue
    if e.get('kind') == 'input_candidate':
        c = e['candidate']; f, t = c['first_read_at_us'], c['trigger_read_at_us']
        if t0 <= f <= t1:
            on = onset_before(t)
            lv = ' '.join(f'{db:.0f}' for b, db in blocks if on <= b < on + 100_000)
            rows.append((f, f'candidate: kept from {(f-t0)/1e6:7.3f}, trigger {(t-t0)/1e6:7.3f}, speech onset {(on-t0)/1e6:7.3f}: lead-in {(on-f)/1000:+5.0f} ms, trigger {(t-on)/1000:.0f} ms after onset; first 100 ms dBFS {lv}'))
    elif e.get('kind') == 'input_candidate_rejected' and t0 <= e['at_us'] <= t1:
        rows.append((e['at_us'], f'candidate rejected {json.dumps(e.get("reason"))}'))
for line in open(f'{d}/rec/voice_heard.jsonl'):
    o = json.loads(line)
    if 'payload_b64' in o:
        h = json.loads(base64.b64decode(o['payload_b64'])); rows.append((o['header']['at_us'], f'    heard (request {h.get("request")}): {h.get("text")!r}'))
for at, text in sorted(rows): print(f'{(at-t0)/1e6:8.3f} {text}')
