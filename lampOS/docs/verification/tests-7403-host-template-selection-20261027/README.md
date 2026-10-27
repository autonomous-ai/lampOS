# 7403 linked-installation template refusal: read-only audit

The exact observed error comes from `round::plan` (round.rs 2910–2924), after
`carried_units` chooses a template. The signed product Actions unit is valid:
`/opt/lampOS/actions/current/lamp-actions`. The same signed manifest also carries
`lamp-52e6.lamp-actions.service`, pinned to `/opt/lampOS/actions/a90e…/lamp-actions`.
Selection unconditionally prefers hostname-specific entries (1284–1297), suppressing
the valid product unit. A 7071 linked Actions installation then rejects that pinned
text with the exact recorded error. This is a selection/layout mismatch, not a
bad product ExecStart. It occurs during the authenticated dry self-confirmation,
so the new updater remains unconfirmed and parts remain 7071.

Every one of the 54 signed units was checked: 26 product templates use current;
all 12 lamp-52e6 and 16 lamp-4ace templates are pinned. All 28 hostname entries have
product counterparts. The 12 52e6 entries all conflict with linked installation
layout; Voice additionally names legacy `/releases/` rather than product `/voice/`.
Full signed hashes/programs and the collision matrix are in audit.json. The
selection and same refusal already exist in offered 4fd; this is not uniquely a
f9 regression. The unit-only rehearsal had no competing hostname template.

The existing precedence test (`product_carrier.rs:70`) uses linked Eyes plus a
hostname Eyes fixture that is ALSO linked. The pinned-protection test (`:126`)
checks the other direction, without a competing hostname entry. Missing is the
real signed pair: linked product + legacy pinned hostname on a linked install.

Proposed narrow fix, requiring product authorization: choose authenticated
candidates compatible with installed layout before precedence. A valid matching
linked hostname still wins; a legacy pinned hostname must not suppress the product
current template on a linked install. Pinned installs keep their pinned behavior.
Keep the final folder/binary/current validator, signatures/hashes and all normal
quiet/watch/fallback checks unchanged. Add the mixed-layout regression in both
manifest orders, compatible linked-host precedence, pinned preservation, wrong
linked folder/binary refusal, corruption refusal, and dry self-confirmation using
the mixed signed envelope. Do not silently bypass corrupted selected content.

Fallback source is unchanged byte-for-byte across 7071/aa9, 4fd and f9 (`own.rs`
SHA dd6f7a14…). A nonbusy back callback restores the previous link/refuses the new
binary/removes self after 600s, then main::back restarts updater without blocking.
The fixed timer has a 120s period and 30s AccuracySec; 600s is eligibility, not an
exact callback time. The last receipt at 15:28:56 was about 674s after the climb,
which may precede the next scheduled callback. A held flock also returns Busy.
Actual self timestamp, timer Last/Next, helper hash and back logs/result are needed
to explain continued pending; this audit does not invent a fallback failure.

No product/test edits, builds, runtime, Git refs, SSH or device actions occurred.
The original production HOLD and earlier failed snapshots remain unchanged. No
fresh snapshot/archive, manual installation or trust/quiet bypass is claimed.
