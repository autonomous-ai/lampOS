#!/usr/bin/python3
"""Prepare cloud credentials before the packaged keeper's normal preflight.

The service adapter executes this as lamp-keeper. Other commands pass straight
through to the upstream binary, preserving environment and process identity.
"""
import json
import os
from pathlib import Path
import sys
import tempfile

V1_CONFIG = '/root/config/config.json'
CLOUD_ACCOUNT = '/etc/lampOS/setup/backend.json'
CLOUD_KEY = '/etc/lampOS/setup/backend-key'


class Refused(Exception):
    pass


def cloud_settings(root='/'):
    """Validate credentials before touching hardware; never log their contents.

    Account matches lamp-voice's crates/cloud/src/account.rs. BackendBase and
    BackendKey follow V1's independent fallbacks in server/config/config.go.
    Keep an existing V2 account/key pair together, including rotated logins.
    """
    def path(name):
        return Path(root) / name.lstrip('/')

    def load_json(name):
        try:
            value = json.loads(path(name).read_text())
            if isinstance(value, dict):
                return value
        except (OSError, ValueError):
            pass
        raise Refused(f'cannot read valid settings from {name}')

    def string(settings, name):
        value = settings.get(name, '')
        if not isinstance(value, str):
            raise Refused(f'cloud setting {name} must be a string')
        return value

    legacy = load_json(V1_CONFIG) if path(V1_CONFIG).exists() else {}
    device_id = string(legacy, 'device_id').strip()
    account_exists = path(CLOUD_ACCOUNT).exists()
    key_exists = path(CLOUD_KEY).exists()
    for name in (CLOUD_ACCOUNT, CLOUD_KEY):
        target = path(name)
        if target.is_symlink() or target.parent.is_symlink():
            raise Refused('cloud credentials must not be symbolic links')
        if target.exists() and target.stat().st_uid != os.geteuid():
            raise Refused('cloud credentials must belong to the preparing user (root on the lamp)')
    writes = []
    if account_exists and key_exists:
        account = load_json(CLOUD_ACCOUNT)
        try:
            key = path(CLOUD_KEY).read_text().strip()
        except OSError:
            raise Refused('cannot read existing backend-key') from None
        writes.extend((name, path(name).read_text()) for name in (CLOUD_ACCOUNT, CLOUD_KEY))
    else:
        if not device_id:
            raise Refused('V1 device_id is missing; complete V1 setup before migrating')
        account = {name: string(legacy, name) for name in
                   ('device_id', 'mqtt_endpoint', 'mqtt_username', 'mqtt_password', 'fa_channel', 'fd_channel')}
        account['device_id'] = device_id
        account['mqtt_port'] = legacy.get('mqtt_port', 0)
        account['llm_base_url'] = (string(legacy, 'backend_base_url').strip()
                                   or string(legacy, 'llm_base_url').strip())
        key = string(legacy, 'backend_api_key').strip() or string(legacy, 'llm_api_key').strip()
        # A power loss between the two writes is retryable only when the
        # existing half still matches V1. Never combine unrelated credentials.
        if account_exists:
            existing = load_json(CLOUD_ACCOUNT)
            if any(existing.get(name) != value for name, value in account.items()):
                raise Refused('partial cloud account differs from V1; restore its matching key')
        if key_exists and path(CLOUD_KEY).read_text().strip() != key:
            raise Refused('partial backend-key differs from V1; restore its matching account')
        writes.extend(((CLOUD_ACCOUNT, json.dumps(account, indent=2) + '\n'), (CLOUD_KEY, key + '\n')))
        writes = [(name, path(name).read_text() if path(name).exists() else text) for name, text in writes]
    if device_id and string(account, 'device_id').strip() != device_id:
        raise Refused('existing cloud account belongs to a different V1 device_id')
    for name in ('device_id', 'mqtt_endpoint', 'mqtt_username', 'fa_channel', 'fd_channel', 'llm_base_url'):
        if not string(account, name).strip():
            raise Refused(f'cloud setting {name} is missing; complete V1 setup before migrating')
    string(account, 'mqtt_password')  # Empty passwords are allowed by Account::complete.
    port = account.get('mqtt_port')
    if isinstance(port, str) and port.strip().isdigit():
        port = int(port.strip())
    if type(port) is not int or not 1 <= port <= 65535:
        raise Refused('cloud setting mqtt_port must be between 1 and 65535')
    if not key:
        raise Refused('backend API key is missing; complete V1 setup before migrating')
    return writes


def write_private(path, content):
    """Create private files atomically; preserve any existing content."""
    target = Path(path)
    if target.is_symlink() or target.parent.is_symlink():
        raise Refused('cloud credentials must not be symbolic links')
    if target.exists():
        if target.read_text() != content:
            raise Refused(f'{target.name} changed during preparation; retry')
        os.chmod(target, 0o600)
        return
    target.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=target.name + '.', dir=target.parent)
    temporary = Path(name)
    try:
        with os.fdopen(fd, 'w') as output:
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
        # Linking, rather than replacing, cannot overwrite a concurrently
        # provisioned account. The temporary file is already mode 0600.
        os.link(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)


def prepare(root='/'):
    settings = cloud_settings(root)  # Validate everything before the first write.
    for name, text in settings:
        write_private(Path(root) / name.lstrip('/'), text)


def main(argv=None):
    argv = sys.argv if argv is None else argv
    real = Path(argv[0]).resolve().with_name('lamp-keeper-real')
    try:
        if argv[1:2] == ['prepare']:
            prepare()
        os.execv(str(real), [str(real), *argv[1:]])
    except (Refused, OSError, UnicodeError):
        # Avoid printing exception values: failed decoding can include secrets.
        print('lamp-cloud prepare: credential preparation or keeper launch failed', file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
