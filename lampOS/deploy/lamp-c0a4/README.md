# lamp-c0a4, skill-builder's lamp, a product lamp on stable

`lamp-c0a4` is the in-house lamp of the builder `skill-builder`
([its role file](../../builders/skill-builder.md)), held by its person
since 2026-11-26. It was twelve's and thirteen's from 2026-10-19
(`lampOS` build 3162, installed by hand from a test-signed channel) until
its first person retired it on 23 Nov; the new person set it up again
through the app, so it is now a product lamp on the stable channel, with
nothing installed by hand. [LAMP.md](LAMP.md) is its owner file, as
[joining.md](../../docs/joining.md) asks.

## What it runs (2026-11-26, 14:40 +07)

- **Build:** stable **16102** (`lamp-voice` `fee90fad10e9`), written by
  `lamp-update` from 15266. `state.json` names two earlier candidates it
  refused (11913 and 13039) with "lamp-voice.service ended and systemd is
  starting it again", which is the updater holding back while voice
  restarts, not a fault of the lamp.
- **Every part's `current` link** (`/opt/lampOS/<part>/current`, the short
  build id): actions 8436d03cc87f, addons 28643cfb5b7b, air 8e6bf671cf12,
  apps-manage f26b3c726a8a, body e8eea1812641, cloud f37243393c13, control
  b0e7da4bbeae, controls 8a6b138613ab, ears ab9b00a8e61f, expression
  e48eff7bb24c, eyes ee7c50be46bd, health 3b25842a395c, memory f69f99f5db62,
  mood 414021857970, name 1b44b061736c, npu bafca85b4d2f, peek 7437abb8f5aa,
  policy 0beb14b38ee2, records 450849324fe2, schedule a951422df5ac, seen
  5a101767a972, selftest d5d725e5407b, services ae9ad3fa8f4a, setup
  d381f45c4643, skills 57637be08267, tasks aa2dbdd38a0f, update 7a7fa06b6dda,
  voice 4a7838265bbf.
- **One unit down:** `lamp-records.service` ("Bound closed lamp voice
  diagnostic sessions") failed at 14:00 +07 with exit code 2
  (`INVALIDARGUMENT`) and systemd did not start it again. Everything else
  under `lampos.target` was active. Worth a look by whoever owns records;
  it is not touched here.
- **The board:** aarch64, hostname `lamp-c0a4`, on the person's Wi-Fi
  (`lamp-c0a4.local`).
- **No lock on it:** `lamp-lock.sh lamp-c0a4 status` showed none. The lamp
  is skill-builder's alone; the lock is still taken for every install, so a
  second session of the same builder cannot cross the first.

## What this folder holds

- [LAMP.md](LAMP.md): the owner file, three lines.
- This README.

## How it was read

One ssh session on 2026-11-26 at 14:40 +07, as `orangepi`, by key (the key
was put on the lamp once, with the person's password, and the password is
not used again), read-only: `hostname`, `uname -m`, `readlink
/opt/lampOS/*/current`, `systemctl status lamp-records.service`,
`journalctl -u lamp-records -n 3`, `sudo -n cat
/var/lib/lampos-update/state.json`, `tools/lamp-lock.sh lamp-c0a4 status`.
`sudo -n` works on this lamp without a password. The lamp was not heard,
seen or touched: no played line, no servo read, no unit restarted, nothing
written.

## What was not shown

- The body's poses (`lamp.env`) were read but are not copied here yet;
  they come with the first row that needs them.
- `switch-drift.sh` was not run; the lamp runs stable as written, with no
  `/run` drop-in.
