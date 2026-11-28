"""Mac only: python3 -I mac/judge.py RES_DIR          (run.sh runs it; run it again on any saved or made-up dir)
          python3 -I mac/judge.py --leg1 RES_DIR   leg 1 only: exit 0 if it passes (run.sh's gate before the seed)
Reads one #8671 run: install.txt, leg1.txt, seed.txt, trigger.txt (lamp/*.sh KEY=VALUE lines), after-files/after/
(the recorder's status.txt, snap-*.txt, reset-journal.txt), snap-live.txt, putback.log, wait.txt.
Writes RES_DIR/row.md: line 1 the ROW line (the train bot reads line 1), the reset's journal lines next, then a
table a leg. Every journal string here is in mac/strings.tsv with its file:line at cf7dacac6d. Counts only."""
import re, sys
from pathlib import Path

LEG1_ONLY = sys.argv[1] == '--leg1'
D = Path(sys.argv[2] if LEG1_ONLY else sys.argv[1])
HEAD = 'cf7dacac6dd32a1e791919300e19912470ce8c68'
BIN = '/run/el-8671/lamp-setup'
HOMES_RE = re.compile(r"lamp-setup: reset: emptied (\d+) entries of the engines' homes")    # reset.rs:2182
LOGS_RE = re.compile(r'lamp-setup: reset: emptied (\d+) log entries')                       # reset.rs:2194
STARTED = 'lamp-setup: reset: started in place, no reboot: '                                # reset.rs:2060
LOG_FAIL = 'reset: a log could not be emptied: '                                            # logs.rs:146
HOMES_TEXT = "lamp-setup: reset: emptied N entries of the engines' homes"
LOGS_TEXT = 'lamp-setup: reset: emptied N log entries'
TEXT_LOGS = ('syslog', 'user.log', 'kern.log', 'auth.log')


def text(name):
    p = D / name
    return p.read_text(errors='replace') if p.exists() else ''


def kv_line(line):
    return {k: v for k, v in re.findall(r'([A-Z][A-Z0-9_]*)=(\S*)', line)}


def kv(name):
    o = {}
    for l in text(name).splitlines():
        o.update(kv_line(l))
    return o


def num(v):
    return int(v) if v is not None and re.fullmatch(r'-?\d+', v) else None


legs = []  # (leg, what, evidence, verdict)


def leg(n, what, verdict, ev):
    legs.append((n, what, ev.replace('|', '/'), verdict))


# ---------------------------------------------------------------- leg 1: the dry runs
def leg1():
    t = text('leg1.txt')
    dry = {}
    for l in t.splitlines():
        m = re.match(r'^DRY (\S+) exit=(\d+) lines=(\d+)(.*)$', l)
        if m:
            dry[m.group(1)] = dict(exit=int(m.group(2)), lines=int(m.group(3)), **{k: int(v) for k, v in re.findall(r'(\w+)=(\d+)', m.group(4))})
        m = re.match(r'^DRY_ERR (\S+) lines=(\d+) refused_line=(\d+)', l)
        if m:
            dry.setdefault(m.group(1), {})['refused'] = int(m.group(3))
    k = kv('leg1.txt')
    why = []
    for n in ('check-pr', 'check-stock', 'agents', 'homes', 'logs'):
        if dry.get(n, {}).get('exit') != 0:
            why.append('%s exit %s' % (n, dry.get(n, {}).get('exit', '?')))
    for n in ('homes-nodry', 'logs-nodry'):
        d = dry.get(n, {})
        if d.get('exit', 0) == 0 or d.get('refused', 0) < 1:
            why.append('%s not refused' % n)
    if k.get('HOMES_BAD_LINES') != '0': why.append('engine-homes lines not `empty|remove <home>`: %s' % k.get('HOMES_BAD_LINES'))
    if k.get('LOGS_BAD_LINES') != '0': why.append('forget-logs lines outside /var/log{,.hdd}: %s' % k.get('LOGS_BAD_LINES'))
    if k.get('HOMES_LISTED') != 'same': why.append('the homes listed are not the folder\'s entries')
    if k.get('UNCHANGED_HOMES') != 'yes' or k.get('UNCHANGED_LOGS') != 'yes': why.append('a dry run changed something')
    if 'LEG1-DONE' not in t: why.append('leg 1 did not finish')
    h, g = dry.get('homes', {}), dry.get('logs', {})
    ev = ('`check --plan` PR plan exit %s, stock plan exit %s; forget-agents %s lines; forget-engine-homes %s lines '
          '(empty %s, remove %s); forget-logs %s lines (truncate %s, remove %s, empty %s); both refused without '
          '--dry-run: %s; nothing changed: %s' % (
              dry.get('check-pr', {}).get('exit', '?'), dry.get('check-stock', {}).get('exit', '?'),
              dry.get('agents', {}).get('lines', '?'), h.get('lines', '?'), h.get('empty', 0), h.get('remove', 0),
              g.get('lines', '?'), g.get('truncate', 0), g.get('remove', 0), g.get('empty', 0),
              'yes' if not [w for w in why if 'refused' in w] else 'no',
              'yes' if k.get('UNCHANGED_HOMES') == 'yes' and k.get('UNCHANGED_LOGS') == 'yes' else 'no'))
    if why: ev += '. NOT OK: ' + '; '.join(why)
    return (not why and bool(dry)), ev


ok1, ev1 = leg1()
if LEG1_ONLY:
    print(('LEG 1 PASS: ' if ok1 else 'LEG 1 FAIL: ') + ev1)
    sys.exit(0 if ok1 else 1)
leg(1, 'read-only dry runs of the PR\'s lamp-setup', 'PASS' if ok1 else 'FAIL', ev1)

# ---------------------------------------------------------------- leg 2: the seed and the before
seed = text('seed.txt')
seeded = re.findall(r'^SEEDED (\S+) (\S+)', seed, re.M)
kinds = {}
for kd, p in seeded:
    kinds.setdefault(kd, []).append(p)
before_home = dict(re.findall(r'^HOME (\S+) (dir n=\d+ \S+)', seed, re.M))
before_kind = dict(re.findall(r'^KIND (\S+) (.*)$', seed, re.M))
before_logs = dict(re.findall(r'^LOGFILE (\S+) (\d+)', seed, re.M))


def counts(t, label):
    """{dir: kv} for TEXT/JOURNAL/LIVE lines in a text."""
    o = {}
    for l in t.splitlines():
        m = re.match(r'^(?:\S+ )?(TEXT|JOURNAL|LIVE) (.*)$', l)
        if m and (label is None or m.group(1) == label):
            dd = re.search(r'dir=(\S+)', m.group(2))
            o[(m.group(1), dd.group(1) if dd else '')] = {k.lower(): v for k, v in re.findall(r'(\w+)=(\S+)', m.group(2))}
    return o


cb = counts(seed, None)
hdd_text_before = num(cb.get(('TEXT', '/var/log.hdd'), {}).get('voice'))
hdd_j_before = num(cb.get(('JOURNAL', '/var/log.hdd/journal'), {}).get('voice'))
trig = kv('trigger.txt')
need = ['stray', 'marked', 'clips_new']
missing = [k for k in need if k not in kinds]
ok2 = 'SEED-OK' in seed and not missing
if trig.get('SEEDED_N') and trig.get('PRESENT_N') != trig.get('SEEDED_N'):
    ok2 = False
leg(2, 'kit markers seeded; the before counted', 'PASS' if ok2 else 'FAIL',
    'markers: %d engine home(s), stray %d, marked %d, clips (PR path) %d; present at the '
    'trigger %s of %s; lamp-voice lines in /var/log.hdd before: text %s, journal %s; history-marks.json %s, '
    'history-forget.json %s (read, not seeded)%s' % (
        len(kinds.get('home', [])), len(kinds.get('stray', [])), len(kinds.get('marked', [])),
        len(kinds.get('clips_new', [])), trig.get('PRESENT_N', '?'),
        trig.get('SEEDED_N', '?'), hdd_text_before, hdd_j_before,
        before_kind.get('/var/lib/lampOS/voice/history-marks.json', '?').split(' ')[0],
        before_kind.get('/var/lib/lampOS/voice/history-forget.json', '?').split(' ')[0],
        ('. NOT OK: no ' + ', '.join(missing)) if missing else ''))

# ---------------------------------------------------------------- leg 2b: the PR's voice removes the old folder
OLD_RE = re.compile(r'^OLD_REMOVED_COUNT=(\d+)$', re.M)   # ring_clips.rs:468, rendered by oldclips.sh
oc = text('oldclips.txt'); o = kv('oldclips.txt')
if not oc:
    leg('2b', 'the PR\'s voice removes the old clips folder at its start', 'VOID' if not ok1 else 'FAIL', 'not run')
else:
    nn = [int(x) for x in OLD_RE.findall(oc)]
    put = num(o.get('KIT_OLD_FILES')); there = num(o.get('OLD_FILES_BEFORE'))
    why = []
    if num(o.get('OLD_REMOVED_N')) != 1 or len(nn) != 1: why.append('removed line x%s' % o.get('OLD_REMOVED_N', '?'))
    elif nn[0] != there: why.append('N=%d, %s file(s) were there' % (nn[0], there))
    if num(o.get('OLD_KIT_PRESENT')) != put: why.append('%s of %s kit file(s) there at the restart' % (o.get('OLD_KIT_PRESENT'), put))
    if o.get('OLD_LEFT_N') != '0': why.append('a "was left" line')
    if o.get('OLD_AFTER') != 'absent': why.append('the old folder is still there')
    if o.get('SHA_OK') != 'yes' or o.get('SWITCH') != 'off': why.append('the voice was not the PR\'s with the switch off')
    if o.get('MUTED') != 'no' or o.get('STOCK_MUTED') != 'no': why.append('muted by the restart guard')
    if o.get('STOCK_BACK') != 'yes': why.append('the stock voice is not back')
    if o.get('NRESTARTS_AFTER') != o.get('NRESTARTS_BEFORE'): why.append('NRestarts %s, before %s' % (o.get('NRESTARTS_AFTER'), o.get('NRESTARTS_BEFORE')))
    leg('2b', 'the PR\'s voice removes the old clips folder at its start', 'FAIL' if why else 'PASS',
        '%s kit clip file(s) put in /var/lib/lampOS/voice/ring-clips (%s other entr(ies)); `lamp-voice: the old offline '
        'ring clips folder was removed (N files)` x%s with N=%s; "was left" x%s; the old folder %s after; stock voice back: '
        '%s. For information only: the new folder held %s entr(ies) before and %s after (`the offline ring clips were '
        'cleared` x%s): with `LAMP_OFFLINE_RINGS=off` #8933, in the PR\'s base, clears it at the consumer\'s start by '
        'design, so this leg puts nothing there and the reset\'s marker is seeded after it%s' % (
            put, o.get('OLD_OTHER_BEFORE', '?'), o.get('OLD_REMOVED_N', '?'), ','.join(map(str, nn)) or '-',
            o.get('OLD_LEFT_N', '?'), o.get('OLD_AFTER', '?').split(' ')[0], o.get('STOCK_BACK', '?'),
            o.get('NEW_ENTRIES_BEFORE', '?'), o.get('NEW_ENTRIES_AFTER', '?'), o.get('NEW_CLEARED_N', '?'),
            ('. NOT OK: ' + '; '.join(why)) if why else ''))

# ---------------------------------------------------------------- the after files
A = 'after-files/after/'
status = text(A + 'status.txt')
rj = text(A + 'reset-journal.txt')
snaps = []
for p in sorted((D / 'after-files' / 'after').glob('snap-*.txt')) if (D / 'after-files' / 'after').exists() else []:
    snaps.append((p.stem[5:], p.read_text(errors='replace')))
if (D / 'snap-live.txt').exists():
    snaps.append(('live', text('snap-live.txt')))
snaps = [(n, t) for n, t in snaps if 'SNAP-DONE' in t]
order = lambda n: (0, int(n)) if n.isdigit() else (1, {'repaired': 0, 'last': 1, 'live': 2}.get(n, 3))
snaps.sort(key=lambda x: order(x[0]))
first = snaps[0][1] if snaps else ''
latest = snaps[-1][1] if snaps else ''
triggered = 'TRIGGERED' in text('trigger.txt')
unread = triggered and not snaps

# ---------------------------------------------------------------- leg 3: the reset
hm = [int(x) for x in HOMES_RE.findall(rj)]
lm = [int(x) for x in LOGS_RE.findall(rj)]
f0 = kv(A + 'snap-0.txt') if (D / (A + 'snap-0.txt')).exists() else kv_line(' '.join(first.splitlines()))
started = max(rj.count(STARTED), num(f0.get('LIVE_STARTED_LINE') or '0') or 0)
logfail = rj.count(LOG_FAIL)
seen = re.search(r'^WIPE_SEEN exe=(\S*)', status, re.M)
end = re.search(r'^WIPE_END state=(\S+) load=(\S+) result=(\S+)', status, re.M)
if not triggered:
    leg(3, 'the real reset (the product\'s reset-now, the PR\'s lamp-setup)', 'VOID', 'not triggered (%s)' % (
        'CONFIRM_RESET not given' if 'NO RESET' in text('go.txt') else 'stopped before leg 3'))
elif unread:
    leg(3, 'the real reset (the product\'s reset-now, the PR\'s lamp-setup)', 'FAIL',
        'triggered at %s; the lamp did not come back in the wait (%s): the recorder\'s files are unread' % (
            trig.get('RESET_WALL_US', '?'), (text('wait.txt').strip() or 'no wait.txt')[:160]))
else:
    why = []
    if not seen or seen.group(1) != BIN: why.append('the wipe unit\'s exe was %s, not %s' % (seen.group(1) if seen else 'never seen', BIN))
    if not end: why.append('no WIPE_END (%s)' % (re.findall(r'^(WIPE_NEVER|WIPE_TIMEOUT)\S*', status, re.M) or ['?'])[0])
    elif not end.group(3).startswith('success'): why.append('wipe result %s' % end.group(3))
    homes_need = len(kinds.get('home', [])) + len(kinds.get('stray', []))
    if len(hm) != 1 or hm[0] < homes_need: why.append('engines\' homes line %s, needs one with N >= %d' % (hm or 'none', homes_need))
    if len(lm) != 1: why.append('logs line x%d' % len(lm))
    if started < 1: why.append('no "started in place" line')
    if f0.get('BOOT_ID') and f0.get('BOOT_BEFORE') and f0['BOOT_ID'] != f0['BOOT_BEFORE']: why.append('the lamp rebooted (the reset\'s fallback)')
    if f0.get('SETUP_MARKER') != 'absent': why.append('setup marker %s' % f0.get('SETUP_MARKER'))
    if f0.get('RESET_JOURNAL') != 'absent': why.append('reset journal %s' % f0.get('RESET_JOURNAL'))
    if 'UNIT lamp-setup=active' not in first: why.append('lamp-setup (setup mode) not active at snap 0')
    leg(3, 'the real reset (the product\'s reset-now, the PR\'s lamp-setup)', 'FAIL' if why else 'PASS',
        'wipe unit exe %s, result %s; `%s` x%d (N=%s, %d seeded); `%s` x%d (N=%s); `%s` x%d; log failures said: %d; '
        'boot id unchanged: %s; setup marker %s, setup %s%s' % (
            seen.group(1) if seen else '?', end.group(3) if end else '?', HOMES_TEXT, len(hm), ','.join(map(str, hm)) or '-',
            homes_need, LOGS_TEXT, len(lm), ','.join(map(str, lm)) or '-', STARTED.strip(), started, logfail,
            'yes' if f0.get('BOOT_ID') == f0.get('BOOT_BEFORE') else 'no', f0.get('SETUP_MARKER', '?'),
            'serving' if 'UNIT lamp-setup=active' in first else '?', ('. NOT OK: ' + '; '.join(why)) if why else ''))


# ---------------------------------------------------------------- leg 4a: the markers gone, the kept kept
def snap_marks(t):
    return re.findall(r'^MARK (\S+) (present|absent) (\S+)', t, re.M)


if triggered and not unread:
    why = []
    for t_name, t in ((snaps[0][0], first), (snaps[-1][0], latest)):
        for kd, st, p in snap_marks(t):
            if kd != 'clips_old' and st == 'present' and not (t_name == 'live' and 'removed %s %s' % (kd, p) in text('putback.log')):
                why.append('%s marker still there at snap %s' % (kd, t_name))
    sh = dict(re.findall(r'^HOME (\S+) (.*)$', first, re.M))
    for h, b in before_home.items():
        a = sh.get(h, 'absent')
        bo, ao = b.split(' ')[-1], a.split(' ')[-1]
        if not a.startswith('dir n=0 ') or ao != bo:
            why.append('home %s: %s (before %s)' % (h.rsplit('/', 1)[-1], a, b))
    ak = dict(re.findall(r'^KIND (\S+) (.*)$', first, re.M))
    if not ak.get('/var/lib/lampOS/voice/marked', '').startswith('dir n=0'): why.append('marked: %s' % ak.get('/var/lib/lampOS/voice/marked'))
    if ak.get('/var/lib/lampOS/voice/history-marks.json', 'absent') != 'absent': why.append('history-marks.json still there')
    if ak.get('/var/lib/lampOS/voice/schedule-delivery/ring-clips', 'absent') != 'absent': why.append('the PR\'s clips folder still there')
    kept_bad, kept_n = [], 0
    for p, b in before_kind.items():
        if p in ('/var/lib/lampOS/voice/history-marks.json', '/var/lib/lampOS/voice/marked',
                 '/var/lib/lampOS/voice/schedule-delivery/ring-clips', '/var/lib/lampOS/voice/ring-clips') or b == 'absent':
            continue
        kept_n += 1
        if ak.get(p, 'absent').split(' ')[0] != b.split(' ')[0]:
            kept_bad.append(p)
    if kept_bad: why.append('kept paths gone: ' + ', '.join(kept_bad))
    al = dict(re.findall(r'^LOGFILE (\S+) (\d+)', first, re.M))
    for f, ino in before_logs.items():
        live = f.rsplit('/', 1)[-1] in TEXT_LOGS
        if live and al.get(f) != ino: why.append('%s: inode %s, before %s (not truncated in place)' % (f, al.get(f, 'gone'), ino))
        if not live and f in al: why.append('%s: rotated copy still there' % f)
    leg('4a', 'every seeded marker gone; the KEPT paths kept', 'FAIL' if why else 'PASS',
        '%d marker(s) of the PR\'s paths absent at snap %s and at %s; %d home(s) empty, owner:mode kept; marked emptied, '
        'history-marks.json %s, the PR\'s clips folder gone with schedule-delivery\'s contents; %d kept path(s) read '
        'still there; %d text log(s) kept in place (same inode), %d rotated cop(ies) removed%s' % (
            sum(1 for kd, st, p in snap_marks(first) if kd != 'clips_old' and st == 'absent'), snaps[0][0], snaps[-1][0],
            len(before_home), ak.get('/var/lib/lampOS/voice/history-marks.json', '?').split(' ')[0], kept_n - len(kept_bad),
            sum(1 for f in before_logs if f.rsplit('/', 1)[-1] in TEXT_LOGS),
            sum(1 for f in before_logs if f.rsplit('/', 1)[-1] not in TEXT_LOGS),
            ('. NOT OK: ' + '; '.join(why)) if why else ''))

# ---------------------------------------------------------------- leg 4b: the voice's journal lines again
if triggered and not unread:
    vs = [(n, num(kv_line(' '.join(l for l in t.splitlines() if l.startswith('VOICE_SINCE_END'))).get('VOICE_SINCE_END'))) for n, t in snaps]
    got = [(n, v) for n, v in vs if v]
    pj = num(kv_line(' '.join(l for l in latest.splitlines() if l.startswith('PROBE_'))).get('PROBE_JOURNAL'))
    ps = num(kv_line(' '.join(l for l in latest.splitlines() if l.startswith('PROBE_'))).get('PROBE_SYSLOG'))
    active = 'VOICE_ACTIVE' in status or any('UNIT lamp-voice=active' in t for _, t in snaps)
    if got:
        leg('4b', 'the voice\'s journal lines appear again', 'PASS', '%d lamp-voice line(s) in the journal since the wipe ended (snap %s); the kit\'s probe line: journal %s, /var/log/syslog %s' % (got[-1][1], got[-1][0], pj, ps))
    else:
        leg('4b', 'the voice\'s journal lines appear again', 'OWED' if not active else 'FAIL',
            ('lamp-voice has not run since the reset: the lamp is in setup mode, waiting for the lead\'s re-pairing (not '
             'onboarded by this kit). Meanwhile the logs still write: the kit\'s probe line in the journal %s, in the '
             'truncated /var/log/syslog %s' if not active else 'lamp-voice ran again but no line of it in the journal; probe journal %s, syslog %s') % (pj, ps))

# ---------------------------------------------------------------- leg 4c: no lamp-voice line from before the reset
if triggered and not unread:
    bad, rows = [], []
    for n, t in snaps:
        c = counts(t, None)
        th = num(c.get(('TEXT', '/var/log.hdd'), {}).get('voice_before'))
        jh = num(c.get(('JOURNAL', '/var/log.hdd/journal'), {}).get('voice_before'))
        tr = num(c.get(('TEXT', '/var/log'), {}).get('voice_before'))
        lv = c.get(('LIVE', ''), {}).get('voice_before', '?')
        s = kv_line(' '.join(l for l in t.splitlines() if l.startswith('SNAP=')))
        rows.append('snap %s (+%ss): hdd text %s, hdd journal %s, RAM text %s, live journal %s' % (n, s.get('SINCE_RESET_S', '?'), th, jh, tr, lv))
        if th != 0 or jh != 0: bad.append(n)
    hn = kv_line(' '.join(l for l in latest.splitlines() if l.startswith('HDD_NEWEST')))
    synced = num(hn.get('HDD_NEWEST_MTIME')) is not None and num(hn.get('RESET_S')) is not None and num(hn['HDD_NEWEST_MTIME']) > num(hn['RESET_S'])
    void = (hdd_text_before or 0) == 0 and (hdd_j_before or 0) == 0
    leg('4c', '/var/log.hdd: no lamp-voice line from before the reset (text logs and journal)',
        'VOID' if void else ('FAIL' if bad else 'PASS'),
        ('before: text %s, journal %s (nothing to forget)' % (hdd_text_before, hdd_j_before) if void else
         'before: text %s, journal %s; ' % (hdd_text_before, hdd_j_before)) + '; '.join(rows) +
        '; a write into /var/log.hdd after the reset (a ramlog sync): %s' % ('yes' if synced else 'not yet'))

# ---------------------------------------------------------------- the row
putback_ok = 'PUTBACK-OK' in text('putback.log')
gates = [v for _, _, _, v in legs if v != 'NOTE']
if all(v == 'PASS' for v in gates) and putback_ok and triggered:
    verdict = 'PASS'
elif all(v in ('PASS', 'OWED') for v in gates) and putback_ok and triggered:
    verdict = 'PENDING'
else:
    verdict = 'FAIL'
build = (re.findall(r'^BUILD=(\d*)', text('install.txt'), re.M) or [''])[0] or '<build?>'
row = ['ROW #8671 %s on lamp-75c9 build %s: %s' % (HEAD, build, verdict),
       'Journal: `%s` x%d (N=%s); `%s` x%d (N=%s); `%s` x%d.' % (HOMES_TEXT, len(hm), ','.join(map(str, hm)) or '-',
                                                              LOGS_TEXT, len(lm), ','.join(map(str, lm)) or '-',
                                                              STARTED.strip(), started if triggered else 0), '']
row += ['El\'s #8671 row on lamp-75c9, a bench lamp whose person said yes to a real reset: the PR\'s CI ARM64 lamp-setup '
        '(%s) and the PR\'s product plan (%s), from a /run tmpfs, run the product\'s own way: the plan\'s `detach` '
        '(`systemd-run --no-block --collect --unit=lamp-setup-wipe`), then `reset-now --plan PLAN --enable`, as '
        'watch-reset and the factory\'s step 4 do. Kit marker files only (no person data); counts only, no log line '
        'read out. The lamp stays reset and is not onboarded (the lead re-pairs it).' % (
            (re.findall(r'BIN_SHA=([0-9a-f]{12})', text('install.txt')) or ['sha ?'])[0] + '...',
            (re.findall(r'PLAN_SHA=([0-9a-f]{12})', text('install.txt')) or ['sha ?'])[0] + '...'), '',
        '| Leg | What | Evidence | Verdict |', '|---|---|---|---|']
for n, what, ev, v in legs:
    row.append('| %s | %s | %s | %s |' % (n, what, ev, v))
row += ['', 'Put-back: %s.' % ('PUTBACK-OK (the lamp left reset, in setup mode; kit markers, the tmpfs and the dead-man '
                               'gone; read back twice)' if putback_ok else 'NOT OK or not run: see putback.log')]
(D / 'row.md').write_text('\n'.join(row) + '\n')
print('\n'.join(row))
