# Installing a skill on a lamp

The lamp's skills live in `/var/lib/lampOS/skills/<name>/`. The runtime
(`lamp-skills`) reloads a folder when it changes: a skill is live at once,
with no restart, and gone at once when removed.

## Before touching the lamp

- Ask the owner. Say which skill, and what it will do (what will move, light
  up or speak) and when.
- Check the skill's tests passed ([testing.md](testing.md)), or say plainly
  that it is untested.
- Tell the owner what the runtime does not do yet: until the Policy's gate
  (#939), **a skill acts even while the lamp is paused**, and keeps out of a
  conversation only if it has a `[[while]]` on `voice/state`. Install only if
  they accept that.

## Install

From the computer with the skill folder, to the lamp by its name (`lamp-xxxx.local`),
by key, never by password:

```sh
scp -r stretch-break orangepi@lamp-xxxx.local:/tmp/
# Checked, loaded beside the live folder, then moved in whole: the runtime
# never sees half a skill, and one that does not load is never live.
ssh orangepi@lamp-xxxx.local '
  sudo /opt/lampOS/skills/current/lamp-skills install /tmp/stretch-break &&
  rm -r /tmp/stretch-break'
```

`/opt/lampOS/skills/current/lamp-skills` is a product lamp's; a lamp set up
by hand runs the one its unit names (`systemctl cat lamp-skills`).
`install` says what it installed and what the skill may use, or why not
(the skill does not load, says `kind = "core"`, would take a core
skill's place, or has a `tool` name another skill on the lamp has), and
takes a package (`lamp-skills pack`) the same way. It replaces an
installed skill of the same name with whatever version it is given, older
or none too, and deletes what an install or remove that was killed left.
`lamp-skills list` shows every skill on the lamp, its kind, its version and
what it may use.

Both end with how many functions the skills give the model: a session takes
32, the voice's own and the actions part's first (30 on a product lamp), and
past 32 the last skills' are dropped unless the voice runs with
`LAMP_USE_SKILL=on` (#1696), so tell the owner if a new tool would not reach
the model. On a lamp whose `lamp-skills` has no `install` yet, copy
the folder beside the live one under a name beginning with a dot (the
runtime skips it), then move it in whole.

From a Mac, never let it add its `._NAME` files: `scp -r` as above adds
none, and a copy by `tar` needs `COPYFILE_DISABLE=1`:

```sh
COPYFILE_DISABLE=1 tar -C skills -cf - stretch-break | ssh orangepi@lamp-xxxx.local 'tar -C /tmp -xf -'
```

(The runtime skips any file whose name begins with a dot, `._NAME` among
them, but a clean folder is the one to install.)

The runtime looks at the folder every two seconds and loads every skill again
when a file other than a test changed; cooldowns are kept across a reload.
Until the Policy's gate is built (#939), a lamp runs `lamp-skills` with
`LAMP_SKILLS_ASK=skip` or nothing a skill starts is done.

Then check it loaded: the runtime writes what it loaded, and why it refused
a skill (a key it does not know, a topic that does not exist), to its journal.

```sh
ssh orangepi@lamp-xxxx.local 'journalctl -u lamp-skills --since "1 min ago" --no-pager'
```

Fix a refusal there, not by guessing. `skills/events` says what a skill did
(triggered, asked, answered, done, with why), not whether it loaded.

## Turn it off, or remove it

- Turning a skill off by voice ("Lamp, stop doing ...") is not built yet: the
  runtime has no off switch for one skill, so tell the owner to ask for it
  to be removed.
- To remove it: `sudo /opt/lampOS/skills/current/lamp-skills remove <name>`.
  A core skill (one that comes with the lamp) can't be removed.

## Never

- Never install without the owner's yes.
- Never change another skill's folder, or the lamp's own parts.
- Never leave a half-copied folder: copy to `/tmp` first, then move it in whole.
