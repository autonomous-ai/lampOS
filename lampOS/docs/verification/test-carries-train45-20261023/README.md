# Test carries after train 45

The relay fixture correction #3255 merged through train #3261 at `8e3eaa6c2`. Both remaining test branches were carried cleanly onto that exact base.

- #3190: `d5d0383ac = 4b92b677d`, the same eight-path patch.
- #3218: `4e1e00e52 = 238403b1c`, the same two-path patch and identical authored file bytes.

[Boot carry](boot-carry.json) and [phone carry](phone-carry.json) record full identities, exact force-with-lease expectations and equal range-diffs. Only owned branches were pushed. Both PRs remain draft with their execution holds intact.

[Exact-head Linux formatting](linux-format/) passed once for each carry. Upstream changed Voice behavior/tests, Tasks, Update and the relay fixture, despite unchanged manifests. Earlier execution receipts remain bound to their recorded heads; they do not qualify this new base. Fresh local and native qualification is separate and pending. No ARM build ran on the Mac.
