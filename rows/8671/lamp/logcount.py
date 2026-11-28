"""python3 -I logcount.py T [--root DIR] [--live]   (#8671 row kit; lamp and Mac)
Counts lines, never prints one. T is the reset's start (epoch seconds); 0 means "count every lamp-voice line".
For /var/log and /var/log.hdd (under --root): every text log of logs.rs TEXT_LOGS and its rotated copies (.1, .gz),
   TEXT dir=D files=N lines=N voice=N voice_before=N voice_after=N voice_unparsed=N
   ("voice" = a line naming lamp-voice anywhere; one whose time can't be read counts as before, never as after);
for /var/log.hdd/journal, each journal file through journalctl --file (only the lamp-voice unit's and tag's entries),
   JOURNAL dir=D files=N voice=N voice_before=N voice_after=N
and with --live the live journal (-u lamp-voice): LIVE voice_before=N voice_after=N."""
import calendar, gzip, os, re, subprocess, sys, time

T = float(sys.argv[1])
ROOT = sys.argv[sys.argv.index('--root') + 1] if '--root' in sys.argv else '/'
LIVE = '--live' in sys.argv
TEXT_LOGS = ('syslog', 'user.log', 'kern.log', 'auth.log')
RFC = re.compile(rb'^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)(\.\d+)?(Z|[+-]\d\d:?\d\d)?')
TRAD = re.compile(rb'^([A-Z][a-z]{2}) +(\d{1,2}) (\d\d):(\d\d):(\d\d) ')
MON = {m: i for i, m in enumerate([b'Jan', b'Feb', b'Mar', b'Apr', b'May', b'Jun', b'Jul', b'Aug', b'Sep', b'Oct',
                                   b'Nov', b'Dec'], 1)}
VOICE = b'lamp-voice'
FAR = 1e12


def stamp(line, year, month_now):
    m = RFC.match(line)
    if m:
        y, mo, d, h, mi, s = (int(x) for x in m.groups()[:6])
        t = calendar.timegm((y, mo, d, h, mi, s, 0, 0, 0)) + (float(m.group(7)) if m.group(7) else 0.0)
        z = m.group(8)
        if z is None:
            return time.mktime((y, mo, d, h, mi, s, 0, 0, -1)) + (t % 1)
        if z != b'Z':
            z = z.replace(b':', b'')
            off = (int(z[1:3]) * 3600 + int(z[3:5]) * 60) * (1 if z[:1] == b'+' else -1)
            t -= off
        return t
    m = TRAD.match(line)
    if m and m.group(1) in MON:
        mo = MON[m.group(1)]
        y = year - 1 if mo > month_now else year
        return time.mktime((y, mo, int(m.group(2)), int(m.group(3)), int(m.group(4)), int(m.group(5)), 0, 0, -1))
    return None


def is_text_log(name):
    for log in TEXT_LOGS:
        if name == log or name.startswith(log + '.'):
            return True
    return False


def text(folder):
    d = os.path.join(ROOT, folder.lstrip('/'))
    c = dict(files=0, lines=0, voice=0, voice_before=0, voice_after=0, voice_unparsed=0)
    if os.path.isdir(d) and not os.path.islink(d):
        for name in sorted(os.listdir(d)):
            p = os.path.join(d, name)
            if not is_text_log(name) or os.path.islink(p) or not os.path.isfile(p):
                continue
            c['files'] += 1
            mt = time.localtime(os.stat(p).st_mtime)
            try:
                f = gzip.open(p, 'rb') if name.endswith('.gz') else open(p, 'rb')
                with f:
                    for line in f:
                        c['lines'] += 1
                        if VOICE not in line:
                            continue
                        c['voice'] += 1
                        t = stamp(line, mt.tm_year, mt.tm_mon)
                        if t is None:
                            c['voice_unparsed'] += 1
                            c['voice_before'] += 1
                        elif t < (T if T > 0 else FAR):
                            c['voice_before'] += 1
                        else:
                            c['voice_after'] += 1
            except OSError as e:
                print('TEXT_UNREAD file=%s error=%s' % (p, e.__class__.__name__))
    print('TEXT dir=%s %s' % (folder, ' '.join('%s=%d' % kv for kv in c.items())))


def journal_lines(args):
    try:
        out = subprocess.run(['journalctl', '-q', '--no-pager', '-o', 'short-unix'] + args, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL, timeout=120).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    b = a = 0
    for line in out.splitlines():
        tok = line.split(b' ', 1)[0]
        try:
            t = float(tok)
        except ValueError:
            continue  # a continuation line of the entry before
        if t < (T if T > 0 else FAR):
            b += 1
        else:
            a += 1
    return b, a


MATCH = ['_SYSTEMD_UNIT=lamp-voice.service', '+', 'SYSLOG_IDENTIFIER=lamp-voice']


def journal(folder):
    d = os.path.join(ROOT, folder.lstrip('/'))
    files = []
    if os.path.isdir(d) and not os.path.islink(d):
        for dp, dns, fns in os.walk(d, followlinks=False):
            files += [os.path.join(dp, f) for f in fns if f.endswith('.journal') or f.endswith('.journal~')]
    b = a = 0
    unread = 0
    for f in sorted(files):
        r = journal_lines(['--file', f] + MATCH)
        if r is None:
            unread += 1
            continue
        b += r[0]; a += r[1]
    print('JOURNAL dir=%s files=%d voice=%d voice_before=%d voice_after=%d unread=%d' % (folder, len(files), b + a, b, a, unread))


text('/var/log')
text('/var/log.hdd')
journal('/var/log.hdd/journal')
if LIVE:
    r = journal_lines(['-u', 'lamp-voice'])
    print('LIVE voice_before=%s voice_after=%s' % (('%d' % r[0], '%d' % r[1]) if r else ('?', '?')))
