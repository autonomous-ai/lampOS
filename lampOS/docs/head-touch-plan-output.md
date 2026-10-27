# Bound derived head-pins text for a future plan

Part of #3689. `head_touch::plan_output::project` is a private, unwired
entry point for future ordinary-plan preparation. It evaluates the same
head-pair policy as the ordinary projector, using the supplied `HeadInputs`,
root, units, carried texts and signed-setting data. It changes no command,
service, allowlist, grant, stored state or ordinary caller.

The ordinary `project` and `project_with` keep their existing formatter,
diagnostics, read order and output bytes. Both paths call one policy body;
the future-plan entry point selects a capped formatter. No policy or
filesystem reader is duplicated.

## Derived output only

The new formatter streams pretty JSON into a fallible byte buffer capped
at **1 MiB, including the trailing newline**. Every append checks the next
length before requesting more capacity or extending the buffer. Capacity
requests grow geometrically within the cap rather than reserving a fresh
exact-sized buffer for each small serializer write. It creates no
intermediate unrestricted pretty String. Successful bytes and SHA-256 are
identical to the ordinary formatter for the same projected object.

The cap covers newly serialized pins text. It does not bound JSON parsing,
existing inputs or preexisting step text, and does not make an arbitrary
step vector an eligible plan. A Keep or no-rewrite path may return existing
data larger than this output cap because it serializes nothing. The future
caller still validates and bounds the complete signed input, before-images,
carried units, read set and final plan independently.

The new API takes an owned staged `Vec<Step>` and returns it only on
success. On serialization or later provider failure, it drops that vector
and returns one fixed refusal category. A partially inserted or projected
row cannot escape in a successful result. This is memory ownership of
staged data, not proof of filesystem ownership or write authority.
Ordinary callers retain their existing in-place API and diagnostics.

With the [saved-input codec](head-touch-codec.md), a future caller can
evaluate from captured observations without reopening live files. This
piece does not authenticate those observations, prove a complete evaluation,
establish root ancestry or writer exclusion, publish a durable artifact or
implement restoration. The old helper and its compiled paths remain unchanged.

## Controls and acceptance

Source controls cover exact UTF-8 byte boundaries and the newline, refusal
before buffer growth, ordinary byte/hash/request-order equality in eight
head-mode/carried-unit/omitted-step combinations, closed replay after the
fixture filesystem is removed, compact input whose pretty output exceeds
the cap, Keep behavior, and a late provider error after omitted-row insertion.
The large-input controls demonstrate this formatter contract, not signature
trust or admission of those inputs.

Execution heads and results belong in the PR and immutable verification
evidence. These fixtures establish no latency improvement, complete plan,
root bootstrap, helper replacement, live restoration or physical power-cut
qualification.
