# Published Settings store compatibility

Part of #2809. This test-only slice qualifies two actual published Settings
executables against one private desired-settings store, both ways and back.
It never substitutes today's Engine or Settings client for a released reader.
The ordinary current-CLI case checks the driver, not the released pair.

| Release | Source | Asset | Bytes | SHA-256 |
|---|---|---|---:|---|
| 6408 | `ee53765c8d014eb00e41c1a5a7054d2546c18bdb` | 668607890 | 1,529,624 | `26079d50d404d1751793519a3179ae86f8a442889a99388e115cfa89f274bd2d` |
| 6498 | `846b3d203fc00ec3886d71f7673d8cc29cc4a634` | 670759896 | 1,529,744 | `559410666aae9d415ff072fc9a8682a4f1f5837026e4ee535028f4f21fbc648c` |

Assets are public `lamp-settings` payloads from tags
`lampos-lamp-voice-6408` and `lampos-lamp-voice-6498`. Their executable bytes
differ, while the complete Settings `src` trees are identical Git tree
`6cb7ff34584749bc09624b24aeda8962eaf1e0ba`. This qualifies shipped artifacts;
it is not evidence of a changed persistence implementation. Pin verification
requires exact size, SHA-256 and ELF64 little-endian AArch64 identity.
Hashes bind selected bytes, not release-signature trust.

The explicit native cases use released catalog commands and three released
server lifetimes per direction. A writes a guarded change, B opens the same
store, replays its receipt without a second commit and writes a new change,
then A reopens the return trip. Checks cover complete catalog/current values,
store identity, exact revisions and retained request semantics. Wrong/stale
guards, a changed request reused under the same ID and unknown wire enum
values must refuse without changing persisted bytes. Authored malformed or
newer stored-state controls are labelled synthetic, preserve their bytes and
count as refusal evidence, never as released-writer output.

Settings' current stored types remain strict. This change does not add unknown
field tolerance, reinterpret unknown tagged `op`/`kind` values, or set aside a
newer store. Refusal must not substitute defaults. It also does not establish
arbitrary future schema compatibility.

Every child has a cleared environment and private working directory/process
group. No bus root, provider, credential, hardware, actuator or setting
consumer is configured. Values are saved intent only; a successful commit
never claims another component applied them. Original case/step/I/O deadlines
bound retries and partial replies; process start, request/reply, reaping,
failure-cleanup and file/input hash receipts remain available after failures.
No inherited directory is removed. Executable copies are excluded from
workflow evidence. Output size checks are not a live disk quota.

Only the official PR workflow executes released AArch64 artifacts. Mac checks
compile and run the current-CLI driver and ordinary controls; local Linux
formatting is source-only. Native execution, ordinary ARM64, Linux Clippy and
independent review remain required before acceptance. No native or physical
pass is claimed by adding this document. Hardware, real settings consumers,
release trust, service-manager sandbox, updater switching, cold boot and
power-loss durability remain unqualified. This slice does not close #2809 or
raise the whole-lamp row by itself.
