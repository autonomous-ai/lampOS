# Independent Records byte-cap reader

Read #3381 at `0d057ef4be25107e203d8ccdef246360b2357ab0`. No P1/P2.
Only four direct boundary assertions and result documentation are added;
production cleanup guards and the six existing fixture controls are unchanged.
ASCII spaces are valid UTF-8 and do not depend on cgroup event parsing:
exactly 4,096 / 65,536 bytes succeed; each plus one refuses.

Independent scoped Mac fmt, strict Records all-targets Clippy and Records
**182 passed / 0 failed / 1 ignored** pass with unchanged source. The
Linux integration crate is absent on Mac. Separately, an exact portable
extraction of the existing helper and six filesystem controls passes **6/0**
on Mac. Removing only the size guard makes the strengthened control fail at
its direct reader assertion, expected exit101. No real cgroup/systemd,
ARM executable or published artifact was run locally.

The extraction runner's final empty-TMPDIR assertion failed because rustc
left a compiler-created xcrun_db file; all four subprocess exit assertions
had already passed. That failure remains in the receipt. Only the inspected
owned regular cache file was removed; all fixture directories were already
gone. No case was repeated.

Official exact-head ordinary ARM40094999252 passed **1,448/0, 16 skipped**,
including the strengthened cap case; Linux fmt/Clippy40094999249 passed.
Dedicated real-systemd ARM40094999273 has three exact passing case receipts,
exit0, source unchanged and six owned units with empty cgroups and confirmed
exit. Independent read of evidence archive12560895868: 2,039,736 bytes,
SHA256 93013b2fe2618ec3b2af50d3552f00332f3145cf11953fbb2b3c080e286f7760.
Selected receipt metadata and individual case hashes are retained. The
preliminary selection file did not verify receipts; the separate final
case-verification record does and reports qualified=true. Signature trust
was explicitly not verified; no physical lamp/power-loss or complete
rotation acceptance is inferred.

Private home paths are redacted with original/published hashes; no archive,
executable, compiler cache or fixture state tree is exported.
