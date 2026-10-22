# Product photo faces through existing consent functions

Part of #2856; unit activation only, under [First’s scope and budget decision](https://github.com/autonomous-ai/lampOS/issues/2856#issuecomment-6336106600).
Moving the photo ask into an installable skill is a separate piece.
**Native integration, complete host qualification and Mobile’s authorised
lamp-0c89 acceptance remain required. This is not a device result.**

## Behavior and authority

Product Voice and Cloud set `LAMP_PHOTO_FACES=on`. Cloud uses
`/run/lampos-photos` and `/run/lampos-eyes/who.sock`; Eyes adds that photo
directory, retaining its current recogniser, people file, Face Mesh V2,
Blendshape V2 and consent configuration. Voice uses the same request socket.
The folder remains shared boot tmpfiles state, `lampos:lampos`, 0700;
no service owns it as its runtime directory. The cross-unit regression
resolves actual Cloud environment through the update parser and
`photos::Place::from`, then checks paths, ownership, setup conditions,
recogniser prerequisites and writable-folder agreement.

The model sees no additional function names. On photo-enabled sessions,
`remember_my_face` and `forget_my_face` accept explicit `photo_reply: true`
with no other argument. The question says that the face came from a photo.
The worker resolves Eyes’ actual pending face and saved asked mark; it does
not accept a provider-supplied name for photo consent. Missing/unasked
context, mixed arguments, disabled photo mode and requests not heard at
the lamp refuse without enrollment or broader deletion. Ordinary calls
still need their name and keep existing replacement/apart behavior; off
schemas remain unchanged.

A captured gap-held Second-route turn may declare and run only the narrow
pending-photo refusal form of the shared forget function. Its schema omits
ordinary forget parameters; runtime checks independently reject grants,
ordinary forget, mixed arguments and revoked requests. The normal forget
confirmation policy applies only to ordinary forgetting. Legacy photo
function dispatch remains flag-gated compatibility, never additional
model declarations. Shared and legacy paths recheck request identity and
lease authority after the pending-face lookup, before mutation.

## Contract and revocation controls

The real product child harness loads the installed core skills and complete
Actions catalog, clears inherited `LAMP_` settings and executes actual Voice
and Second-route admission. Installed/on/off configurations must all retain
the exact pre-activation lists: Live31 and Second32, all17 Actions, Dance
and Second web search. Only the already-existing `fetch_page` omission is
allowed. Both names/order and diagnostic output are pinned, so earlier
admission cuts cannot hide behind an empty link rejection list.

This census is for the recorded product configuration, not every dev-unit
profile. First owns the separate Second-route budget change. A later
upstream admission-policy change must receive its own explicit baseline
review; this activation changes no cap or priority policy.

Nine new Voice test functions cover routing, heard/off/mixed authority,
pending/asked context, Eyes refusals, ordinary forget, revocation during
lookup, the slow lookup, held Engine calls and the actual held Net request
schema. The Engine, socket and schema controls are separate layers, not a
single complete Engine-to-Eyes-store transaction. Existing photo-question
and legacy photo tests run alongside them; the product census is one new
function and the Cloud contract extends an existing function.

Independent source review found an inherited gap in retained legacy
compatibility. The extended revocation control observed a real mutation
after revocation in legacy yes/no while shared replies stopped. With the
post-lookup authority check, all four variants perform one lookup and zero
mutations. The controller services and records a wrong mutation even if the
outer wrapper returns an unavailable error, so the control checks the
side effect. Accept, read, write and completion waits are bounded.

## Validation and latency boundary

`validation.json` records tested source identities, exact commands/counts
and raw-log hashes. Raw evidence and external runners remain under
`/private/tmp/fourth-photo-product-activation-checks-20261022/`.
Repeated focused cases are not added to complete package totals.

The scoped pre-publication gate passed **29/0/0**, formatting, strict
workspace Clippy and workspace build on `26fb5cf89` / parent `06478cdc2`.
A final plain replay includes the already-qualified memory retry tests,
factory shell and documentation changes on parent `ef1eb0cf2`; all14 Rust
files and three product units remain byte-identical to that scoped gate.
The complete host and exact published-head native gates remain separate.

The initial implementation checks exposed an ordinary-forget policy
collision, Unix-socket sandbox denials and inherited nonblocking accepted
streams on macOS. The policy exception is limited to an exact heard photo
refusal; the permitted socket execution path and explicitly blocking,
bounded fixtures resolve the latter checks. The old photo-question fixture
also needed its photo mode enabled after switching to the shared calls.
Failed stages remain in the raw record.

An initial full-host run passed formatting, strict workspace Clippy,
nine focused controls and Cloud187/0/2 on its recorded older source. It was
intentionally interrupted during Update coverage to correct the legacy
revocation finding; it is not a completed full gate or a test failure.
`host-initial-interrupted/interruption.json` preserves that boundary.

The measured fast path starts immediately before the routed tool call and
ends at its return, including pending lookup and mutation through two
controlled local socket exchanges. Compare legacy/shared cold and repeated
calls in the same process; the host target is under50ms, without a flaky
loaded-CI wall-clock assertion. The slow lookup holds the response beyond
the existing300ms bound and proves that no mutation follows its timeout.
The existing mutation bound is6s; total worst-case two-RPC allowance remains
300ms+6s. No new steady-turn I/O or background poll is added. These are
synthetic protocol timings, not provider, acoustic or lamp latency.

Scoped-gate observations (milliseconds, first/repeated):

| Route and reply | First | Repeated |
|---|---:|---:|
| Legacy yes | 4.597 | 4.937 |
| Shared yes | 3.075 | 2.717 |
| Legacy no | 2.780 | 2.324 |
| Shared no | 4.892 | 2.482 |

All meet the host target; these small samples do not establish an
improvement. The held pending lookup returned at300.297ms with no mutation.

## Remaining product proof

Normal Cloud/Eyes JPEG cleanup attempts unlink; ignored unlink errors,
process interruption and durable pending-store deletion failures are not
qualified by these routing controls. Watcher cancellation is not immediate
pending deletion; existing expiry behavior remains. App/backend retention
outside these Rust sources is unverified.

First/reviewers own the ready-head merge handoff after required local/native
gates. Mobile’s authorised full-update lamp-0c89 run must show one app
selfie question and a spoken yes that keeps the face, together with the
prior recognition and immediate-answer prerequisites. Keep only build
identities, counts and times in that device receipt. Fourth has no lamp;
no SSH, install, restart, photo, face or real person data was used here.
