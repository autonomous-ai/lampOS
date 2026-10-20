# Friday's work log

Friday works on `lamp-friday`, with one `friday/<task>` branch per task. The
running list belongs to the first builder (the Queue issue, #1722). Friday's
lamp is `lamp-06c1` (blue-lamp), shared with el and jarvis; a result on it
says so, and any other is not a result on a lamp.

## Proposed tasks

| Id | Proposal | Answer |
|----|----------|--------|

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
