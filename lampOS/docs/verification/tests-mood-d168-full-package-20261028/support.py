"""Current private control helpers. No command runs on import."""
import datetime
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import time
from types import SimpleNamespace


def progress(value):
    try:
        print(value, flush=True)
    except BrokenPipeError:
        pass  # Raw command receipts decide success; a closed progress pipe cannot erase them.


def sha(data):
    return hashlib.sha256(data).hexdigest()


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def write_json(path, data):
    temporary = path.with_name(path.name + '.tmp')
    with temporary.open('w') as stream:
        json.dump(data, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)
    directory = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def aggregate(wt, source_map, source, expected_source):
    digest = hashlib.sha256()
    for entry in source_map:
        data = (wt / entry['path']).read_bytes()
        expected = expected_source if entry['path'] == source else entry['sha256']
        assert sha(data) == expected, entry['path']
        digest.update(entry['path'].encode() + b'\0')
        digest.update(len(data).to_bytes(8, 'big'))
        digest.update(data)
    return digest.hexdigest()


def observe_disk(record, folder, target, reserve):
    """Measure both owned filesystems; a low sample is never a successful gate."""
    samples = []
    for path in (folder, target):
        free = os.statvfs(path)
        samples.append(free.f_bavail * free.f_frsize)
    available = min(samples)
    record['disk_samples'] = record.get('disk_samples', 0) + 1
    record['disk_last_available_bytes'] = available
    record['disk_minimum_available_bytes'] = min(
        record.get('disk_minimum_available_bytes', available), available)
    if available < reserve:
        record['disk_reserve_breached'] = True
        record['stop_reason'] = 'disk reserve'
    return available


# Reuse the reviewed WNOWAIT closer; importing it executes definitions only.
_POLICY = json.loads((Path(__file__).resolve().parent / 'plan.json').read_text())
_CLOSER_PATH = Path(_POLICY['reserved_closer_path'])
_CLOSER_SHA256 = '468ea6f8b4ce19e58c977014f7f015d12bc99e02ef158498e7c2fd2191a57efc'
assert sha(_CLOSER_PATH.read_bytes()) == _POLICY['reserved_closer_sha256'] == _CLOSER_SHA256
_spec = importlib.util.spec_from_file_location('carrier_reserved_closer', _CLOSER_PATH)
Q = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(Q)
OWNED_OPEN = Q.OWNED_OPEN


def observed_exit(process):
    return os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)


def run_command(argv, cwd, env, folder, label, timeout):
    assert not OWNED_OPEN, "Previous owned group unresolved; compiler lease remains held"
    stdout = folder / (label + '.stdout.txt')
    stderr = folder / (label + '.stderr.txt')
    record = {'command': argv, 'cwd': str(cwd), 'started_at': now(),
              'timeout_seconds': timeout, 'stdout': stdout.name, 'stderr': stderr.name,
              'timeout_cleanup': [], 'timed_out': False,
              'disk_reserve_breached': False, 'stop_reason': None,
              'process_group_scope': 'Owned group only; no detached-descendant cleanup claim.',
              'group_closure_method': 'WNOWAIT leader reservation and ps group inventory before reap',
              'launch_state': 'not_yet_attempted'}
    progress(json.dumps({'event': 'start', 'label': label, 'folder': str(folder)}))
    started = time.monotonic()
    process = None
    disk_target = None
    operational_reserve = None
    next_disk_check = None

    def exited_unreaped():
        return observed_exit(process)

    def finish_owned(request_stop):
        if process is None:
            return
        if process.pid in OWNED_OPEN:
            assert OWNED_OPEN[process.pid] is process
            Q.close_owned(process, SimpleNamespace(observed_exit=observed_exit),
                          record['timeout_cleanup'], request_stop=request_stop)
        assert process.returncode is not None and process.pid not in OWNED_OPEN
        record['direct_wait_status'] = process.returncode
        record['exit_code'] = process.returncode
        # Closure was inventoried while the unreaped leader reserved the PGID.
        # Never probe or signal a numeric PGID after reaping the leader.
        record['owned_group_absent_after_cleanup'] = True
        record['unexpected_owned_descendants'] = any(
            event.get('action') == 'unexpected descendants after leader exit'
            for event in record['timeout_cleanup'])
        if record['unexpected_owned_descendants']:
            raise RuntimeError('Unexpected descendants after leader exit; owned group closed, stage failed')

    try:
        with stdout.open('xb') as out, stderr.open('xb') as err:
            if True:  # Apply the allocated disk policy to direct inventory/runtime too.
                policy = json.loads((Path(__file__).resolve().parent / 'plan.json').read_text())
                minimum = policy['minimum_free_bytes']
                assert isinstance(minimum, int) and minimum > 0, 'Current disk policy is unbound'
                operational_reserve = policy['operational_reserve_bytes']
                assert type(operational_reserve) is int and operational_reserve == 2 * 1024**3 + 64 * 1024**2
                assert minimum >= operational_reserve, 'Admission minimum is below the operational reserve'
                disk_target = Path(policy['target'])
                record['disk_reserve_bytes'] = operational_reserve
                record['disk_sample_interval_seconds'] = 1
                record['disk_paths'] = [str(folder), str(disk_target)]
                record['available_bytes_before_Cargo'] = observe_disk(
                    record, folder, disk_target, operational_reserve)
                assert record['available_bytes_before_Cargo'] >= minimum, 'Insufficient disk before Cargo'
                next_disk_check = time.monotonic() + 1
            record['launch_state'] = 'attempting_Popen'
            write_json(folder / (label + '.json'), record)
            process = subprocess.Popen(argv, cwd=cwd, env=env, stdout=out,
                                       stderr=err, start_new_session=True)
            OWNED_OPEN[process.pid] = process
            record['owned_pid'] = record['owned_process_group'] = process.pid
            record['launch_state'] = 'Popen_recorded'
            write_json(folder / (label + '.json'), record)
            while not exited_unreaped():
                now_monotonic = time.monotonic()
                if next_disk_check is not None and now_monotonic >= next_disk_check:
                    observe_disk(record, folder, disk_target, operational_reserve)
                    next_disk_check = now_monotonic + 1
                    if record['disk_reserve_breached']:
                        break
                if now_monotonic - started >= timeout:
                    record['timed_out'] = True
                    record['stop_reason'] = 'stage deadline'
                    break
                time.sleep(0.02)
            finish_owned(request_stop=record['timed_out'] or record['disk_reserve_breached'])
    except BaseException as error:
        record['controller_exception'] = repr(error)
        raise
    finally:
        try:
            if process is not None and process.pid in OWNED_OPEN:
                finish_owned(request_stop=True)
        except BaseException as cleanup_error:
            record['cleanup_error'] = repr(cleanup_error)
            raise
        finally:
            record['unresolved_owned_pids'] = sorted(OWNED_OPEN)
            record['compiler_lease_held'] = bool(OWNED_OPEN)
            if OWNED_OPEN:
                record['owned_group_absent_after_cleanup'] = False
            record['wall_seconds'] = round(time.monotonic() - started, 6)
            record['finished_at'] = now()
            for name, path in [('stdout', stdout), ('stderr', stderr)]:
                if path.exists():
                    with path.open('rb') as stream:
                        os.fsync(stream.fileno())
                        data = stream.read()
                    record[name + '_sha256'] = sha(data)
                    record[name + '_bytes'] = len(data)
            # Raw launch/exit/streams are durable before caller's source/parser guards.
            write_json(folder / (label + '.json'), record)
    progress(json.dumps({'event': 'finish', 'label': label,
                      'exit_code': record.get('exit_code'),
                      'wall_seconds': record['wall_seconds']}))
    return record


def require_success(record):
    assert not record.get('unresolved_owned_pids') and not record.get('compiler_lease_held'), record
    assert not record.get('unexpected_owned_descendants'), record
    assert not record.get('disk_reserve_breached'), record
    assert not record.get('timed_out') and record.get('exit_code') == 0, record
    assert record.get('direct_wait_status') == 0 and record.get('owned_group_absent_after_cleanup') is True


def inventory(record, folder):
    require_success(record)
    rows = (folder / record['stdout']).read_text().splitlines()
    return sorted(row.removesuffix(': test') for row in rows if row.endswith(': test'))
