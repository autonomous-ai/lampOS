# Third builder's work log

Third works on `lamp-third`, with one `third/<task>` branch per task.
The running list belongs to First on `origin/lamp-voice`.
All measurements below are from the development machine unless a different
machine is explicitly named. Third has no lamp access.

## 2026-10-14 — expression — taken

- Task: 4, Expression: one part that authors what the lamp shows.
- Base: `origin/lamp-voice` at `1e1fb807a3486944cf55298cf3b1c2640b0cc3eb`.
- Branch: `third/expression`, based on `lamp-third` after this claim.
- Scope: new `crates/expression`, its bus types in `crates/msgs`, workspace
  membership, clock-driven tests and replay fixtures, and English documents.
  The body and voice integration remains with First. Outputs will require
  explicit enablement and expire; the existing lamp behavior stays in place.
- Host identified so far: Darwin 25.6.0, x86_64. The repository pins Rust
  1.96.0; installation and the unchanged workspace build are the next checks.
- Latency targets to test with an injected clock: interruption light request
  within 100 ms, body transition about 350 ms; return requested before the
  scheduled first sound; pause nod suppressed when speech resumes. Hardware
  lag and how any motion looks remain lamp-only checks.
- Questions: none yet. Any missing input contract will be recorded here with
  a proposed interface and its limitations.

## Not shown

No task is finished, no lamp was accessed, and no device behavior or timing
has been measured by Third.
