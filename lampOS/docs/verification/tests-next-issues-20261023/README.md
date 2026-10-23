# Read-only next-work audit

Source frozen for this audit: `origin/lamp-voice`
`b7700b7c268c0c22e26c1c88c9e9ee359cddf550`. V1 comparison where relevant:
`origin/main` `1b5f7a78b86a0aac64a87950100e09d1bda6e42b`.
Read all bodies and comments of #2839, #2848 and #2862; each has zero comments
and the builder:tests label. Raw API responses are beside this receipt.
No claims, code changes, Cargo, tests, device commands or GitHub writes.

## #2839 — say the sign-in code once, repeat only on request

Existing implementation:

- `crates/tasks/src/sign_in.rs:243` records the code given by a start;
  `view` at 389 exposes `code_told`. `tasks/src/runtime.rs:1088` retains one
  open session, marks its current code before returning a start result.
- `crates/voice/src/sign_in.rs:260` tells the model to call
  `sign_in_computer` again only when asked for the code, never from memory.
  `run` polls a starting sign-in; `news` at 524 suppresses code news when
  `code_told` is true or that code-news key was already offered.
- Existing Voice tests at 1072 and 1318 cover separate tool/news responses,
  switches, heard requests, and code-news suppression; Tasks runtime test
  at 1529 checks that another start returns the same code and starts only one
  session. These do not count actual code delivery through a stand-in Harness
  followed by a spoken repeat request.
- `tasks/tests/fixtures/harness_cli.rs:104` has `code_flow`, which prints the
  same code twice then immediately the email confirmation. It cannot keep a
  code open for a deliberate repeat scenario as currently scripted.
- `tools/played/ask-again.sh` checks an interrupted email-confirmation question,
  its 30-second reoffer and the subsequent yes; it never says "say it again"
  or counts code deliveries. Its six passing checks in `docs/lamp-runs.md:108`
  qualify that different scenario, not #2839.

Concrete source risk to reproduce, not an established cause of the lamp report:
START can first return `starting`; the Tasks loop can then receive a code;
the news reader can offer it with `code_told=false`; START's next poll can
return and mark that same code. `task_news.rs:837`/`:948` retain/take queued
sign-in news without rereading its current `code_told` state. The separate
code paths each look correct in the existing scripted tests. A deterministic
interleaving regression should establish whether two delivery paths survive.

Smallest safe Tests PR: only test sections/fixtures in
`crates/voice/src/sign_in.rs`, `crates/voice/src/task_news.rs` if its real queue
is needed, `crates/tasks/tests/fixtures/harness_cli.rs` for an event-controlled
code-only phase, plus `docs/tasks.md`. Count one initial delivery, no delivery
on repeated unchanged news reads, and exactly one additional delivery after
an explicit second heard request. Include the code-arrival interleaving above;
preserve off/not-heard, question retry, single-session, and privacy assertions.
Use positive fixture events, no completion sleep. A separate played-script
extension may add the same count to `ask-again.sh` with its stand-in tests.
If the product fails, report the failing boundary and request product scope;
do not hide it by changing a runtime gate in a test-only PR.

Gates for an implemented test slice: focused new tests and relevant sign-in/
task-news tests, full serial Voice and Tasks, fmt, strict Clippy, workspace
build, Linux CI. Actual speech count still needs an authorized played run on
a named lamp with CI ARM artifacts, a stand-in account and restoration. The
V1 search found no corresponding `sign_in_computer`/`login --code` path in its
maintained runtime directories; the current Tasks/Voice protocol is the useful
regression baseline.

## #2848 — waking face in product units

`deploy/product/lamp-waking.service` is installed/enabled by Update but has no
`Environment=LAMP_WAKING_FACE=on`. It is WantedBy sysinit.target, gated on the
setup marker, and `Restart=no`. Neither the voice unit nor lampos.target pulls
it in on a voice restart. Merely adding the environment setting would not
establish the issue's voice-restart requirement.

The implementation already has strong host coverage: `setup/src/wake.rs`
tests the exact-on switch, initially lit frame, fade/breath/unwell behavior;
`setup/tests/wake.rs` checks actual file-lock ownership transfer, missing device,
malformed asks, and the stuck-face privacy deadline. `update/tests/install.rs:770`
checks marker/sysinit wiring. `ring/src/waking.rs` preserves the bounded release
and privacy bypass. V1's `hal/server_support/boot_led.py` and tests stop its boot
unit and wait for successful exit before opening SPI; retain the one-writer
principle, not its HAL/systemd callback design.

The explicit prerequisite #2847 is **closed and accepted by First**:
https://github.com/autonomous-ai/lampOS/issues/2847#issuecomment-6339260862
The underlying three cold boots are recorded here:
https://github.com/autonomous-ai/lampOS/issues/2847#issuecomment-6333120153
Keep its limits: first-light latency was inferred, not measured; round 2's
early logs were lost; the observed handovers in rounds 1/3 were 277/195 ms.
The switch's drop-in was removed afterward. Do not relabel that receipt as
fresh-install or voice-restart qualification.

This issue cannot be completed by test-only edits. The smallest prospective
product PR needs `deploy/product/lamp-waking.service`, explicit agreed voice
restart lifecycle wiring (possibly `lamp-voice.service`), its install/unit
regression, and `docs/{switches,install,ring-choreography}.md`. Avoid writing a
failing enablement assertion or redundant current-off assertion as a stand-alone
green test PR. First should settle whether each voice restart must launch the
waking face; then test the chosen lifecycle and privacy/ownership handover.
Qualification still needs fresh installation and a voice restart on a held,
authorized lamp. No such run was attempted here.

## #2862 — fresh-image reports consent

The consent implementation is already fail-closed:

- `setup/src/serve.rs:317` accepts only JSON true for `share_reports`; missing,
  false, malformed or legacy requests mean no. Its writer keeps the versioned
  choice in `/etc/lampOS/setup/reports.json` after linking and before the key.
- `cloud/src/reports.rs:897` reads consent at each send; missing, unreadable,
  unknown/newer records mean no. The own choice overrides setup's choice.
  The bus can only turn reports off. Existing tests cover all three gates,
  no-secret canaries, stop/revoke/queues and malformed/forward records.
- `setup/tests/it/serve.rs:1405` checks the real writer and confirms the product
  reset plan wipes both setup's answer and `/var/lib/lampos-cloud`; this is a
  plan-membership assertion, not a full writer→broker→reset lifecycle test.
- `cloud/src/online.rs:2075` already provides an in-process test broker proving
  report publication/ack/reconnect. `cloud` already dev-depends on `lamp-setup`.
  `reports::Timing`/`step` take time explicitly (first health at 5 minutes,
  ordinary health every 15 minutes, existing voice deferral up to 60 seconds).

Two product gaps are concrete. Neither `lamp-cloud.service` nor
`lamp-voice.service` sets LAMP_REPORTS. The shipped `setup/src/page.html` has no
reports question and does not submit `share_reports`; it currently means no
by design. The external app's setup-screen state was not inspected here.
V1's `system/telemetry/telemetry.go` gates its different analytics transport on
a configured endpoint; it does not supply the explicit consent policy wanted
here. Do not copy that default into reports.

Smallest safe separate Tests PR: extend the existing Cloud test-broker fixture
to use Setup's actual consent writer in a temporary root: missing choice and
explicit no produce no `/report` publication; explicit yes produces a health
using the driver's supplied time; a reset using a **relocated test plan**, then
reopening, removes both choices and sends no new report. Keep heartbeat/info
traffic distinct from report traffic. Preserve the existing stopped-message
semantics for revocation and all production cadence/deferral constants. Include
the default-page request's omission as no, without pretending it asks consent.
Scope can stay in `cloud/src/online.rs` cfg(test), a narrow Setup integration
test if reset wiring needs it, and `docs/cloud.md`/`docs/setup.md`.

Completing the issue additionally needs approved product changes: the two unit
settings, a default-unchecked explicit question and boolean submission in the
setup page (and coordination with the external app owner), unit/function census
checks, and matching docs. `update/tests/functions.rs` already has a reports-on
tool-count case; enabling it in the product also changes its current default
tool expectations. Existing #1683 lamp reports and `docs/lamp-runs.md:192` do not
prove shipped-image defaults/reset. The issue's fresh-install no/yes≤15-minute/
reset broker run still needs a designated lamp and explicit reset authorization.

Recommendation: #2839's deterministic regression first; a separate #2862
writer-to-broker/reset test is useful within Tests scope. Leave #2848 enablement
and both issues' product defaults/lamp operations for First's scope decision.
