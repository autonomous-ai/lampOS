# Friday's work log

Friday works on `lamp-friday`, with one `friday/<task>` branch per task. The
running list belongs to the first builder (the Queue issue, #1722). Friday's
lamp is `lamp-06c1` (blue-lamp), shared with el and jarvis; a result on it
says so, and any other is not a result on a lamp.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|
| friday-P1 | Memory: take from other systems after #2176: finish #2291's hits (gate, Vietnamese, bytes told), replace-by-link for kept facts with link-only-what-was-used (finding 14), "kept" only from the store's answer, since/until from `replacements`, an embeddings bench before any build (#1722, 21 Oct 13:0x) | |

## Found, not mine

## 2026-10-20: joined

Ready for lamp-voice: no

- Role file `lampOS/builders/friday.md` made from the template, on
  `lamp-friday` from `origin/lamp-voice` at `aa81b2531`. No `builder:friday`
  issue yet; Friday takes none until the first builder has taken the file
  onto `lamp-voice` and labelled one.
- Its person asked for one task, a review of the lamp's memory, and asked
  that Friday be given nothing else until it is done. Asked of the first
  builder on #1722.
- A first read-only pass of `crates/memory` and `crates/voice/src/remember.rs`
  is done on the Mac, from `f524712bf`. No lamp was touched. Its findings
  are posted on #1722 for the first builder and twelve, and go into the
  review once the issue exists.

## 2026-10-20: #2176 taken (the memory review)

Ready for lamp-voice: no

- The first builder opened #2176 at 12:1x +07, labelled `builder:friday`,
  as Friday's one task. Taken at 12:15 (`status:taken`, session
  `03bc4e44`), the first "taken" comment.
- Rechecked at lamp-voice `be278c7da` and #1712's `9ad9416ad`: findings 1
  (`journal_inhibited` never cleared) and 2 (linked kept facts expire, the
  expiry at `days.rs:337`) still hold on both.
- Order: finding 2 first (a test, then the fix: no shipped lamp makes such a
  fact yet), finding 1's plan posted on #2176 for Twelve and Fourth before
  any code, then 3 and 4. lamp-06c1 waits for El's release after the
  re-set-up.

## 2026-10-20: #2176 rows 2, 1a and 4d up

Ready for lamp-voice: yes (#2198, #2206, #2197)

- Row 2, #2198 (`961b6d487`): expiry keeps a kept fact whose conversation
  expired. Off unless set (`LAMP_MEMORY_FORGET_ALL`). lamp-memory 230
  passed, 0 failed, 5 ignored on the Mac (Homebrew 1.99, not 1.96). The
  reviewer read it with no finding.
- Row 1a, #2206 (`2f4ed31a3`): the voice's journal says when memory's
  journal stopped, why, and how many exchanges it held back. No behaviour
  change. lamp-voice lib 2476 passed, 12 failed (#739's timing groups,
  73 passed on a rerun by group), 13 ignored.
- Row 4d, #2197 (`589861921`): five memory docs agree with the code.
- Clippy under 1.99 trips lints new in 1.99, in untouched crates; each PR's
  ARM64 and clippy runs are the 1.96 gate.
- Not shown on a lamp yet: lamp-06c1 is El's until about 14:00.

## 2026-10-20 evening: #2176 rows, lamp checks, the probe

Ready for lamp-voice: yes (#2298, #2277, #2319)

- Merged today: #2197 (docs), #2198 (row 2), #2206 (row 1a), #2241 (lamp
  readings), #2279 (probe-tools through the relay), #2282 (row 3r), #2299
  (row 10).
- Open: #2298 (row 11, ready), #2277 (row 6, the first builder's to merge:
  the privacy path), #2319 (row 5a, Live only; the second route reads no
  faces between turns: asked on #2176).
- Reviews of twelve's #2226 (rows 1b/1c, 13): five reads; the blocking
  Live P2 and row 13 are fixed; the call sites stay untested (P3).
- Finding 9 was built twice (#2233 by sixth landed): dropped mine. Now a
  duplicate check (open PRs, branches, #1722) comes before every row.
- lamp-06c1, second route, build 4142: today, correction and a used
  preference pass by voice; "never mind" forgot the wrong kept thing (row
  6); journalling stopped after a forget (row 1). Readings:
  docs/verification/memory-review-lamp-06c1-20261020/ (#2241).
- probe-tools on lamp-06c1, 10 runs through the relay: preference-used
  8/10, correction-first 0/10, correction-kept 10/10, earlier-today 10/10,
  earlier-today-said 0/10, never-mind 10/10; "what do you remember about
  …" called recall 0/10. #2063 targets the two 0/10 arms.
- Row 3 waits on the first builder's choice: the eyes have no stable
  person key.
- The Mac reached load ~400 with six agents running full suites: now
  targeted tests only, ARM64 is the full gate.

## 2026-10-21 night: every #2176 row of mine merged

Ready for lamp-voice: yes (all merged)

- Merged: #2197, #2198, #2206, #2241, #2277 (row 6), #2279, #2282 (3r),
  #2298 (11), #2299 (10), #2319 (5a, Live only), #2330 (row 3: stable
  person id; a remove forgets the by-name id only when no other record
  holds it).
- Open for #2176: lamp-06c1 re-test with the person (previous-day recall
  of 20 Oct room facts; the cases with the new switches), the probe after
  #2063 once it has #2279, twelve's #2226.

## 2026-10-21: the lamp re-tests, the row, friday-P1

Ready for lamp-voice: yes (#2446 waits behind #2439)

- lamp-06c1, 4576 (room's scope): the previous-day recall shown by voice for the first time (yesterday's entries found); remember-once, forget-about and the journal-stop line pass; "green now" answered but never kept.
- lamp-06c1, #1712 4510 (with #2233), its person at the lamp: per-person scope shown (every op `p-`); the observation's day 1 kept 2 threads for the person, 0 for the room. Finding 14: a forget reaches later turns and observations linked to the forgotten thing.
- Row "Remembers what matters" (#1693): part 1 #2446 (re-ask the note after 10 quiet minutes); part 2 observe-dry on 20 synthetic conversations through the relay: 10/10 kept right, 0 wrong, 0 intrusive; the real-store run declined by the person.
- friday-P1 posted on #1722, from the architecture review with the person.
