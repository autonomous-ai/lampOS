# devops007 work log

## Proposed tasks

| id | proposal | where |
|---|---|---|
| devops007-P1 | Link the ARM64 run's test binaries with lld: 34% off a cold test compile, 33–35% off each rebuild after an edit, every test passing | #1806 |
| devops007-P2 | lamp-voice's ARM64 push runs: the binaries and the cache save in a job no newer merge cancels, the tests keep their one-at-a-time limit | #1806 |

## Found, not mine

- **The clippy cache crowds out ARM64's** (21 Oct 12:4x +07). Since #2465 each lamp-voice clippy run saves about 2.2 GB, three in four minutes (12:43–12:47), against ARM64's 1.1 GB and the repository's 10 GB, which GitHub evicts least recently used first. `gh api repos/autonomous-ai/lampOS/actions/caches` shows it. Posted on #1806.

## 2026-10-21 12:55 +0700 — P2, the clippy cache, and asking to write P1

Ready for lamp-voice: no

Asked by its person: post what was found and ask for a plan so devops007 writes P1 without waiting. On #1806: the clippy cache (above), devops007-P2 (2 of the last 5 lamp-voice ARM64 runs cancelled while waiting, so the cache's renewal and those commits' binaries are lost), and a two-step P1 that needs no lamp check first: lld on a test job of its own, the binaries' job unchanged, then one job with lld everywhere after the first builder's yes and a lamp check. On #1722: the ask for a `builder:devops007` task for it.

## 2026-10-21 11:45 +0700 — the ARM64 run measured, and lld proposed

Ready for lamp-voice: no (a proposal; no code for lamp-voice)

Asked by its person: track the CI's parts, their time and cost, and what cuts the time. Speeding up "lampOS on ARM64" is Tests' task (#1806, #2465, #2451), so this is measurement and one proposal, not a pull request.

**Today, from the API (24 h to 10:30 +07):** 373 pull request runs at 21.3 min median (p90 24.1) and 112 lamp-voice pushes at 24.3 min, 62 of them cancelled while waiting, so 55% of lamp-voice's commits get no artifact. One 4-core runner spends about 1 min on setup, 4–8 min on the release binaries, 10–12 min compiling the test binaries and 2.5 min running 9,043 tests. Free: the repository is public.

**Trial run [39540759083](https://github.com/autonomous-ai/lampOS/actions/runs/39540759083)** on `devops007/ci-trial` (never merged), lamp-voice at `563a97099`, five variants each on its own `ubuntu-22.04-arm` runner, cold, no cache read or saved:

| Variant | Cold test compile | Edit lamp-memory | Edit lamp-voice | Edit lamp-msgs | Tests |
|---|---|---|---|---|---|
| v0 today (binaries 437 s, then tests 617 s) | 1,054 s | 67 s | 291 s | 715 s | 144 s |
| v1 release test job alone | 1,058 s | 65 s | 285 s | 689 s | 144 s |
| v2 release + lld | 700 s | 42 s | 186 s | 461 s | 136 s |
| v3 `ci` profile (opt-level 1, deps 3) | 931 s | 51 s | 222 s | 556 s | 148 s |
| v4 `ci` profile + lld | 966 s | 48 s | 217 s | 558 s | 150 s |

An edit is one appended line, then `cargo test --no-run --workspace` again. Every variant ran 9,057 of 9,057 tests. Rust 1.96 on aarch64 Linux links with GNU ld (the binaries' `.comment`), so lld is a real change. The `ci` profile loses to release with lld and is dropped. A parallel binaries job saves no time by itself (v1 ≈ v0).

**Not shown:** the trial's runs are cold; with #2465 and #2451 a pull request rebuilds only the edited crates, which the edit columns stand in for. Nothing ran on a lamp.
