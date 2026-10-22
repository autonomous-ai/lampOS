# The lamp skill creator

A Claude Code skill (`SKILL.md` and `references/`) that makes physical
skills for an Autonomous lamp: "Lamp, learn to ..." hands the request to
Claude Code with this skill (#941, `lampOS/docs/skills.md`).

Install it into Claude Code, on the computer that takes the lamp's tasks:

```sh
mkdir -p ~/.claude/skills
cp -r lampOS/skill-creator ~/.claude/skills/lamp-skill-creator
```

It adapts V1's `skill-creator` (`skills/skill-creator` in autonomous-os):
the same process (capture what is wanted, draft, test, iterate), taught the
lamp's topics and services, `skill.toml`, tests on the lamp's own bus
recordings and installing to a lamp. V1's Python evaluation scripts are left
out: lampOS is Rust only, and a lamp skill is tested by replaying the lamp's
recordings through the runtime (`lamp-skills`, #938).

## Making one yourself, without Claude Code

A developer at their own computer makes a skill the same way, by hand. The
references are the whole of it; read them in this order.

1. **What the lamp can sense and do:**
   [references/senses-and-actions.md](references/senses-and-actions.md). A
   skill uses only what is listed there.
2. **The folder:** [references/skill-folder.md](references/skill-folder.md).
   That is `skill.toml` and `SKILL.md`, with `kind = "installable"`, a
   `version`, the parts it `needs` (eyes, body, light, ...) and the
   `[permissions]` it asks for. The installable skills in
   `lampOS/skills/` are worked examples, such as `sip-of-water` and
   `stand-up`.
3. **Its tests:** [references/testing.md](references/testing.md). That is
   cases in `tests/`, each with recordings and `expect.toml`. Build the
   runtime from this repository and run them. From `lampOS/`,
   `cargo build -p lamp-skills` puts it at `target/debug/lamp-skills`; put
   that on your `PATH`, or give its path, for the commands below.

   ```sh
   lamp-skills test my-skill
   # my-skill: 9 cases pass, 0 have no recording yet
   ```

4. **A package** for another lamp or the Skills Store:

   ```sh
   lamp-skills pack --out . my-skill
   # packed ./my-skill-1.0.0.tar
   ```

   `pack` refuses a skill that doesn't load, says no version, or is `core`.
   The `.tar` holds the folder and `lamp-skill.json`, which names the skill,
   its version and every file with its size and SHA-256. An install checks
   each one first. A lamp installs from the Store only a package signed with
   the update channel's key (`pack --key-file` signs with it), so a package
   you make yourself is unsigned. How a package is sent to the Store and
   reviewed is the Store's side (#2802), not this repository's.
5. **On a lamp:** [references/installing.md](references/installing.md). That
   means `lamp-skills install` with the folder or the `.tar`, by key over
   ssh, and `lamp-skills list` to see it there. It is live within 2 s, with
   nothing restarted, and `lamp-skills remove NAME` takes it off. A package
   installed by hand from a file is not held to a signature, and `install`
   says so.

To try `install`, `list` and `remove` without a lamp, give them a folder of
your own with `--dir`:

```sh
mkdir -p try
lamp-skills install --dir try my-skill-1.0.0.tar
lamp-skills list --dir try
lamp-skills remove --dir try my-skill
```
