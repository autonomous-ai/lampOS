"""Mac only. python3 -I mac/fake.py RESULTS_DIR           writes made-up run dirs (fake-*) for the judge;
          python3 -I mac/fake.py --verify RESULTS_DIR  checks the judge's row.md in each against what it should say.
fake-pass passes every leg; fake-pending has no re-pair yet
(the voice's lines OWED); each fake-fail-* breaks one leg one way it could really fail on the lamp; fake-void-4c had
nothing in /var/log.hdd to forget; fake-unreachable never came back; fake-noreset stopped before leg 3.
fake-homes-made is a SEED_HOMES=1 run on a lamp with no /var/lib/lampos-agents (the kit made it and the homes claude
and codex: PASS, no limit line); fake-fail-4a-home-removed is that run with a home removed instead of emptied."""
import shutil, sys
from pathlib import Path

HEAD = 'cf7dacac6dd32a1e791919300e19912470ce8c68'
VARIANTS = {
    'pass': ('PASS', {'1': 'PASS', '2': 'PASS', '2b': 'PASS', '3': 'PASS', '4a': 'PASS', '4b': 'PASS', '4c': 'PASS'}),
    'fail-2b': ('FAIL', {'2b': 'FAIL', '3': 'PASS'}),
    'fail-2b-left': ('FAIL', {'2b': 'FAIL'}),
    'new-cleared': ('PENDING', {'2b': 'PASS'}),
    'pending': ('PENDING', {'3': 'PASS', '4a': 'PASS', '4b': 'OWED', '4c': 'PASS'}),
    'fail-1': ('FAIL', {'1': 'FAIL'}),
    'fail-3': ('FAIL', {'3': 'FAIL'}),
    'fail-3exe': ('FAIL', {'3': 'FAIL'}),
    'fail-3reboot': ('FAIL', {'3': 'FAIL'}),
    'fail-4a': ('FAIL', {'4a': 'FAIL'}),
    'fail-4a-inode': ('FAIL', {'4a': 'FAIL'}),
    'fail-4a-kept': ('FAIL', {'4a': 'FAIL'}),
    'fail-4c': ('FAIL', {'4c': 'FAIL'}),
    'void-4c': ('FAIL', {'4c': 'VOID'}),
    'unreachable': ('FAIL', {'3': 'FAIL'}),
    'noreset': ('FAIL', {'1': 'PASS', '2': 'PASS', '3': 'VOID'}),
    'homes-made': ('PASS', {'1': 'PASS', '2': 'PASS', '2b': 'PASS', '3': 'PASS', '4a': 'PASS', '4b': 'PASS', '4c': 'PASS'}),
    'fail-4a-home-removed': ('FAIL', {'2': 'PASS', '3': 'PASS', '4a': 'FAIL'}),
}
MADE = ('homes-made', 'fail-4a-home-removed')
PASSLIKE = ('pass', 'homes-made')
H = '/var/lib/lampos-agents'
HOMES = [H + '/claudecode', H + '/codex']
M = 'el8671-kit-marker'
LOGS = ['/var/log/syslog 101', '/var/log/user.log 102', '/var/log.hdd/syslog 201', '/var/log.hdd/syslog.1 202',
        '/var/log.hdd/syslog.2.gz 203', '/var/log.hdd/auth.log 204']
KEPT = ['/var/lib/lampOS/voice/day-tally dir n=3 root:root:700', '/var/lib/lampOS/voice/tts-cache dir n=40 root:root:700',
        '/var/lib/lampOS/voice/history-forget.json absent', '/var/lib/lampos-agents dir n=3 root:root:755',
        '/var/lib/lampOS/voice/schedule-delivery dir n=2 root:root:700', '/var/lib/lampOS/voice/history-marks.json absent',
        '/var/lib/lampOS/voice/marked dir n=1 root:root:700', '/var/lib/lampOS/voice/ring-clips dir n=5 root:root:700',
        '/var/lib/lampOS/voice/schedule-delivery/ring-clips dir n=1 root:root:700']
T = 1795000000
KS = 'e18671e18671e18671e18671e18671e1'


def counts(text_before, j_before, hdd_t=0, hdd_j=0, ram=0):
    return ['TEXT dir=/var/log files=2 lines=900 voice=%d voice_before=%d voice_after=0 voice_unparsed=0' % (ram, ram),
            'TEXT dir=/var/log.hdd files=4 lines=5000 voice=%d voice_before=%d voice_after=0 voice_unparsed=0' % (text_before or hdd_t, hdd_t),
            'JOURNAL dir=/var/log.hdd/journal files=3 voice=%d voice_before=%d voice_after=0 unread=0' % (j_before or hdd_j, hdd_j),
            'LIVE voice_before=0 voice_after=0']


def homes_of(v):
    return [H + '/claude', H + '/codex'] if v in MADE else HOMES


def seeded(v):
    made = v in MADE
    s = [(h + '/' + M, 'home', h if made else '-') for h in homes_of(v)] + [(H + '/el8671-kit-stray', 'stray', H if made else '-'),
         ('/var/lib/lampOS/voice/marked/' + M, 'marked', '/var/lib/lampOS/voice/marked'),
         ('/var/lib/lampOS/voice/schedule-delivery/ring-clips/' + M, 'clips_new', '/var/lib/lampOS/voice/schedule-delivery/ring-clips'),
]
    return s


def snap(v, name, since):
    o = ['SNAP=%s SNAP_WALL_US=1 SINCE_RESET_S=%d BOOT_ID=b1 BOOT_BEFORE=%s' % (name, since, 'b0' if v == 'fail-3reboot' else 'b1')]
    for f, kd, made in seeded(v):
        left = (v == 'fail-4a' and kd == 'home' and f.startswith(HOMES[1]))
        o.append('MARK %s %s %s' % (kd, 'present' if left else 'absent', f))
    o.append('HOMES=dir n=2 root:root:755')
    for h in homes_of(v):
        if v == 'fail-4a-home-removed' and h.endswith('/codex'):
            o.append('HOME %s absent' % h)
            continue
        o.append('HOME %s dir n=%d lampos:lampos:700' % (h, 1 if v == 'fail-4a' and h == HOMES[1] else 0))
    for k in KEPT:
        p = k.split(' ')[0]
        if p.endswith('history-marks.json') or p.endswith('schedule-delivery/ring-clips'):
            o.append('KIND %s absent' % p)
        elif p.endswith('/marked'):
            o.append('KIND %s dir n=0 root:root:700' % p)
        elif p.endswith('tts-cache') and v == 'fail-4a-kept':
            o.append('KIND %s absent' % p)
        elif p.endswith('voice/ring-clips'):
            o.append('KIND %s dir n=5 root:root:700' % p)
        else:
            o.append('KIND ' + k.replace('n=2 root:root:700', 'n=0 root:root:700'))
    o.append('CLIPS_OLD_OTHER=4')
    for l in LOGS:
        f, ino = l.split(' ')
        if f.endswith('.1') or f.endswith('.gz'):
            continue
        o.append('LOGFILE %s %s' % (f, '999' if v == 'fail-4a-inode' and f == '/var/log.hdd/syslog' else ino))
    o.append('SETUP_MARKER=absent RESET_JOURNAL=absent')
    o += ['UNIT lampos.target=inactive', 'UNIT lamp-setup=active', 'UNIT lamp-voice=%s' % ('active' if v in PASSLIKE and name == 'repaired' else 'inactive')]
    o.append('TMPFS=gone')
    o.append('LIVE_HOMES_LINE=0 LIVE_LOGS_LINE=0 LIVE_STARTED_LINE=1 LIVE_LOG_FAIL=0 LIVE_WIPE_LINES=3')
    o.append('PROBE_JOURNAL=1 PROBE_SYSLOG=1')
    o.append('VOICE_SINCE_END=%d WIPE_END_S=%d' % (57 if v in PASSLIKE and name == 'repaired' else 0, T + 60))
    o += counts(0, 0, hdd_j=4 if v == 'fail-4c' and name == '1' else 0)
    o.append('HDD_NEWEST_MTIME=%d RESET_S=%d' % (T + 900, T))
    o.append('WLAN0=DOWN WPA_CONF=absent')
    o.append('SNAP-DONE')
    return '\n'.join(o) + '\n'


def make(root, v):
    d = root / ('fake-' + v)
    shutil.rmtree(d, ignore_errors=True); d.mkdir(parents=True)
    w = lambda n, s: ((d / n).parent.mkdir(parents=True, exist_ok=True), (d / n).write_text(s))
    w('install.txt', 'BUILD=16600\nBIN_SHA=abcdef0123456789\nPLAN_SHA=07bc294005f1f8a3\nINSTALLED\n')
    w('leg1.txt', '\n'.join([
        'DRY check-pr exit=%d lines=0 ' % (2 if v == 'fail-1' else 0), 'DRY_ERR check-pr lines=0 refused_line=0',
        'DRY check-stock exit=0 lines=0 ', 'DRY_ERR check-stock lines=0 refused_line=0',
        'DRY agents exit=0 lines=12 remove=7 keep=3 cut=2 ', 'DRY_ERR agents lines=0 refused_line=0',
        'DRY homes exit=0 lines=2 empty=2 ', 'DRY_ERR homes lines=0 refused_line=0',
        'DRY logs exit=0 lines=9 truncate=8 empty=1 ', 'DRY_ERR logs lines=0 refused_line=0',
        'DRY homes-nodry exit=1 lines=0 ', 'DRY_ERR homes-nodry lines=1 refused_line=1',
        'DRY logs-nodry exit=1 lines=0 ', 'DRY_ERR logs-nodry lines=1 refused_line=1',
        'HOMES_BAD_LINES=0', 'LOGS_BAD_LINES=0', 'HOMES_LISTED=same', 'UNCHANGED_HOMES=yes', 'UNCHANGED_LOGS=yes', 'LEG1-DONE']) + '\n')
    if v == 'fail-1':
        return
    s = ['HOMES_DIR=absent', 'HOMES_MADE=yes (dir n=2 root:root:755; homes: %s)' % ' '.join(homes_of(v))] if v in MADE else ['HOMES_DIR=present']
    s += ['SEEDED %s %s' % (kd, f) for f, kd, made in seeded(v)] + ['SEED_WALL_US=1']
    s += ['KIND ' + k for k in KEPT] + ['HOME %s dir n=%d lampos:lampos:700' % (h, 1 if v in MADE else 4) for h in homes_of(v)]
    s += ['CLIPS_OLD_OTHER=4']
    s += ['LOGFILE ' + l for l in LOGS]
    s += counts(0 if v == 'void-4c' else 310, 0 if v == 'void-4c' else 1200)
    s += ['SEEDED_N=5', 'SEED-OK']
    w('seed.txt', '\n'.join(s) + '\n')
    w('oldclips.txt', '\n'.join([
        'OLD_OTHER_BEFORE=0', 'NRESTARTS_BEFORE=0', 'KIT_OLD_FILES=3', 'NEW_ENTRIES_BEFORE=%d' % (4 if v == 'new-cleared' else 0),
        'OLD_FILES_BEFORE=3 OLD_KIT_PRESENT=3', 'RESTART_WALL_US=1', 'listening_ready: pid 901 after 6s',
        'VOICE_PID=901 EXE=/run/el-8671/lamp-voice SHA_OK=yes SWITCH=off', 'OLD_REMOVED_N=1 OLD_LEFT_N=0', 'NEW_CLEARED_N=%d' % (1 if v == 'new-cleared' else 0),
        'OLD_REMOVED_COUNT=%d' % (2 if v == 'fail-2b' else 3), 'MUTED=no',
        'OLD_AFTER=%s' % ('dir n=0 root:root:700' if v == 'fail-2b-left' else 'absent'),
        'NEW_ENTRIES_AFTER=0 (information only: #8933 clears it with the switch off)', 'STOCK_BACK=yes',
        'STOCK_EXE=/opt/lampOS/voice/releases/1/lamp-voice NRESTARTS_AFTER=0', 'STOCK_MUTED=no', 'LEG2B-DONE']) + '\n')
    if v == 'noreset':
        w('go.txt', 'NO RESET: CONFIRM_RESET is not lamp-75c9: legs 1-2 only; putting back\n')
        w('putback.log', 'read-back 1: no kit marker left\nPUTBACK-OK\n')
        return
    w('trigger.txt', 'PRESENT_N=5 SEEDED_N=5\nRESET_CMD=/usr/bin/systemd-run --no-block --collect --unit=lamp-setup-wipe '
      '--property=Type=oneshot /run/el-8671/lamp-setup reset-now --plan /run/el-8671/reset-plan.json --enable\n'
      'SYSTEMD_RUN_EXIT=0\nTRIGGERED RESET_WALL_US=%d000000\n' % T)
    if v == 'unreachable':
        w('wait.txt', 'FAIL: not back in 600 s at orangepi@10.0.0.9\n')
        return
    exe = '/opt/lampOS/setup/releases/16600/lamp-setup' if v == 'fail-3exe' else '/run/el-8671/lamp-setup'
    w('after-files/after/status.txt', 'RECORDER_START boot=b1 1\nWIPE_SEEN exe=%s argv=x 2\nWIPE_END state=inactive '
      'load=not-found result=success/ 3\nFOLLOWER_STOPPED lines=9 4\nTMPFS_UNMOUNTED 5\nSNAP 0 6\nSNAP 1 7\n%s' % (
          exe, 'VOICE_ACTIVE 8\nSNAP repaired 9\nDONE repaired 10\n' if v in PASSLIKE else ''))
    rj = ['2026-11-28T10:05:01.000000+07:00 lamp-75c9 lamp-setup[1]: lamp-setup: reset: emptied 2 entries of Hermes and OpenClaw']
    if v != 'fail-3':
        rj.append("2026-11-28T10:05:01.100000+07:00 lamp-75c9 lamp-setup[1]: lamp-setup: reset: emptied 3 entries of the engines' homes")
    rj.append('2026-11-28T10:05:01.200000+07:00 lamp-75c9 lamp-setup[1]: lamp-setup: reset: emptied 11 log entries')
    rj.append('2026-11-28T10:05:09.000000+07:00 lamp-75c9 lamp-setup[1]: lamp-setup: reset: started in place, no reboot: lamp-setup.service')
    w('after-files/after/reset-journal.txt', '\n'.join(rj) + '\n')
    w('after-files/after/snap-0.txt', snap(v, '0', 70))
    w('after-files/after/snap-1.txt', snap(v, '1', 190))
    if v in PASSLIKE:
        w('after-files/after/snap-repaired.txt', snap(v, 'repaired', 5000))
    w('snap-live.txt', snap(v, 'live', 600))
    w('putback.log', 'recorder stopped\nremoved clips_old /var/lib/lampOS/voice/ring-clips/%s\nread-back 1: no kit marker left\nPUTBACK-OK\n' % M)


def verify(root):
    bad = 0
    for v, (want, legs) in VARIANTS.items():
        row = (root / ('fake-' + v) / 'row.md').read_text().splitlines()
        ok = row[0] == 'ROW #8671 %s on lamp-75c9 build 16600: %s' % (HEAD, want)
        got = {}
        for l in row:
            p = [x.strip() for x in l.split('|')]
            if len(p) == 6 and p[1] in ('1', '2', '2b', '3', '4a', '4b', '4c'):
                got[p[1]] = p[4]
        for k, want_leg in legs.items():
            ok = ok and got.get(k) == want_leg
        if v in MADE and any(l.startswith('Limit: this lamp has no') for l in row):
            ok = False
        print('%s fake-%s: %s, legs %s' % ('ok  ' if ok else 'BAD ', v, row[0].rsplit(': ', 1)[1], got))
        bad |= not ok
    print('judge on made-up dirs: %s' % ('all as expected' if not bad else 'A VERDICT IS WRONG'))
    return 1 if bad else 0


if __name__ == '__main__':
    if sys.argv[1] == '--verify':
        sys.exit(verify(Path(sys.argv[2])))
    r = Path(sys.argv[1]); r.mkdir(parents=True, exist_ok=True)
    for v in VARIANTS:
        make(r, v)
    print('made %d fake dirs in %s' % (len(VARIANTS), r))
