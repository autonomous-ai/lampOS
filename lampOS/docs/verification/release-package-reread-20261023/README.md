# Generic release notice corrections, 23 October 2026 (+07)

[PR #3095](https://github.com/autonomous-ai/lampOS/pull/3095) head
`82fbaf42e`, base `1b5f7a78b`, corrects both P3s from the read of
`9694ec633`. No remaining P1/P2/P3 was found in those corrections.

Cached notices now require both declared size and digest; the mismatch
fixture checks that a 20-byte cached manifest declared as 21 bytes is
fetched again and the mismatching result refused. The upload mock has
separate notice-fetch and final-package failures, and checks their order
relative to Docker run; neither publishes or changes the version.

Independent affected tests: **41 passed / 0 failed**, 14.669 seconds,
plus shell syntax success. All 51 verified source hashes and modes were
unchanged after validation. [Review](review.json), [source binding](source-binding.json)
and [exact commands/results](affected-gate.json). The unaltered test log
is [affected-tests.txt](affected-tests.txt); the gate receipt calls this
same 223-byte output `affected-1.log`. Its SHA-256 is unchanged.

The release tools are mocked. This is not an asset-download, ARM64-build,
real-key, publication, installation or device-acceptance result. First
retains merge and release acceptance.
