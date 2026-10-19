# Test cases and the traceability matrix

How each check in the production coverage matrix
(`lampOS/production/matrix-v0.toml`, #1785) is run, and what is ready today.
Kept by `hw-first`; every limit, fixture and stop is a proposal until the
hardware lead approves it.

| File | What |
|---|---|
| `test-cases-v0.toml` | one `[[case]]` per test: the matrix checks it covers, preconditions, fixture, the agent's steps, the person's steps, the pass, the stop, the evidence, how the verdict reaches `lamp-station`'s answers, and what blocks it |
| `rtm.py` | builds the traceability matrix from the matrix and the cases, and lists the gaps |
| `traceability-v0.md` | its output: check → case → script, limit → ready or not. Generated; never edited by hand |

```sh
python3 rtm.py --matrix lampOS/production/matrix-v0.toml --check      # a mandatory check with no case fails
python3 rtm.py --matrix lampOS/production/matrix-v0.toml --out traceability-v0.md
```

## How a hardware builder runs a case

1. Take the lamp's lock, as joining.md's "Sharing a lamp" says, and claim the
   case's issue (`status:taken`).
2. Check every `pre`. A qualification case needs the person's yes at the lamp
   and the operator's stop in reach before anything moves.
3. Run the `agent` steps, and ask the person each `person` step. A question
   the person does not answer is `not_checked`, never a pass.
4. On a `stop`, end the case at once and record `interrupted`.
5. Write the verdict into the run's answers file as `answers` says, keep the
   `evidence` in the fleet record (never in this repository), and run
   `lamp-station`.

Cases run in `order`: the routine cases first (they must pass), then station
and integrated, then qualification, the long soak last.
