# Current Tests carries, 24 October 2026

Ready for lamp-voice: no. These receipts show source carries and Linux formatting only; matched local qualification and exact-head native CI remain pending.

All three owned branches were rebased onto `88b1ad796e5045490ad678eb240eea2dab010c81`. Each rebase had no conflict and a one-commit equal range-diff. Old heads are preserved in owned remote archive refs. No PR branch has been pushed by this receipt.

| PR | Old head | Rebased head | Range-diff | Linux fmt exit |
| --- | --- | --- | --- | --- |
| #3190 | `224e0bd76` | `5f87be93c` | Equal | 0 |
| #3218 | `328f7da0f` | `c46e6cfe1` | Equal | 0 |
| #3273 | `809d8493f` | `2c244d708` | Equal | 0 |

Each numbered source-and-format JSON binds the Git source, exact changed paths, formatter commands, raw streams and terminal result. The existing rootless Linux amd64 image ran `cargo fmt --all --check` on an exact Git archive with no network, compilation, test execution, ARM build or shared target. Source archive and Git identities remained unchanged. Container timings include setup, extraction, formatting and hashing; they are not test timings. The repository cwd and ephemeral container hostname are projected where explicitly recorded, with original byte counts and hashes retained. Full archives and image metadata remain external in the paths named by the receipts.

The bounded #3190 clean-base full run started its test stage at 01:17:20 UTC, after all three formatting commands completed. Subsequent candidate runs use the same 12 packages, commands and environment. Original failures remain failures; qualification will record matched-base outcomes and exactly three standalone observations where required. No new pass count or ready claim is made here.
