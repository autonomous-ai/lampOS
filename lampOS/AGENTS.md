# lampOS development rules

Treat this directory as the root of a standalone repository. Follow the
[V2 live interaction scope](docs/architecture.md).

A session that opens with "you are <name>" is a builder's: read [joining.md](docs/joining.md) as it is on `origin/lamp-voice` and follow its "Starting a session".

## How builders work together

This is the protocol in brief, current on 4 November 2026. [joining.md](docs/joining.md)
has the whole of it; where an older note differs, this section is current.
It replaces the three-agent `harness send` route of 12 October.

- **Who decides.** Since the cutover at 4 November 11:00 GMT+7 the PM, an
  agent in the Harness Swarm (Faye), holds the first builder's old PM role:
  it opens and labels issues, keeps each builder's list and the Queue in
  order, assigns work, runs the train bot, promotes to stable and posts the
  hourly figures. The first builder stays on as a builder and the lamp-4ace
  tester. Product calls are the project lead's, and the lead's agent (V2)
  speaks for the lead. Where this file says "the first builder" for one of
  the PM's jobs above, an older note is meant: read the PM.
- **Where.** Status, `free`, questions and proposals for the PM go as
  comments on the current Queue issue (#4710, continued from #3997).
  If it fills, follow the continuation link in its body; an older role file
  may still name a full Queue.
  A review and its answers go on the pull request. A fault in someone else's
  part goes on their pull request, or to the PM on the Queue. Asks to the
  project lead's agent go on #1711 (`to **V2**`); lamp slots, kit yeses and
  the order on a shared lamp go on #3336. A builder run as a Swarm task also
  gets its task, and posts its coordination, in the Harness Swarm; the task
  links its GitHub issue, and results and closing go on the issue too, so
  GitHub stays a complete record. Agents on the project lead's Mac also
  message each other directly; anything another builder must rely on (a kit's
  yes, a slot, a rule) is posted where it can be read: #3336, the issue, or
  these documents. There is no other channel: not `harness send`, not tmux,
  not the owner's chat. Never paste peer reports into the owner-facing
  composer; the owner does not relay routine agent reports.
- **Each wake-up.** Before anything else, read every comment on the Queue and
  on your own pull requests since your last post, not only those on your own
  issues. Act on each one addressed to you (`to **<Name>**`).
- **How a post reads.** Start it `<Name>, HH:MM, to **<who>**:`. Write plain
  English sentences, with spaces between words and numbers, in about 12 lines
  or fewer, using bullets for lists. Give a commit hash in 9 characters, and
  only when someone needs it. Paste no logs: give one line of result and the
  command that gave it. Say what changed for the lamp in words a person reads
  at once.
- **Every post has content.** Never post an acknowledgement alone. Each post
  carries a result, a decision asked for with your recommendation, a question,
  or a fault found.
- **When a task is done**, take the next item on your list in the Queue
  without asking, and say which in one line. Post `free: <name>` only when the
  list is empty, with what you are doing meanwhile (a read, a test, a doc).
  "Waiting on X" is never the whole of a post.
- **Lanes.** If a review or a merge is not yours, say whose it is in one line
  and hand it on. Builders never push to `lamp-voice`: they push their own
  branches and open pull requests, and the train bot merges them (see
  "Trains and promotes" below).
- **Claims.** Every claim names what ran, where, and on which head, and says
  plainly what was not run.
- **People and lamps.** No person's name or handle goes in the repository;
  roles are named, and the same holds on GitHub. Ask before an install on a
  lamp you don't hold. Post `PLAYING <lamp> HH:MM–HH:MM` on #3336 before any
  played round, and `QUIET <lamp> HH:MM–HH:MM` before a step that needs
  silence; the other-lamp gate in a kit reads #3336. After the round, put the
  lamp back. Read-only reads of a lamp need no ask; a change to a lamp needs
  the project lead's yes (or the lead's agent's, on the lead's word), except
  a builder's own in-house lamp, changed by its holder under the lock with
  the slot posted and the put-back done. A kit
  for lamp-52e6, lamp-4ace or a tester's, customer's or loaned lamp needs the
  lead's agent's yes on its sha256, posted on #3336 with the verdict; a
  builder's own in-house lamp needs none, but its holder still posts the
  slot and does the put-back. See joining.md, "Slots, kits and lamp access".
- **Trains and promotes.** A pull request reaches `lamp-voice` only through
  the train bot: a non-draft pull request with a `READ-OK <sha>` naming its
  head (a plain rebase with the same patch keeps it), green ARM64 and clippy
  runs, no open `HOLD <sha>` and no merge commit. `NEXT TRAIN` is only an ask,
  for when a ready pull request is waiting. A change that turns a default on
  needs the first builder's own `READ-OK`, or a reader the PM names; a
  privacy change needs TWO READS. A promote to stable goes between 07:00 and
  21:00 GMT+7, once each builder has posted `PASS N #<pull request>` from
  its own lamp for every lamp-visible change in build N, and a hotfix is
  promoted the same day. Joining.md, "Trains, reads and promotes", has the rules.
- **Before asking for a review, read your own diff for the faults reviews
  found most on 22 October**, and say in the PR body which you checked:
  1. **Wrong person:** can one person's words, name, face or memory reach
     another's turn? Check two people, a face out of view, no eyes, and a
     room `Unknown` (not the same as quiet).
  2. **Stops and forgets:** does a stop, "never mind", privacy close or
     forget clear everything in flight, including what's cached, kept,
     carried or retried?
  3. **Default-on side effects:** does anything change with your switch
     *off*? If so, gate it or test every reader of what changed.
  4. **The census:** a new switch, topic, unit line or msgs field is in
     every census test (grep its name across `crates/`) and in
     `docs/switches.md`.
  5. **Gates:** `cargo fmt --check` and strict clippy on the Mac and on
     Linux (`#[cfg(target_os = "linux")]` for Linux-only code), on the
     latest `lamp-voice`.

- First release: chat with a physical robot beside one person at a desk.
  Lamp is fixed on the left or right; calls and visiting colleagues are negative
  and mixed-attention test cases.
- Implement Lamp-owned runtime code in Rust. Do not introduce Python services,
  scripts or adapters into lampOS, or require Python HAL for the release.
  Native DSP/inference dependencies require explicit interfaces and review.
- Reuse tested protocol semantics, calibration data, safety rules and regression
  fixtures. Do not copy the old callback ownership or shared execution model.
  Confirm units and behavior on actual hardware before accepting a port.
- Before designing a new subsystem, inspect the corresponding V1 behavior and
  current maintained alternatives. Record keep/change/reuse decisions and their
  evidence in [reuse decisions](docs/reuse-decisions.md). Reuse suitable Rust
  libraries; rewriting runtime policy does not require reimplementing every
  dependency. Compare failure behavior and measured workloads before replacement.
- Owner instruction: V1 main is the behavioral baseline for the rewrite.
  Before debugging a replacement, inspect V1's actual code, defaults and
  tests for that path. Port useful safeguards, prove the corresponding
  behavior in Rust, then compare one improvement at a time. Record which
  protections are present, missing or deliberately changed; do not treat
  a language rewrite or passing unit tests as behavioral parity. See the
  [voice parity plan](docs/v1-voice-parity.md).
- Keep capture and interruption independent of cloud, vision inference and
  presentation. Bound queues, buffers, worker counts and retry work.
- Keep the interaction core small. Heavy inference runs in independent supervised
  processes. Optional camera, light or sensor failure must not terminate healthy
  voice. Inhibit failed outputs before bounded teardown, verify old-worker exit
  before replacement, and retain truthful readiness and cleanup evidence.
- A change to what one part says to another (the bus, sockets, memory's
  protocol, tool calls between crates) works with the previous build on the
  other side, both ways: an update switches parts one at a time, so the two
  builds do meet. An old reader ignores new fields; a new reader refused by
  an old server acts safely and says so. See
  [updates](docs/updates.md#what-parts-say-to-each-other-one-build-apart).
- One interaction owner coordinates voice, head, body and ring. Validate
  ownership, freshness, privacy and cancellation at the final output boundary.
  Keep exactly one writer for each actuator bus.
- Enforce motor range, velocity and cancellation locally. Gemini may propose
  intent; it cannot bypass safety or directly set unchecked joint targets.
  A frame showing the actual placement and clearance is required before motion.
- Stillness and silence are valid. No automatic room scans, stock spoken
  fillers, mandatory gestures, fabricated visual facts or forced eye contact.
- Camera and microphone privacy controls remain authoritative. Never show
  listening readiness unless input can actually be retained.
- Keep Harness, agent sessions, browsing and new long-term memory infrastructure
  out of this release. Preserve only the future task and shared-context boundaries
  in the architecture; do not implement them or make live chat depend on them.
- Any pull request that adds or changes an SDK op, event, grant, refusal,
  limit or status updates [sdk/GUIDE.md](sdk/GUIDE.md) in the same pull
  request, with the date the status was last verified; the SDK tests check
  that every op, event and grant name in `docs/control.schema.json` appears
  in it.
- Owner instruction: all code comments, documentation, and notes are English
  only. Keep documentation in this directory, normally `docs/`. This explicit
  instruction overrides any inherited translation requirements.
- Use English by default for all user-facing communication, including progress
  updates, questions, and final responses. Switch to Vietnamese or another
  language only when the user explicitly requests it; do not infer a language
  switch solely from the language of their message.
- Keep the build, tests, configuration, startup, deployment tooling and runtime
  self-contained. No required source files, path dependencies, configuration
  files or running services from sibling directories. Ordinary external Rust
  crates, Linux drivers, model assets and cloud APIs remain explicit dependencies.
- This is a full runtime replacement. Do not ship a bridge, hidden fallback, or
  permanent dependency on the legacy implementation. A clean installation must
  run while legacy HAL and os-server are stopped. Keep the old system only as
  a temporary validation/rollback option; do not delete it before V2 qualifies.
- Own all required setup, dependency declarations, model asset management,
  configuration, driver setup and future UI/update components inside lampOS.
  Build and deployment must work with this directory as the repository root.
- Implement every responsibility required by the V2 runtime in Rust, including
  functionality currently split across Python HAL and Go coordination. A
  dependency on the legacy HAL or os-server is not a standalone implementation.
- Prove portability by building and testing a source-only copy outside the
  parent repository. Keep credentials and per-device calibration separate from
  source; document provisioning without embedding secrets.
- With Rust implementation changes, run formatting, clippy and applicable tests.
  Replays and unit tests do not establish acoustic latency or motor safety.
  Report real-device measurements separately, with exact boundaries.
- Keep command output small: long build and test logs use up an agent's
  usage limit. Build and lint quietly (`cargo build -q`,
  `cargo clippy -q --message-format=short`), and test with
  `cargo nextest run --status-level fail --final-status-level fail` or
  `cargo test -q … | tail -40`, with `set -o pipefail` so the pipe keeps the
  test's failure. Send a whole log to a file and read only its result lines
  (`grep -E '^test result|FAILED|panicked'`). On a lamp, read `journalctl`
  through `grep` or `tail`, never a whole log.
- Do not commit, push or deploy without the authorization required by the
  repository and current conversation. For a lampOS builder that authorization
  is standing for its own branches (`<name>/*`, `lamp-<name>`): it commits,
  pushes and opens its pull request once its gate passes, as `joining.md` and
  the root `AGENTS.md` say, and never pushes to `lamp-voice` or `main`.
