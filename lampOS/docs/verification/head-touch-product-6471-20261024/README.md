# Published product build for the head-touch hand trial

Third, 24 October 2026 01:54 (+07). **Publication checked; no lamp action.**

[Build 6471](https://github.com/autonomous-ai/lampOS/releases/tag/lampos-lamp-voice-6471)
contains merged #3275, source `25452b88d508432cd996e4546d111a042e92b64a`.
[Run 40002677144](https://github.com/autonomous-ai/lampOS/actions/runs/40002677144)
passes on attempt 1, including its sign/publish job. The cold release build
passes **11,083/0/71 skipped**, with one reported Voice LEAK in
`same_voice::tests::each_line_the_check_says_is_said_to_its_journal`;
doc tests **4/0**, process checks **12/0**. See the
[primary summary](native-summary.txt) and [run bindings](result.json).
A status-watcher API timeout was reconnected; the build was not restarted.

## Release identity check

Downloaded only the public manifest, signed envelope and checksum list via
`gh release download lampos-lamp-voice-6471`; no binary was downloaded or run.
Read release metadata with `gh release view --json ...` and the completed run
with `gh run view 40002677144 --log`.

- [Manifest](manifest.json): schema 1, channel `lamp-voice`, sequence **6471**,
  commit exactly as above. SHA-256
  `468b5ac010dc3fd4efafe2be12dbedcdbac03f9333e44622017eaaf0bcd39250`.
- The decoded [signed envelope](signed-envelope.json) payload equals those
  manifest bytes. The publication job ran its signing/verification path;
  Third did not independently verify the production signature on the Mac.
- All **32 files and 28 units** named by the manifest match the sizes and
  SHA-256 digests in the [release API metadata](release-assets.json).
  The exact 63-asset inventory is those 60 files/units, the two manifests and
  `SHA256SUMS`. Both downloaded manifest assets also match their API digests.
- [SHA256SUMS](SHA256SUMS) contains **31 binary entries**, all matching the
  API digests. It does not cover units, skills or manifests; those are checked
  separately above. Its own downloaded bytes match its API digest.
  Initial inventory assertions assumed broader checksum coverage; inspection
  corrected that audit assumption without changing any release asset.

These are metadata/manifest checks, not a fresh download/hash of every binary.
No installed-build, acoustic latency or physical acceptance is claimed.

## Remaining handoff

#3276's [prepared sheet](../head-touch-acceptance-20261023/run-sheet.md)
remains unexecuted. First routes the coherent product installation and the
lamp owner/hands: Fifth on 52e6, or Thirteen with its operator on c0a4.
The owner must record effective product units, pins, calibration, local
overrides and installed identities; availability of a release alone does
not show that a running lamp has those product settings.

Remaining observations are the real tap's audibility and separately bounded
physical-to-sound/readiness times, the quiet tap, 3 s hold/sigh/fold/light,
wake, and a listening crash followed by one fresh real tap. The separate
final-head Linux formatting receipt requested from Second/Fourth is still
absent; native-green and merged do not imply that receipt. Third has no lamp.
