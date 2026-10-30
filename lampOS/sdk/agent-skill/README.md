# lampos-sdk: an agent skill for programming a lampOS lamp

`lampos-sdk/` is a skill for coding agents: with it, an agent asked to
"program the lamp" knows the lampos SDK (Python, JavaScript), lamp-mcp, the
lamp's rules, ready patterns and the gotchas, and tests against the fake lamp.

Install it in Claude Code as a plugin (it also adds lamp-mcp's tools when
`lamp-mcp` is on your PATH):

```sh
claude plugin marketplace add ./lampOS/sdk/agent-skill
claude plugin install lampos-sdk@lampos
```

or as a plain skill: copy `lampos-sdk/` to `~/.claude/skills/` (or a repo's
`.claude/skills/`). Other agents: point them at `lampos-sdk/SKILL.md`.

`sdk/python/tests/test_agent_skill.py` keeps the skill naming every public
call, tool, event, state and error code the SDKs and lamp-mcp have, and runs
its scripts against the fake lamp. See `docs/develop.md`.
