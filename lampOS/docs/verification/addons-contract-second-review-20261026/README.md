# Add-on contract documentation review

READ-OK `844a28283fe07874892a703bc9b423ddd2182f40` (#3550), no P1/P2. The exact diff changes only `lampOS/docs/addons.md` and passes `git diff --check`.

The [public primary contract](https://github.com/autonomous-ai/openharness/blob/3f49f426833aacb712f20c3b4c04df8a2e51ec25/docs/autonomous-device-addons.md) confirms 300-byte notes; six proposals per minute per add-on/device and twelve per device; four undecided per device; 32 latest-value topics; and clearing on hello. The event-stream requirement is a design prerequisite, not a claim that current transport preserves events. Existing per-person consent and closed-by-default language are unchanged.

P3 reported: the four-undecided cap reports `BACKPRESSURE`, while per-minute limits report `RATE_LIMITED`; naming both would avoid conflation. No runtime or device test applies to this documentation-only read. No external repository was changed.
