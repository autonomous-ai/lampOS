# Reader platform catalog clarification

This additive correction addresses [R2 comment 6452382467](https://github.com/autonomous-ai/lampOS/pull/3543#issuecomment-6452382467). The original closed ECC host receipt is preserved byte for byte. Its `Linux_Reader_unrun_on_Mac` field contains the complete 16-name Linux source catalog; the label does not establish 16 Linux-only unrun cases.

The source catalog and observed Mac rows reconcile as follows:

| Category | Count | Mac evidence |
| --- | ---: | --- |
| Portable Reader cases | 3 | Each PASS once |
| Ordinary Linux-exclusive Reader cases | 12 | Not compiled or run on Mac |
| Linux-exclusive privileged DAC case | 1 | Ignored; no runtime credit |
| Mac-only ACL fallback | 1 | PASS once |

Thus Linux has 15 ordinary cases (3 portable + 12 Linux-exclusive) and one ignored DAC case. The Mac module has four PASS cases (3 portable + 1 fallback). The Linux-exclusive catalog has 13 names, not 16. The six ServiceTrace PASS rows are unchanged. This correction adds no execution or full Voice qualification claim.

`clarification.json` records every identity and the original census, source-reference, and terminal hashes and byte counts. All prior failures, native scopes, draft/latest-base conditions, and active gate inputs remain unchanged.

The prospective controller proposal in `future-controller-proposal.json` is source-only design. It is not applied to any active or historical controller. If a later completed gate emits the inherited label, its own receipt must be retained and receive a separately hash-bound post-run clarification.
