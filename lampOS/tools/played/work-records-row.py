#!/usr/bin/env python3
"""The work records ROW (#7930 part 1), run from the Mac against one lamp, in one command:

  work-records-row.py LABEL --tasks-bin LAMP_TASKS --voice-bin LAMP_VOICE [--clips DIR]
  work-records-row.py LABEL --put-back        # only the put-back, from the lamp's own state file

--only L4,L4s runs those legs alone (L0 and the put-back always; the rest SKIP (--only)); it takes L1,L2,L4,L4s,HW.
--deadline HH:MM or HH:MM:SS (local): the legs stop there and the put-back begins. SIGINT (Ctrl-C) or SIGTERM do the
same at once (--deadline-in S: S seconds from now); a signal during the put-back is held, so it always runs to its end.

Legs (docs/tasks.md "Work records" and "The voice asks about its work"; only start_agent_task/confirm_agent_task
work, kind task, keeps a record), each with a verdict file under
$PLAYED_OUT/scenarios/work-records-LABEL/ (PLAYED_OUT defaults to ~/lamp-played):

  L0  setup: the installed builds and the units' environment; LAMP_LAMP_ENGINE=openclaw for lamp-tasks by a
      drop-in when it is not (logged to /home/orangepi/changes.txt); our lamp-tasks and lamp-voice staged with a
      backup of the installed ones; only lamp-tasks and lamp-voice restarted, then waited for until ready.
  L1  a task, no voice: status, prepare (request.sock) and confirm (voice.sock), the voice's own ops, then
      {"op":"work","id":…} every --poll s (2) up to --cap s (240): states, elapsed_s, tools, step, session, answer.
  L2  the list: work_list while L1 runs and after it: L1's record newest both times.
  L4  the voice side, no spoken yes: a task handed over by request.sock as in L1 and waited for until
      working (a tool if it comes, up to 90 s), then only the how clip (--clips DIR from work-records-clips.sh:
      en-how, then vi-how; -what with --how-clip what), played here at the Mac's own output of at most 70%
      (read, never changed; the lamp's volume is never touched). PASS: check_agent_task on that turn, the reply
      naming the state, and the record's id, state and step. Each turn's lamp-voice lines are kept by request.
  L4s (--l4-spoken; never blocks) the old spoken flow: ask, yes, then how (en-ask, en-yes, … too).
  HW  (only when --only names it; en only) "how far is it?" while a long do-task runs (en-long-do, en-yes:
      start_agent_task); its record polled until working with a tool (--running-cap 150 s), then en-how.
      PASS: check_agent_task called, the record working as the how was heard, the reply naming a running
      state or the step, not the answer; VOID when the record was already over then ("finished too fast").
      --clips DIR:DIR looks in each folder.
  Every leg runs between two reads of the units' running binaries (/proc/<MainPID>/exe): either one not
      the installed test build's makes it VOID (lamp-update rolled the lamp), and the run stops for the put-back.
  put-back  the installed binaries back (their SHA-256 checked), the drop-in removed (and a lamp-voice
      hard-asks drop-in an older kit run left, under /run or /etc), both units restarted, read twice, logged.

Nothing is confirmed by voice but the task the clips ask for. Every lamp command goes through lamp_ssh, or
WORK_SSH's stand-in, one `sudo -n` per command (a stand-in may turn it into `sudo -S`). WORK_ROOT prefixes every
lamp path (the dry run's stand-in only; never set for a lamp).
"""
import base64
import hashlib
import json
import os
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.environ.get('WORK_ROOT', '')
RUN = f'{ROOT}/run/lampos-tasks'
REQUEST, VOICE = f'{RUN}/request.sock', f'{RUN}/voice.sock'
STATE_DIR = f'{ROOT}/var/tmp/work-records-7930'
STATE = f'{STATE_DIR}/state.json'
# The drop-in lives under /run, as the other kits' do: a reboot drops it, so the engine goes back with it. The binaries swapped over the installed release files do survive a reboot: after one, run
# --put-back (its state is in /var/tmp, which a reboot keeps).
DROP_IN_DIR = f'{ROOT}/run/systemd/system/lamp-tasks.service.d'
DROP_IN = f'{DROP_IN_DIR}/zz-7930-row-engine.conf'
# A lamp-voice hard-asks drop-in an older kit run wrote (this kit writes none): the put-back removes it.
VOICE_DROP_IN_DIR = f'{ROOT}/run/systemd/system/lamp-voice.service.d'
VOICE_DROP_IN = f'{VOICE_DROP_IN_DIR}/zz-7930-row-hard-asks.conf'
# Where an older kit wrote them: the put-back removes these too.
OLD_DROP_IN_DIR = f'{ROOT}/etc/systemd/system/lamp-tasks.service.d'
OLD_DROP_IN = f'{OLD_DROP_IN_DIR}/zz-7930-row-engine.conf'
OLD_VOICE_DROP_IN_DIR = f'{ROOT}/etc/systemd/system/lamp-voice.service.d'
OLD_VOICE_DROP_IN = f'{OLD_VOICE_DROP_IN_DIR}/zz-7930-row-hard-asks.conf'
CHANGES = f'{ROOT}/home/orangepi/changes.txt'
SESSIONS = f'{ROOT}/var/lib/lampOS/voice/sessions'
TMP = f'{ROOT}/tmp'
HELPER = f'{TMP}/work-records-ask.py'
UNITS = ('lamp-tasks', 'lamp-voice')
# Short research: a few tool calls, done within L1's cap on c0a4 (the slot's whole run is 18 min).
TASK = ('Look up the weather forecast for Hanoi tomorrow on two different websites and tell me in two short '
        'lines whether they agree.')
DONE_STATES = {'done', 'stopped', 'failed', 'timed_out', 'cancelled'}
# The lamp naming how far its work is, in its reply: English and Vietnamese.
STATE_WORDS = re.compile(r'\b(working|still|in progress|searching|looking|reading|step|steps|done|finished|'
                         r'queued|started|waiting|almost|tools?)\b|đang|xong|bước|chờ|hoàn thành|tìm|bắt đầu',
                         re.IGNORECASE)
# lamp-update's journal when a round installs or switches a build (it is never stopped or touched here).
# Only these: lamp-update's routine "OpenClaw may be installed now: …" is not a round (c0a4-5).
ROUND_LINE = re.compile(r'switched to build|installed /opt/lampOS/')
# lamp-tasks' journal once its kept session stands.
KEPT_LINE = re.compile(r'kept session.*\b(idle|working)\b')
TOOLS_LINE = re.compile(r'lamp-voice second: request (\d+): tools: \S+ calls=(\S+)')
# Any lamp-voice line naming its request: the read-back, a yes taken in code, the calls.
REQUEST_LINE = re.compile(r'\brequest (\d+)\b')

# Runs on the lamp: one line to a socket, each answer line printed with the lamp's time as it comes.
HELPER_SOURCE = r'''import base64, json, socket, sys, time
path, line, most, within = sys.argv[1], base64.b64decode(sys.argv[2]), int(sys.argv[3]), float(sys.argv[4])
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.settimeout(within)
s.connect(path)
s.sendall(line + b"\n")
buf, n = b"", 0
end = time.time() + within
try:
    while n < most and time.time() < end:
        s.settimeout(max(0.1, end - time.time()))
        part = s.recv(65536)
        if not part:
            break
        buf += part
        while b"\n" in buf and n < most:
            one, buf = buf.split(b"\n", 1)
            print(json.dumps({"at": time.time(), "line": json.loads(one)}), flush=True)
            n += 1
except socket.timeout:
    print(json.dumps({"at": time.time(), "timeout": True}), flush=True)
'''


# Spoken fractions as percentages, English and Vietnamese (c0a4-6: "about a quarter" for 24%).
FRACTIONS = [(r'three quarters|three-quarters|ba phần tư', 75), (r'two thirds|hai phần ba', 67),
             (r'\bhalf\b|một nửa', 50), (r'(?:a|one) third|một phần ba', 33), (r'(?:a|one) quarter|một phần tư', 25),
             (r'(?:a|one) tenth|một phần mười', 10)]
NUMBER_WORDS = {w: n for n, w in enumerate('zero one two three four five six seven eight nine ten eleven twelve thirteen '
                                           'fourteen fifteen sixteen seventeen eighteen nineteen twenty'.split())}
NUMBER_WORDS.update({'thirty': 30, 'forty': 40, 'fifty': 50, 'sixty': 60, 'seventy': 70, 'eighty': 80, 'ninety': 90})


def spoken_numbers(reply):
    """The numbers in `reply`: digits, number words up to ninety, and fractions as percentages."""
    text = reply.lower()
    found = [float(n) for n in re.findall(r'\d+(?:\.\d+)?', text)]
    found += [float(NUMBER_WORDS[w]) for w in re.findall(r'[a-z]+', text) if w in NUMBER_WORDS]
    for pattern, percent in FRACTIONS:
        if re.search(pattern, text):
            found.append(float(percent))
            text = re.sub(pattern, ' ', text)
    return found


def answer_told(reply, answer):
    """Whether `reply` tells the record's `answer`, in any language: two of the answer's numbers said (within
    1, or 2 for a percentage said as a fraction), or at least 3 of its words of 4 letters or more."""
    if not reply or not answer:
        return False
    said = set(re.findall(r'\w+', reply.lower()))
    words = {w for w in re.findall(r'\w+', answer.lower()) if len(w) >= 4 and not w.isdigit()}
    if len(words & said) >= 3:
        return True
    heard = spoken_numbers(reply)
    numbers = {float(n) for n in re.findall(r'\d+(?:\.\d+)?', answer)}
    matched = {n for n in numbers if any(abs(n - h) <= (2 if n > 20 else 1) for h in heard)}
    return len(matched) >= 2


class LockLost(BaseException):
    """The lamp's lock is not ours: nothing more is done on the lamp, the put-back neither."""


class Lock:
    """The shared lamp lock (`lampOS/tools/lamp-lock.sh`, or the script `WORK_LOCK_SCRIPT` names): ours only while its holder
    and session are ours and it has not expired. Renewed by the kit at most every --lock-renew s (180) while
    it runs."""

    def __init__(self, options):
        self.script = os.environ.get('WORK_LOCK_SCRIPT') or os.path.join(
            os.path.dirname(os.path.abspath(__file__)), '..', 'lamp-lock.sh')
        self.name, self.holder, self.session = options['lock_name'], options['lock_holder'], options['lock_session']
        self.every = options['lock_renew']
        self.lost = None
        self.stopped = threading.Event()

    def holder_now(self):
        """None when the lock is ours, else who or what holds it, in words."""
        try:
            done = subprocess.run([self.script, self.name, 'status'], capture_output=True, text=True, timeout=60,
                                  stdin=subprocess.DEVNULL)
        except (OSError, subprocess.TimeoutExpired) as error:
            return f'unknown ({error})'
        out = done.stdout + done.stderr
        if done.returncode == 0:
            return 'nobody (the lock is free)'
        if done.returncode != 1:
            return f'unknown (status exit {done.returncode}: {out.strip()[-120:]})'
        field = lambda name: (re.search(rf'^\s*{name}: (.*)$', out, re.MULTILINE) or [None, ''])[1].strip()
        holder, session = field('holder'), field('session')
        if holder == self.holder and session == self.session and 'expired' not in out:
            return None
        return f'{holder or "?"} ({session or "?"}){", expired" if "expired" in out else ""}'

    def renewing(self, log):
        def loop():
            while not self.stopped.wait(self.every):
                done = subprocess.run([self.script, self.name, 'renew', self.holder, self.session],
                                      capture_output=True, text=True, timeout=60, stdin=subprocess.DEVNULL)
                if done.returncode != 0:
                    self.lost = f'renew failed (exit {done.returncode}): {(done.stdout + done.stderr).strip()[-120:]}'
                    log(f'lock: {self.lost}')
                    return
        threading.Thread(target=loop, daemon=True).start()


class Rolled(BaseException):
    """A unit's running binary changed during a leg: lamp-update rolled the lamp. The leg is VOID, the run
    stops, and the put-back runs."""


ROLLED = 'the binary changed mid-leg: lamp-update rolled the lamp'


def binary_changed(expected, running):
    """The units whose running binary (/proc/<MainPID>/exe's SHA-256) is not the installed test build's."""
    return sorted(unit for unit, want in expected.items() if running.get(unit) != want)


class Refused(Exception):
    """A stale state's put-back failed at L0: nothing staged, and no put-back again."""


class Stop(BaseException):
    """SIGINT, SIGTERM or --deadline: the legs end where they are, and the put-back runs."""


def stop_on(signum, _frame):
    raise Stop({signal.SIGINT: 'SIGINT', signal.SIGTERM: 'SIGTERM', signal.SIGALRM: 'the deadline'}[signum])


def held_during_put_back(signum, _frame):
    print(f'{time.strftime("%H:%M:%S")} signal {signum} held: the put-back runs to its end', flush=True)


class Lamp:
    def __init__(self):
        self.ssh = [os.environ.get('WORK_SSH') or os.path.join(HERE, 'lamp_ssh')]
        self.env = dict(os.environ, LAMP_SSH_TIMEOUT=os.environ.get('LAMP_SSH_TIMEOUT', '8'))

    def run(self, command, timeout=40, check=True):
        done = subprocess.run(self.ssh + [command], capture_output=True, text=True, timeout=timeout,
                              env=self.env, stdin=subprocess.DEVNULL)
        if check and done.returncode != 0:
            raise RuntimeError(f'lamp command failed ({done.returncode}): {command[:60]}: {done.stderr.strip()[-200:]}')
        return done.stdout

    def put(self, local, remote, mode='755'):
        """`local` to `remote` through ssh's input, without sudo, made whole before it is named."""
        with open(local, 'rb') as f:
            done = subprocess.run(self.ssh + [f'cat > {remote}.part && chmod {mode} {remote}.part && mv {remote}.part {remote}'],
                                  stdin=f, capture_output=True, env=self.env, timeout=600)
        if done.returncode != 0:
            raise RuntimeError(f'copy to the lamp failed ({done.returncode})')

    def put_text(self, text, remote, mode='644'):
        with tempfile.NamedTemporaryFile('w', delete=False) as f:
            f.write(text)
        try:
            self.put(f.name, remote, mode)
        finally:
            os.unlink(f.name)

    def ask(self, socket_path, request, most=1, within=10.0, timeout=None):
        """Each answer line to `request` on `socket_path`, as {"at", "line"} (or {"timeout"})."""
        line = base64.b64encode(json.dumps(request).encode()).decode()
        out = self.run(f'sudo -n python3 {HELPER} {socket_path} {line} {most} {within}',
                       timeout=timeout or within + 30)
        return [json.loads(row) for row in out.splitlines() if row.strip()]

    def ask_one(self, socket_path, request, within=10.0):
        rows = self.ask(socket_path, request, 1, within)
        return rows[0]['line'] if rows and 'line' in rows[0] else None

    def monotonic_us(self):
        return int(self.run("python3 -c 'import time; print(time.monotonic_ns()//1000)'").strip())

    def now(self):
        return float(self.run("python3 -c 'import time; print(time.time())'").strip())

    def environment(self, unit):
        text = self.run(f'systemctl show {unit} -p Environment --value')
        return dict(part.split('=', 1) for part in text.split() if '=' in part)

    def link(self, unit):
        """The unit's ExecStart path as written (through `current`)."""
        text = self.run(f'systemctl show {unit} -p ExecStart --value')
        found = re.search(r'path=(\S+)', text)
        if not found:
            raise RuntimeError(f'no ExecStart path for {unit}')
        return found.group(1)

    def binary(self, unit):
        """The file the unit runs now: its ExecStart path through the `current` links."""
        return self.run(f'readlink -f {self.link(unit)}').strip()

    def round_since(self, since):
        """lamp-update's install or switch lines since `since` (lamp time): a round, read only."""
        text = self.run(f'sudo -n journalctl -u lamp-update -o cat --no-pager --since @{int(since)}', timeout=60,
                        check=False)
        return [line.strip()[:200] for line in text.splitlines() if ROUND_LINE.search(line)]

    def sha(self, path):
        return self.run(f'sudo -n sha256sum {path}').split()[0]


# The how turn's tool, HW's.
HOW_TOOL = 'check_agent_task'
# c0a4-15 HW: "…been quiet for about nine minutes on the weather-style lookups, though. I'll tell you myself
# when it's done." names a running task without these words: a promise to tell when done counts too.
RUNNING_WORDS = re.compile(r"\b(working|still|looking|checking|searching|going|underway|on it|not done|"
                           r"(?:tell|let) you (?:\w+ )?(?:know )?when (?:it'?s|it is) (?:done|finished|ready))\b",
                           re.IGNORECASE)


def record_checks(record, steps_on):
    """What a record must carry for a leg: with lamp-tasks' LAMP_TASKS_STEPS=on its id, state and step
    (strict); off (the product), the steps reader never runs and `step` stays null (c0a4-16), so its id,
    state, a tool counted and its session instead."""
    if steps_on:
        return {'record_has_id_state_step': bool(record.get('id') and record.get('state') and record.get('step'))}
    return {'record_has_id_state_tools_session': bool(record.get('id') and record.get('state')
                                                      and record.get('tools') is not None and record.get('session'))}


def running_verdict(leg, at_heard, calls, reply, after, steps_on=False):
    """HW scored: VOID when the record was already over as the how was heard ("finished too fast");
    else PASS only when the how turn called the how tool, the record was `working` as it was heard and
    carried what `record_checks` asks for the steps switch, and the reply names a running state or the
    step (its tool's name as words), not the answer."""
    if at_heard.get('done') or at_heard.get('state') in ('done', 'failed', 'timed_out', 'cancelled', 'stopped'):
        return {'verdict': 'VOID', 'checks': {}, 'why': 'finished too fast: the record was '
                f'{at_heard.get("state")} when the how was heard'}
    step = ((at_heard.get('step') or {}).get('tool') or '').replace('_', ' ').strip().lower()
    checks = {
        'how_tool_called': any(HOW_TOOL in c['calls'] for c in calls),
        'working_when_heard': at_heard.get('state') == 'working',
        'reply_names_running_state_or_step': bool(RUNNING_WORDS.search(reply)) or bool(step and step in reply.lower()),
        'reply_not_the_answer': not answer_told(reply, after.get('answer')),
        **record_checks(at_heard, steps_on),
    }
    ok = all(checks.values())
    return {'verdict': 'PASS' if ok else 'FAIL', 'checks': checks,
            'why': '' if ok else ', '.join(k for k, v in checks.items() if not v)}


class Row:
    def __init__(self, label, options):
        self.label, self.options = label, options
        self.lamp = Lamp()
        self.out = os.path.join(os.environ.get('PLAYED_OUT', os.path.expanduser('~/lamp-played')), 'scenarios',
                                f'work-records-{label}')
        os.makedirs(self.out, exist_ok=True)
        self.log_file = open(os.path.join(self.out, 'run.log'), 'a', encoding='utf-8')
        self.verdicts = {}
        self.began = time.time()
        self.refused = None
        self.lock = None if options.get('no_lock') else Lock(options)
        if self.lock:
            self.lock.renewing(self.log)
        self.since = 0
        self.state = None
        self.steps = None

    def steps_on(self):
        """Whether lamp-tasks runs with LAMP_TASKS_STEPS=on (its units' environment, read once): only then
        is a record's `step` required."""
        if self.steps is None:
            self.steps = self.lamp.environment('lamp-tasks').get('LAMP_TASKS_STEPS', '') == 'on'
            self.log(f'LAMP_TASKS_STEPS={"on" if self.steps else "off"}: a record\'s step is '
                     f'{"required" if self.steps else "not required (state, tools and session are)"}')
        return self.steps

    def log(self, text):
        line = f'{time.strftime("%Y-%m-%dT%H:%M:%S%z")} {text}'
        print(line, flush=True)
        self.log_file.write(line + '\n')
        self.log_file.flush()

    def verdict(self, leg, result):
        result = dict(result, leg=leg, label=self.label, at=time.strftime('%Y-%m-%dT%H:%M:%S%z'))
        with open(os.path.join(self.out, f'{leg}.json'), 'w', encoding='utf-8') as f:
            json.dump(result, f, ensure_ascii=False, indent=2)
        self.verdicts[leg] = result['verdict']
        self.log(f'{leg}: {result["verdict"]}' + (f' ({result["why"]})' if result.get('why') else ''))
        return result

    # L0
    def setup(self):
        lamp, o = self.lamp, self.options
        if lamp.run(f'test -e {STATE} && echo there || true').strip() == 'there':
            self.log(f'L0: {STATE} is there from an earlier run: its put-back first')
            if self.put_back()['verdict'] != 'PASS':
                raise Refused(f'{STATE} is there and its put-back failed (see putback.json); fix the lamp by hand '
                              'or run --put-back again; nothing staged')
            self.verdicts.pop('putback', None)
        self.since = lamp.now()
        lamp.put_text(HELPER_SOURCE, HELPER, '755')
        found = {}
        for unit in UNITS:
            path = lamp.binary(unit)
            # Never run: the build is read by its SHA-256 and its release folder alone.
            found[unit] = {'path': path, 'sha256': lamp.sha(path)}
        engine = lamp.environment('lamp-tasks').get('LAMP_LAMP_ENGINE', '')
        self.log(f'L0: installed {json.dumps(found)}; LAMP_LAMP_ENGINE={engine or "(unset)"}')
        staged = {'lamp-tasks': o['tasks_bin'], 'lamp-voice': o['voice_bin']}
        drop_lines = ([] if engine == 'openclaw' else ['LAMP_LAMP_ENGINE=openclaw'])
        state = {'label': self.label, 'drop_in': bool(drop_lines), 'drop_lines': drop_lines, 'units': {},
                 'since': self.since}
        for unit, local in staged.items():
            with open(local, 'rb') as f:
                want = hashlib.sha256(f.read()).hexdigest()
            remote = f'{TMP}/work-records-{unit}'
            lamp.put(local, remote)
            if lamp.sha(remote) != want:
                return self.verdict('L0', {'verdict': 'FAIL', 'why': f'{unit} copy changed on the way'})
            backup = f'{STATE_DIR}/{unit}.installed'
            state['units'][unit] = {'path': found[unit]['path'], 'link': lamp.link(unit), 'installed_sha256': found[unit]['sha256'],
                                    'backup': backup, 'staged_sha256': want, 'staged': remote}
        # The state first, so a put-back knows what to undo whatever happens next.
        lamp.put_text(json.dumps(state), f'{TMP}/work-records-state.json')
        lamp.run(f'sudo -n sh -c "mkdir -p {STATE_DIR} && mv {TMP}/work-records-state.json {STATE}"')
        stamp = time.strftime('%Y-%m-%d %H:%M')
        for unit, row in state['units'].items():
            lamp.run(f'sudo -n sh -c "cp -p {row["path"]} {row["backup"]} && install -m 755 {row["staged"]} '
                     f'{row["path"]}.row && mv {row["path"]}.row {row["path"]} && rm -f {row["staged"]} && '
                     f'echo \'{stamp} #7930 work-records ROW {self.label}: {unit} staged {row["staged_sha256"][:12]} '
                     f'(installed {row["installed_sha256"][:12]} kept at {row["backup"]})\' >> {CHANGES}"')
        if state['drop_in']:
            body = '[Service]\\n' + ''.join(f'Environment={x}\\n' for x in drop_lines)
            lamp.run(f'sudo -n sh -c "mkdir -p {DROP_IN_DIR} && printf \'{body}\' '
                     f'> {DROP_IN} && systemctl daemon-reload && echo \'{stamp} #7930 work-records ROW {self.label}: '
                     f'drop-in {DROP_IN} {" ".join(drop_lines)} (removed at put-back)\' >> {CHANGES}"')
        ready = self.restart()
        now = {unit: lamp.sha(row['path']) for unit, row in state['units'].items()}
        self.state = state
        ok = ready['ok'] and all(now[u] == state['units'][u]['staged_sha256'] for u in now)
        return self.verdict('L0', {'verdict': 'PASS' if ok else 'FAIL', 'installed': found, 'engine_before': engine,
                                   'drop_in': state['drop_in'],
                                   'running_sha256': now, 'ready': ready,
                                   'why': '' if ok else 'not ready or not our build'})

    def restart(self, engine='openclaw', within=120):
        """lamp-tasks then lamp-voice restarted, and waited for: tasks answers status, its lamp target on
        `engine` (any when None), lamp-voice active for 5 s."""
        lamp = self.lamp
        lamp.run('sudo -n systemctl restart lamp-tasks lamp-voice', timeout=120)
        began, seen = time.time(), {}
        while time.time() - began < within:
            time.sleep(2)
            try:
                status = lamp.ask_one(REQUEST, {'op': 'status'}, 5)
                targets = lamp.ask_one(REQUEST, {'op': 'targets'}, 5)
            except (RuntimeError, subprocess.TimeoutExpired, ValueError):
                continue
            mine = [t for t in (targets or {}).get('targets', []) if t.get('alias') == 'lamp']
            seen = {'status_ok': bool(status and status.get('ok')), 'lamp_target': mine}
            if not seen['status_ok'] or not mine or (engine and mine[0].get('engine') != engine):
                continue
            if lamp.run('systemctl is-active lamp-voice', check=False).strip() != 'active':
                continue
            time.sleep(5)
            if lamp.run('systemctl is-active lamp-voice', check=False).strip() == 'active':
                return dict(seen, ok=True, seconds=round(time.time() - began))
        return dict(seen, ok=False, seconds=round(time.time() - began))

    # L1, a task handed over as the voice's confirm does
    def hand_over(self, text):
        lamp = self.lamp
        status = lamp.ask_one(REQUEST, {'op': 'status'})
        guard = lambda tag: {'store_id': status['store_id'], 'revision': status['revision'],
                             'request_id': f'row-7930-{tag}-{int(time.time() * 1000)}'}
        # A prepare refused `conflict` prepared nothing (c0a4-6 vi: the first task settling moved the store's
        # revision), so it is asked again on a fresh status, a few times.
        for tries in range(4):
            prepared = lamp.ask_one(REQUEST, {'op': 'prepare', 'guard': guard('prepare'), 'text': text, 'target': 'lamp'})
            if (prepared or {}).get('error') != 'conflict':
                break
            time.sleep(1)
            status = lamp.ask_one(REQUEST, {'op': 'status'})
        if not prepared or not prepared.get('ok'):
            raise RuntimeError(f'prepare refused: {prepared}')
        if tries:
            self.log(f'prepare taken after {tries} conflict(s)')
        # The store's revision moves whenever any task's state does (c0a4-5: L1's task settling between
        # the status read and the confirm gave `conflict`), so a conflict is read again and the same
        # prepared task confirmed again on the fresh revision, a few times; nothing else is retried.
        refusals = []
        for _ in range(4):
            status = lamp.ask_one(REQUEST, {'op': 'status'})
            confirmed = lamp.ask_one(VOICE, {'op': 'confirm', 'guard': guard('confirm'), 'id': prepared['id'],
                                             'prepared_digest': prepared['prepared_digest'],
                                             'session_id': status['summary']['session_id']})
            if confirmed and confirmed.get('ok'):
                if refusals:
                    self.log(f'confirm of {prepared["id"]} taken after {len(refusals)} conflict(s)')
                return str(prepared['id'])
            refusals.append(confirmed)
            if (confirmed or {}).get('error') != 'conflict':
                break
            time.sleep(1)
        raise RuntimeError(f'confirm refused: {refusals}')

    def follow(self, work_id, cap, while_running=None):
        """The work's record every --poll s until done or `cap` s: the polls kept whole, the changes logged."""
        polls, last, began = [], None, time.time()
        while time.time() - began < cap:
            try:
                answer = self.lamp.ask_one(REQUEST, {'op': 'work', 'id': work_id})
            except (RuntimeError, subprocess.TimeoutExpired, ValueError) as error:
                answer = {'ok': False, 'error': f'not asked: {error}'[:200]}
            record = (answer or {}).get('work')
            if not record and self.state:
                # No socket or no record: a lamp-update round restarts lamp-tasks, which forgets its records.
                self.no_round()
            polls.append({'t': round(time.time() - began, 1), 'answer': answer})
            if record:
                view = {k: record.get(k) for k in ('state', 'tools', 'step', 'session', 'done')}
                if view != last:
                    self.log(f'work {work_id}: {json.dumps(view, ensure_ascii=False)} elapsed_s={record.get("elapsed_s")}')
                    last = view
                if while_running and not record.get('done') and record.get('state') not in (None, 'queued'):
                    while_running()
                    while_running = None
                if record.get('done'):
                    break
            time.sleep(self.options['poll'])
        return polls

    def running_shas(self):
        """The SHA-256 of each unit's running binary, read through /proc/<MainPID>/exe (one sudo)."""
        pids = {u: self.lamp.run(f'systemctl show {u} -p MainPID --value', check=False).strip() for u in UNITS}
        files = ' '.join(f'{ROOT}/proc/{pid}/exe' for pid in pids.values())
        out = self.lamp.run(f"sudo -n sh -c 'sha256sum {files}'", check=False).split('\n')
        shas = {}
        for unit, pid in pids.items():
            line = next((x for x in out if x.strip().endswith(f'/proc/{pid}/exe')), '')
            shas[unit] = line.split()[0] if line.strip() else None
        return shas

    def leg_on_our_build(self, leg, run):
        """`run` (a leg) between two reads of the running binaries: either not the installed test build's,
        the leg is VOID (never PASS or FAIL), and the run stops for the put-back."""
        expected = {u: row['staged_sha256'] for u, row in (self.state or {}).get('units', {}).items()
                    if row.get('staged_sha256')}
        if not expected:
            return run()
        for when in ('before', 'after'):
            if when == 'after':
                result = run()
            running = self.running_shas()
            changed = binary_changed(expected, running)
            if changed:
                self.log(f'{leg}: VOID, {ROLLED} ({", ".join(changed)} {when} the leg); the run stops for the put-back')
                self.verdict(leg, {'verdict': 'VOID', 'why': f'{ROLLED}: {", ".join(changed)} {when} the leg',
                                   'expected_sha256': expected, 'running_sha256': running})
                raise Rolled(f'{", ".join(changed)} {when} {leg}')
        return result

    def lock_held(self, before):
        """Stops everything at once, nothing more done on the lamp, when the lock is not ours."""
        if self.lock is None:
            return
        holder = self.lock.lost or self.lock.holder_now()
        if holder:
            self.log(f'lock not held: {holder} (before {before}); nothing more is done on the lamp')
            raise LockLost(holder)

    def no_round(self):
        """Stops the legs when lamp-update ran a round since L0: a `current` link moved or its journal says so.
        The lock is looked at first: a leg begins only while it is ours."""
        self.lock_held('a leg')
        moved = [u for u, row in self.state['units'].items() if self.lamp.binary(u) != row['path']]
        lines = self.lamp.round_since(self.since)
        if moved or lines:
            self.log(f'a lamp-update round since L0: links moved {moved}, journal {lines[:3]}')
            raise Stop('a lamp-update round')

    def sockets_ready(self, within=60):
        """request.sock there and the kept session standing, waited for up to `within` s."""
        began, sock, kept = time.time(), False, False
        while time.time() - began < within:
            sock = self.lamp.run(f'sudo -n test -S {REQUEST} && echo there || true').strip() == 'there'
            text = self.lamp.run(f'sudo -n journalctl -u lamp-tasks -o cat --no-pager --since @{int(self.since)}',
                                 timeout=60, check=False)
            kept = any(KEPT_LINE.search(line) for line in text.splitlines())
            if sock and kept:
                break
            time.sleep(2)
        self.log(f'before L1: request.sock {"there" if sock else "MISSING"}, kept session '
                 f'{"standing" if kept else "not seen"} ({round(time.time() - began)} s)')
        return sock, kept

    def task_leg(self):
        sock, kept = self.sockets_ready()
        if not sock:
            return self.verdict('L1', {'verdict': 'FAIL', 'why': f'{REQUEST} not there after 60 s (kept session '
                                       f'{"seen" if kept else "not seen either"})'}), None
        try:
            work_id = self.hand_over(TASK)
        except (RuntimeError, KeyError, TypeError) as error:
            return self.verdict('L1', {'verdict': 'FAIL', 'why': str(error)}), None
        self.log(f'L1: task {work_id} handed over')
        during = {}
        polls = self.follow(work_id, self.options['cap'],
                            lambda: during.update(self.lamp.ask_one(REQUEST, {'op': 'work_list', 'limit': 5}) or {}))
        records = [p['answer']['work'] for p in polls if (p['answer'] or {}).get('work')]
        states = list(dict.fromkeys(r['state'] for r in records))
        tools = [r.get('tools') or 0 for r in records]
        steps = [r['step'] for r in records if r.get('step')]
        sessions = [r['session'] for r in records if r.get('session')]
        last = records[-1] if records else {}
        checks = {
            'two_states': len(states) >= 2,
            'tools_rose': bool(tools) and max(tools) > 0 and any(b > a for a, b in zip(tools, tools[1:])),
            'step_set': bool(steps),
            'session_filled': bool(sessions),
            'done_with_answer': last.get('state') == 'done' and bool(last.get('answer')),
        }
        must = (('two_states', 'tools_rose', 'step_set', 'done_with_answer') if self.steps_on()
                else ('two_states', 'tools_rose', 'session_filled', 'done_with_answer'))
        ok = all(checks[k] for k in must)
        result = {'verdict': 'PASS' if ok else 'FAIL', 'work_id': work_id, 'task': TASK, 'states': states,
                  'tools_seen': sorted(set(tools)), 'steps_seen': [dict(s) for s in {json.dumps(s): s for s in steps}.values()],
                  'session': sessions[-1] if sessions else None, 'elapsed_s': last.get('elapsed_s'),
                  'answer': last.get('answer'), 'checks': checks, 'polls': polls,
                  'why': '' if ok else 'missing: ' + ', '.join(k for k in must if not checks[k])}
        self.verdict('L1', result)
        return result, during

    def list_leg(self, task_id, during):
        after = self.lamp.ask_one(REQUEST, {'op': 'work_list', 'limit': 5}) or {}
        ids = lambda answer: [w.get('id') for w in answer.get('works', [])]
        checks = {'during_newest_is_the_task': ids(during)[:1] == [task_id],
                  'after_newest_is_the_task': ids(after)[:1] == [task_id]}
        ok = all(checks.values())
        return self.verdict('L2', {'verdict': 'PASS' if ok else 'FAIL', 'during': during, 'after': after,
                                   'checks': checks, 'why': '' if ok else 'failed: ' + ', '.join(k for k, v in checks.items() if not v)})

    # L4, by voice
    def clip(self, name):
        """The clip `name` from --clips: one folder, or several joined by ':', the first that has it."""
        for folder in self.options['clips'].split(':'):
            for ext in ('.wav', '.mp3', '.m4a'):
                path = os.path.join(folder, name + ext)
                if os.path.exists(path):
                    return path
        return None

    def mac_volume(self):
        if os.environ.get('WORK_MAC_VOLUME'):
            return int(os.environ['WORK_MAC_VOLUME'])
        out = subprocess.run(['osascript', '-e', 'output volume of (get volume settings)'], capture_output=True, text=True)
        return int(out.stdout.strip() or 101)

    def play(self, path):
        subprocess.run([os.environ.get('WORK_PLAY', '/usr/bin/afplay'), path], check=True, timeout=60)

    def clips_ready(self, leg, names):
        """None when `names` are there and the Mac is at most 70%; else the leg's SKIP or FAIL verdict."""
        if not self.options.get('clips'):
            return self.verdict(leg, {'verdict': 'SKIP', 'why': 'no --clips: there is no typed-turn entry, so '
                                      f'{leg} needs the pre-rendered clips ' + ', '.join(names)})
        missing = [n for n in names if not self.clip(n)]
        if missing:
            return self.verdict(leg, {'verdict': 'FAIL', 'why': 'clips missing: ' + ', '.join(missing)})
        volume = self.mac_volume()
        if volume > 70:
            return self.verdict(leg, {'verdict': 'FAIL', 'why': f"the Mac's output is {volume}%, past 70%: "
                                      'lower it by hand (the kit never changes it)'})
        return None

    def working_task(self, reuse=None, within=90):
        """A task handed over as L1 does (no voice), waited for until `working` with a tool if it comes,
        up to `within` s; `reuse` (an id) when that one is still working. Its id and latest record."""
        if reuse:
            record = (self.lamp.ask_one(REQUEST, {'op': 'work', 'id': reuse}) or {}).get('work') or {}
            if record.get('state') == 'working' and not record.get('done'):
                return reuse, record
        work_id = self.hand_over(TASK)
        self.log(f'L4: task {work_id} handed over for the voice to ask about')
        began, record = time.time(), {}
        while time.time() - began < within:
            record = (self.lamp.ask_one(REQUEST, {'op': 'work', 'id': work_id}) or {}).get('work') or {}
            if record.get('done') or (record.get('state') == 'working' and (record.get('tools') or 0) >= 1):
                break
            time.sleep(self.options['poll'])
        self.log(f'L4: task {work_id} {record.get("state")} tools={record.get("tools")} step={record.get("step")}')
        return work_id, record

    def ask_how(self, lang, work_id):
        """The how clip alone on a fresh turn, scored on that turn: check_agent_task, the reply naming
        its state, and the record (id, state, step) read just after."""
        how = self.options['how_clip']
        marker = self.lamp.now()
        how_at, how_us = marker, self.lamp.monotonic_us()
        self.play(self.clip(f'{lang}-{how}'))
        time.sleep(self.options['reply_wait'])
        record = (self.lamp.ask_one(REQUEST, {'op': 'work', 'id': work_id}) or {}).get('work') or {}
        scored = self.voice_score(marker, how_at, how_us, record)
        self.log(f'{lang} how: {json.dumps(scored["checks"])} reply={scored["reply"][:120]!r}')
        return scored

    # L4: the voice asks after work handed over by request.sock (#7930's voice side, no spoken yes)
    def voice_leg(self):
        how = self.options['how_clip']
        refused = self.clips_ready('L4', [f'{lang}-{how}' for lang in self.options['langs']])
        if refused:
            return refused
        legs, work_id = {}, None
        for lang in self.options['langs']:
            try:
                work_id, before = self.working_task(reuse=work_id)
            except (RuntimeError, KeyError, TypeError) as error:
                legs[lang] = {'checks': {'task_handed_over': False}, 'why': str(error)}
                continue
            legs[lang] = dict(self.ask_how(lang, work_id), work_id=work_id,
                              before={k: before.get(k) for k in ('state', 'tools', 'step')})
            # The next language on a fresh conversation.
            time.sleep(self.options['gap'])
        ok = all(all(leg['checks'].values()) for leg in legs.values())
        return self.verdict('L4', {'verdict': 'PASS' if ok else 'FAIL', 'legs': legs,
                                   'why': '' if ok else 'see legs.*.checks'})

    # L4s: the old spoken ask, yes, how (off unless --l4-spoken; never blocks)
    def spoken_leg(self):
        how = self.options['how_clip']
        names = [f'{lang}-{part}' for lang in self.options['langs'] for part in ('ask', 'yes', how)]
        refused = self.clips_ready('L4s', names)
        if refused:
            return refused
        legs = {}
        for lang in self.options['langs']:
            marker = self.lamp.now()
            self.play(self.clip(f'{lang}-ask'))
            time.sleep(self.options['readback_wait'])
            self.play(self.clip(f'{lang}-yes'))
            time.sleep(self.options['how_wait'])
            how_at, how_us = self.lamp.now(), self.lamp.monotonic_us()
            self.play(self.clip(f'{lang}-{how}'))
            time.sleep(self.options['reply_wait'])
            works = self.lamp.ask_one(REQUEST, {'op': 'work_list', 'limit': 5}) or {}
            legs[lang] = self.voice_score(marker, how_at, how_us, (works.get('works') or [{}])[0], spoken=True)
            self.log(f'L4s {lang}: {json.dumps(legs[lang]["checks"])}')
            time.sleep(self.options['gap'])
        ok = all(all(leg['checks'].values()) for leg in legs.values())
        return self.verdict('L4s', {'verdict': 'PASS' if ok else 'FAIL', 'legs': legs,
                                    'why': '' if ok else 'see legs.*.checks (kept apart: never blocks)'})

    # HW: the voice asks "how far is it?" while a long do-task still runs (en only)
    def running_leg(self):
        """A long do-task spoken (start_agent_task, then the yes). Its work record is polled until `working`
        with a tool, then the how clip plays while it runs; the record read as the how ends is what the lamp
        knew when it heard it. Scored by running_verdict."""
        leg, how, kind = 'HW', self.options['how_clip'], 'task'
        refused = self.clips_ready(leg, ['en-long-do', f'en-{how}', 'en-yes'])
        if refused:
            return refused
        marker = self.lamp.now()
        self.play(self.clip('en-long-do'))
        time.sleep(self.options['readback_wait'])
        self.play(self.clip('en-yes'))
        began, record = time.time(), {}
        while time.time() - began < self.options['running_cap']:
            works = (self.lamp.ask_one(REQUEST, {'op': 'work_list', 'limit': 10}) or {}).get('works') or []
            mine = [w for w in works if w.get('kind') == kind and (w.get('started_ms') or 0) >= marker * 1000 - 2000]
            record = mine[0] if mine else {}
            if record.get('done') or (record.get('state') == 'working' and (record.get('tools') or 0) >= 1):
                break
            time.sleep(self.options['poll'])
        self.log(f'{leg}: record {record.get("id")} ({record.get("kind")}) {record.get("state")} '
                 f'tools={record.get("tools")} step={record.get("step")}')
        if not record:
            return self.verdict(leg, {'verdict': 'FAIL', 'why': f'no work record of kind {kind} since the clip'})
        work = lambda: (self.lamp.ask_one(REQUEST, {'op': 'work', 'id': record['id']}) or {}).get('work') or {}
        how_at, how_us = self.lamp.now(), self.lamp.monotonic_us()
        self.play(self.clip(f'en-{how}'))
        at_heard = work()
        time.sleep(self.options['reply_wait'])
        after = work()
        scored = self.voice_score(how_at, how_at, how_us, after)
        verdict = running_verdict(leg, at_heard, scored['tools_lines'], scored['reply'], after, self.steps_on())
        self.log(f'{leg}: {verdict["verdict"]} {json.dumps(verdict["checks"])} reply={scored["reply"][:120]!r}')
        keep = lambda r: {k: r.get(k) for k in ('id', 'kind', 'state', 'done', 'step', 'tools', 'session', 'elapsed_s')}
        return self.verdict(leg, dict(verdict, polled=keep(record), at_heard=keep(at_heard), after=keep(after),
                                      tools_lines=scored['tools_lines'], heard=scored['heard'], reply=scored['reply'],
                                      journal_by_request=scored['journal_by_request']))

    def voice_score(self, marker, how_at, how_us, record, spoken=False):
        """The how turn scored from the journal and the session; every lamp-voice line naming a request
        since `marker` is kept, by request (the read-back, a yes taken in code, the calls)."""
        journal = self.lamp.run(f'sudo -n journalctl -u lamp-voice -o short-unix --no-pager --since @{int(marker)}', timeout=60)
        calls, by_request = [], {}
        for line in journal.splitlines():
            at = float(line.split(' ', 1)[0]) if re.match(r'^\d+(\.\d+)?\s', line) else 0.0
            found = TOOLS_LINE.search(line)
            if found:
                calls.append({'at': at, 'request': int(found.group(1)), 'calls': found.group(2)})
            request = REQUEST_LINE.search(line)
            if request:
                by_request.setdefault(request.group(1), []).append(line.split(': ', 1)[-1][:400])
        # One sudo per command: a stand-in that types the password gives it once.
        events = self.lamp.run(f"sudo -n sh -c 'S=$(ls -t {SESSIONS} | head -1); cat {SESSIONS}/$S/metadata/events-*.jsonl'",
                               timeout=60)
        said, heard = [], []
        for line in events.splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if event.get('kind') != 'transcript' or event.get('at_us', 0) < how_us:
                continue
            # As scenario.py (L4s's played reading) tells them apart: the lamp's when it has a turn and is not
            # input; else what it heard.
            if event.get('turn') is not None and event.get('role', 'output') != 'input':
                said.append((event.get('text') or '').strip())
            else:
                heard.append((event.get('text') or '').strip())
        reply = ' '.join(said[-3:])
        after_how = [c for c in calls if c['at'] >= how_at - 1]
        called = any('check_agent_task' in c['calls'] for c in after_how)
        # Work already done may be told by its answer (c0a4-5 vi): the reply carrying the record's answer
        # words counts as naming its state, and as the voice knowing it, called or not.
        told = answer_told(reply, record.get('answer'))
        checks = {
            'check_agent_task_called_or_answer_told': called or told,
            'reply_names_its_state': bool(STATE_WORDS.search(reply)) or told,
            **record_checks(record, self.steps_on()),
        }
        if spoken:
            checks['handed_over'] = any(re.search(r'(start_agent_task|confirm_agent_task)', c['calls']) for c in calls)
        return {'checks': checks, 'check_agent_task_called': called, 'answer_told': told, 'tools_lines': calls,
                'journal_by_request': by_request, 'heard': ' '.join(heard), 'reply': reply,
                'record': {k: record.get(k) for k in ('id', 'kind', 'state', 'step', 'tools', 'elapsed_s', 'answer')}}

    def drop_in_left(self):
        """Whether any of the kit's drop-ins is there: lamp-tasks' engine one or an older run's lamp-voice
        hard-asks one, under /run or /etc."""
        return self.lamp.run(f'test -e {DROP_IN} -o -e {VOICE_DROP_IN} -o -e {OLD_DROP_IN} -o -e {OLD_VOICE_DROP_IN} '
                             '&& echo there || true').strip() == 'there'

    def remove_drop_ins(self, state, label, stamp):
        """The engine drop-in and an older kit run's lamp-voice hard-asks drop-in removed, in both places (/run
        and an older kit's /etc), whatever the state says; logged. Whether any was removed."""
        lamp, removed = self.lamp, False
        there = lambda *paths: lamp.run(f'test -e {" -o -e ".join(paths)} && echo there || true').strip() == 'there'
        if state.get('drop_in') or there(DROP_IN, OLD_DROP_IN):
            lamp.run(f'sudo -n sh -c "rm -f {DROP_IN} {OLD_DROP_IN}; rmdir {DROP_IN_DIR} {OLD_DROP_IN_DIR} 2>/dev/null; '
                     f'systemctl daemon-reload && '
                     f'echo \'{stamp} #7930 work-records ROW {label}: drop-in removed\' >> {CHANGES}"')
            removed = True
        if state.get('voice_drop_in') or there(VOICE_DROP_IN, OLD_VOICE_DROP_IN):
            lamp.run(f'sudo -n sh -c "rm -f {VOICE_DROP_IN} {OLD_VOICE_DROP_IN}; rmdir {VOICE_DROP_IN_DIR} {OLD_VOICE_DROP_IN_DIR} '
                     f'2>/dev/null; systemctl daemon-reload && '
                     f'echo \'{stamp} #7930 work-records ROW {label}: lamp-voice drop-in removed\' >> {CHANGES}"')
            removed = True
        return removed

    def remove_helper(self):
        """L0's ask helper removed from the lamp's /tmp; whether it is gone."""
        self.lamp.run(f'rm -f {HELPER}', check=False)
        return self.lamp.run(f'test -e {HELPER} && echo there || true').strip() != 'there'

    # put-back
    def put_back(self):
        lamp = self.lamp
        try:
            self.lock_held('the put-back')
        except LockLost as holder:
            return self.verdict('putback', {'verdict': 'NOT RUN', 'why': f'lock not held: {holder}; the state is '
                                            'kept on the lamp for --put-back under the lock'})
        text = lamp.run(f'sudo -n cat {STATE}', check=False).strip()
        if not text:
            # No state, but a drop-in an older kit run left (/run or /etc) still goes, the units restarted on it.
            if not self.remove_drop_ins({}, self.label, time.strftime('%Y-%m-%d %H:%M')):
                self.remove_helper()
                return self.verdict('putback', {'verdict': 'PASS', 'why': 'nothing to put back (no state file)'})
            ready = self.restart(engine=None)
            left = self.drop_in_left() or not self.remove_helper()
            ok = ready['ok'] and not left
            return self.verdict('putback', {'verdict': 'PASS' if ok else 'FAIL', 'ready': ready, 'drop_in': left,
                                            'why': 'no state file; leftover drop-ins removed' if ok else
                                            'no state file; a drop-in left or not ready'})
        state = json.loads(text)
        stamp = time.strftime('%Y-%m-%d %H:%M')
        # A lamp-update round since L0 moved the `current` links (or rewrote a file): the files the units run
        # now are its build, never ours, so nothing is copied over them; our build is taken out of the old
        # release folder alone, where L0 put it in place.
        moved = [u for u, row in state['units'].items() if lamp.binary(u) != row['path']]
        lines = lamp.round_since(state.get('since', 0))
        round_ran = bool(moved or lines)
        if round_ran:
            self.log(f'put-back: a lamp-update round since L0 (links moved {moved}, journal {lines[:3]}): '
                     'the units are judged by their current links')
        for unit, row in state['units'].items():
            if lamp.run(f'sudo -n test -e {row["path"]} && echo there || true').strip() != 'there':
                continue
            if round_ran and lamp.sha(row['path']) != row['staged_sha256']:
                # Not ours: the round's own file, or the old one already; left as it is.
                continue
            # No backup: stopped before it was staged (the copy and the install are one command), so the
            # installed build must still be there.
            if lamp.run(f'sudo -n test -e {row["backup"]} && echo there || true').strip() != 'there':
                if lamp.sha(row['path']) != row['installed_sha256']:
                    return self.verdict('putback', {'verdict': 'FAIL', 'why': f'{unit}: no backup and not the '
                                                    'installed build; nothing put back, state kept'})
                continue
            if lamp.sha(row['backup']) != row['installed_sha256']:
                return self.verdict('putback', {'verdict': 'FAIL', 'why': f'{unit} backup is not the installed build; '
                                                'nothing put back, state kept'})
            lamp.run(f'sudo -n sh -c "cp -p {row["backup"]} {row["path"]}.back && mv {row["path"]}.back {row["path"]} && '
                     f'echo \'{stamp} #7930 work-records ROW {state["label"]}: {unit} put back '
                     f'{row["installed_sha256"][:12]}\' >> {CHANGES}"')
        self.remove_drop_ins(state, state['label'], stamp)
        ready = self.restart(engine=None)
        reads = []
        for _ in range(2):
            # The files the units run now, through their current links.
            reads.append({'running': {u: lamp.binary(u) for u in state['units']},
                          'sha256': {u: lamp.sha(lamp.binary(u)) for u in state['units']},
                          'engine': lamp.environment('lamp-tasks').get('LAMP_LAMP_ENGINE', ''),
                          'drop_in': self.drop_in_left()})
            time.sleep(3)
        def installed(unit, sha):
            row = state['units'][unit]
            return sha != row['staged_sha256'] and (round_ran or sha == row['installed_sha256'])
        ok = ready['ok'] and all(all(installed(u, r['sha256'][u]) for u in r['sha256']) and not r['drop_in']
                                 for r in reads)
        # The ask helper L0 left in /tmp goes last: the restart and reads above ask through it.
        ok = self.remove_helper() and ok
        if ok:
            lamp.run(f'sudo -n sh -c "rm -rf {STATE_DIR} && echo \'{stamp} #7930 work-records ROW {state["label"]}: '
                     f'put back, read twice\' >> {CHANGES}"')
        return self.verdict('putback', {'verdict': 'PASS' if ok else 'FAIL', 'ready': ready, 'reads': reads,
                                        'round': {'links_moved': moved, 'journal': lines[:5]} if round_ran else None,
                                        'why': '' if ok else 'not the installed build, a drop-in or the helper left, or not ready; state kept'})

    def all_legs(self):
        for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
            signal.signal(signum, stop_on)
        deadline = self.options.get('deadline')
        if deadline:
            signal.alarm(max(1, int(deadline - time.time())))
            self.log(f'deadline: the put-back begins by {time.strftime("%H:%M:%S", time.localtime(deadline))}')
        try:
            self.lock_held('L0')
            if self.setup()['verdict'] != 'PASS':
                return
            only = self.options['only']
            wants = lambda leg: only is None or leg in only
            skip = lambda leg: self.verdict(leg, {'verdict': 'SKIP', 'why': f'--only {",".join(sorted(only))}'})
            # The lock and a round are looked at (no_round) just before each leg that runs, never for one
            # skipped: a skipped leg costs no ssh and no wait (c0a4-12: ~35 s each).
            task, during = {}, None
            if wants('L1'):
                self.no_round()
                task, during = self.leg_on_our_build('L1', self.task_leg)
            else:
                skip('L1')
            if not wants('L2'):
                skip('L2')
            elif task.get('work_id'):
                self.no_round()
                self.leg_on_our_build('L2', lambda: self.list_leg(task['work_id'], during or {}))
            if wants('L4'):
                self.no_round()
                self.leg_on_our_build('L4', self.voice_leg)
            else:
                skip('L4')
            # The blocking legs first; L4s never blocks.
            if self.options['l4_spoken'] and wants('L4s'):
                self.no_round()
                self.leg_on_our_build('L4s', self.spoken_leg)
            elif only is not None and not wants('L4s'):
                skip('L4s')
            # HW runs only when --only names it.
            if only is not None and 'HW' in only:
                self.no_round()
                self.leg_on_our_build('HW', self.running_leg)
        except Refused as why:
            self.refused = str(why)
        except LockLost as holder:
            self.verdict('stopped', {'verdict': 'STOPPED', 'why': f'lock not held: {holder}'})
        except Rolled as where:
            self.verdict('stopped', {'verdict': 'STOPPED', 'why': f'{ROLLED} ({where}); the put-back follows'})
        except Exception as error:  # noqa: BLE001 - any fault still ends in the put-back, said why
            try:
                if self.state:
                    self.no_round()
                self.verdict('stopped', {'verdict': 'STOPPED', 'why': f'a fault: {error!r}'[:300]})
            except Stop as why:
                self.verdict('stopped', {'verdict': 'STOPPED', 'why': f'{why} at {time.strftime("%H:%M:%S")}'})
        except Stop as why:
            self.verdict('stopped', {'verdict': 'STOPPED', 'why': f'{why} at {time.strftime("%H:%M:%S")}; '
                                     'the leg under way has no verdict'})
        finally:
            signal.alarm(0)
            for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGALRM):
                signal.signal(signum, held_during_put_back)
            if self.refused:
                self.log(f'L0 refused: {self.refused}')
                self.verdicts['L0'] = 'REFUSED'
            else:
                self.put_back()
            with open(os.path.join(self.out, 'verdict.txt'), 'w') as f:
                f.write(''.join(f'{leg} {v}\n' for leg, v in self.verdicts.items()))
            self.log('verdicts: ' + ' '.join(f'{leg}={v}' for leg, v in self.verdicts.items()))
            if self.lock:
                self.lock.stopped.set()


def main(argv):
    if not argv or argv[0].startswith('-'):
        sys.exit(__doc__)
    label, rest = argv[0], argv[1:]
    options = {'tasks_bin': None, 'voice_bin': None, 'clips': None, 'put_back': False,
               'l4_spoken': False, 'only': None, 'langs': 'en,vi', 'running_cap': 150,
               'lock_name': 'lamp-c0a4', 'lock_holder': os.environ.get('LAMP_LOCK_HOLDER'),
               'lock_session': os.environ.get('LAMP_LOCK_SESSION'), 'lock_renew': 180,
               'poll': 2.0, 'cap': 240, 'readback_wait': 25, 'how_wait': 30,
               'reply_wait': 20, 'gap': 60, 'how_clip': 'how'}
    flags = {'--tasks-bin': 'tasks_bin', '--voice-bin': 'voice_bin', '--clips': 'clips', '--poll': 'poll',
             '--cap': 'cap', '--readback-wait': 'readback_wait', '--how-wait': 'how_wait',
             '--reply-wait': 'reply_wait', '--gap': 'gap', '--how-clip': 'how_clip',
             '--deadline': 'deadline', '--deadline-in': 'deadline_in', '--only': 'only', '--langs': 'langs', '--running-cap': 'running_cap',
             '--lock-name': 'lock_name', '--lock-holder': 'lock_holder', '--lock-session': 'lock_session',
             '--lock-renew': 'lock_renew'}
    while rest:
        flag = rest.pop(0)
        if flag == '--l4-spoken':
            options['l4_spoken'] = True
        elif flag == '--put-back':
            options['put_back'] = True
        elif flag in flags and rest:
            value = rest.pop(0)
            options[flags[flag]] = value if flag in ('--tasks-bin', '--voice-bin', '--clips', '--how-clip', '--deadline', '--only', '--langs', '--lock-name',
                                             '--lock-holder', '--lock-session') else float(value)
        else:
            sys.exit(f'work-records-row.py: unknown or incomplete {flag}')
    if not re.fullmatch(r'[A-Za-z0-9_.-]+', label):
        sys.exit('work-records-row.py: LABEL is letters, digits and ._- only')
    # The lock's holder and session are the runner's own, never a default: --lock-holder and --lock-session,
    # or LAMP_LOCK_HOLDER and LAMP_LOCK_SESSION.
    for key, flag, env in (('lock_holder', '--lock-holder', 'LAMP_LOCK_HOLDER'),
                           ('lock_session', '--lock-session', 'LAMP_LOCK_SESSION')):
        if not options[key]:
            sys.exit(f'work-records-row.py: {flag} (or {env}) is required: the lamp lock\'s {key[5:]}')
    # --langs en,vi (both by default): the languages L4 and L4s play and score, in that order.
    langs = {lang.strip() for lang in options['langs'].split(',') if lang.strip()}
    if not langs or langs - {'en', 'vi'}:
        sys.exit('work-records-row.py: --langs is en, vi or en,vi')
    options['langs'] = tuple(lang for lang in ('en', 'vi') if lang in langs)
    if options['only'] is not None:
        # --only L4,L4s: those legs alone (L0 and the put-back always); naming L4s turns it on.
        options['only'] = {leg.strip() for leg in options['only'].split(',') if leg.strip()}
        unknown = options['only'] - {'L1', 'L2', 'L4', 'L4s', 'HW'}
        if unknown or not options['only']:
            sys.exit(f'work-records-row.py: --only takes L1,L2,L4,L4s,HW (not {",".join(sorted(unknown))})')
        options['l4_spoken'] |= 'L4s' in options['only']
    for key in ('cap',):
        options[key] = int(options[key])
    if options.get('deadline'):
        parts = options['deadline'].split(':')
        if len(parts) not in (2, 3) or not all(p.isdigit() for p in parts):
            sys.exit('work-records-row.py: --deadline is HH:MM or HH:MM:SS')
        hour, minute, second = (int(x) for x in (parts + ['0'])[:3])
        at = time.mktime(time.localtime()[:3] + (hour, minute, second, 0, 0, -1))
        if at <= time.time() + 5:
            sys.exit(f'work-records-row.py: --deadline {options["deadline"]} is past')
        options['deadline'] = at
    if options.get('deadline_in'):
        options['deadline'] = time.time() + options['deadline_in']
    row = Row(label, options)
    if options['put_back']:
        for signum in (signal.SIGINT, signal.SIGTERM):
            signal.signal(signum, held_during_put_back)
        row.put_back()
        row.lock and row.lock.stopped.set()
        return
    for key in ('tasks_bin', 'voice_bin'):
        if not options[key] or not os.path.isfile(options[key]):
            sys.exit(f'work-records-row.py: --{key.replace("_", "-")} names no file')
    row.all_legs()
    if row.refused:
        sys.exit(f'work-records-row.py: {row.refused}')


if __name__ == '__main__':
    main(sys.argv[1:])
