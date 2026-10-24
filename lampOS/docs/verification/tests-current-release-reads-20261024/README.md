# Current release-reader independent reviews

Own fixture PR qualification remains active. These are two completed independent reads of other branches; their earlier failures remain separately preserved.

| PR and authored head | Dedicated native pair | Ordinary ARM64 | Skipped | Linux Clippy |
|---|---:|---:|---:|---|
| #3312 `4ba9d959f` | 12 passed, 0 failed/ignored | 11,163 PASS + 1 LEAK; zero failed/unrun | 83 | 40057007027 passed |
| #3327 `8bc821736` | 6 passed, 0 failed/ignored | 11,163 PASS + 1 LEAK; zero failed/unrun | 77 | 40057211165 passed |

Both ordinary censuses contain 11,164 unique contiguous indexed identities. Their LEAK is `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`, retained separately from PASS; the prepared test teardown work has not yet run. Counts for docs, separate process harnesses and dedicated pairs are not added to the primary ordinary census.

#3312 is one folded commit directly on 9ed44f88c. All fixture/workflow source is unchanged from its reviewed correction; only the verification README adds correctly attributed historical evidence. Fresh dedicated run 40057006999 and ordinary run 40057006981 both bind to preview ee65e8fcd, whose tree equals the authored tree. All 6,977 source hashes match before/after and Git. The Tasks private-parent, six schema/direction cases, and six Voice replay/Ack/settlement cases verify, including all eighteen Voice child reaps. [Completed read](https://github.com/autonomous-ai/lampOS/pull/3312#issuecomment-6379272647). The earlier local serial StaleOwner failure and isolated pass remain the author's separate observations.

#3327 fixes the bus/process-directory collision by using `child-<stage>` consistently for all five sockets, leaving the shared bus and every original assertion intact. Fresh dedicated run 40057211166 and ordinary run 40057211171 bind to preview 9ca22aff7, tree-equal to the authored head. All 6,974 source hashes verify. Both 135-record traces show A→B→A, pinned executable identities, six actual child reaps, separate stale/wrong-receipt refusals and matching Cancel, admission renewal, one private stretch followed by synthetic Body Over, exact daily bits, immutable manifests and return suppression. The absent-rested negative check is an executed source assertion; no separate early snapshot was retained. Parent #3323 must land and the continuation must follow its rebase/current-gate sequence before merge. A nonblocking README correction should replace stale introductory “not executed” wording while retaining the initial 4/2 failure.

The JSON receipts record exact source, artifact and log hashes and the verified protocol observations. The copied executables, sockets and complete source exports are not committed. Raw native logs and downloadable artifacts remain on their GitHub run; external audit paths are not a remote availability promise. SHA-256 equality is an identity check, not independent release-signature trust.

These fixtures use genuine released Skills, Tasks and Voice processes with private synthetic peers and completion messages. They do not establish physical motion, acoustic output, actual Harness completion, an elapsed night, updater installation, synchronization or power-loss durability. No local ARM build or lamp action ran.
