# Actual updater CLI handover, 22 October 2026

For [#2810](https://github.com/autonomous-ai/lampOS/issues/2810), two isolated
ARM64 Podman scenarios exercised the real `lamp-update once --apply on`, its
`Command::exec` into the candidate, new-main confirmation and a subsequent
ordinary round. This supplements the seven library-process cut cases in
[#3012](https://github.com/autonomous-ai/lampOS/pull/3012). It does not replace
the attended physical unplug.

| Input and fixture | Normal detach | Observer SIGKILL | Total | Subscription warnings |
| --- | ---: | ---: | ---: | ---: |
| Authored PR artifact, mounted output bus | 3.409 s | 0.327 s | 3.748 s | 18 |
| Replacement PR artifact, container-local bus | 3.335 s | 0.302 s | 3.651 s | 0 |

[authored.json](authored.json) and [current.json](current.json) retain separate
input/source identities, verified prefixes, outcomes and log hashes. These
are two scenarios per input, not additional Cargo tests. Both scenarios keep
the updater alive; the SIGKILL is delivered to the **observer**, not the updater.

## Artifact identity

The authored input is the independently verified artifact of head `33a43cdd2`,
updater SHA-256 `a42dbaade44bc19a65addcb18bd09d1dbdf764c55d571f0e05551c6200dce535`.
The replacement input is head `d4d5e5a66`, tested as merge `a4e570c` with
base `c6d50559a`. Its [artifact receipt](artifact-d4.json) verifies archive
`12432514917`, **50 ZIP members and all 49 payload checksums**, with archive
SHA-256 `f7e8f85fcefeb44e4c8afd5514cbddde0eaa9ceebd48fd7ee6c4290d72bfed14`.
Its updater is ELF64 little-endian AArch64, 4,592,768 bytes, SHA-256
`8191ea0429655ec1fa62c2cf18b9f79cd29e73ce0a96d7372b17700ff9293ecd`.
These updater inputs differ; evidence is not transferred between them.

The replacement's first archive download timed out. A range retry checked
the response range and final API digest before any extracted file was
qualified. No partial archive was executed. A PR merge artifact remains
ineligible as a production signed-release input.

## What actually ran

Each scenario used a fresh disposable root, private local `file://` channel,
disposable Ed25519 key, and an exact simulated systemctl readiness response.
Network access was disabled. The image was Debian bookworm with GNU GDB 13.1
and Python 3.11.2; ptrace was explicitly allowed for this container only.

The old and new fixture executables were derived from the same verified
input by appending distinct fixed trailers beyond its original ELF segments.
Their original byte prefixes matched the input hash, and both executed
`--help` successfully before the trial. They are synthetic variants of one
implementation, **not two software releases**. The fixed fallback retained
the original verified bytes and executable mode.

No updater state or pending record was manufactured. Records were absent
before the real old CLI ran. That CLI read the locally signed manifest,
recorded installed sequence 2, staged its candidate, wrote pending and
changed current before its actual exec. The manifest's all-zero-prefix
commit is a fixture value, separate from the artifact's source commit.

A FIFO-controlled shell guaranteed that GDB attached before execution.
The first verified exec was shell to old updater; the second was the old
updater's own handover into the candidate. Both required the same PID/start
time, expected executable path/hash, exact argument bytes and stopped state.
Before READY, both observer and parent independently verified current,
pending old/new identity, installed sequence, made candidate, old/new/fixed
fallback bytes and modes, and absence of candidate confirmation.

With normal detach, the candidate continued after the bounded three-second
hold. In the other case, killing the observer released the stopped candidate.
Both candidates then ran their real main, verified the signed channel,
recorded **exactly one self-confirmed event**, cleared pending, and exited 0.
A separate next-round CLI invocation also exited 0 at sequence 2, with no
second confirmation. All owned children were reaped, with no cleanup kill
or SIGCONT rescue. The runner had a 90-second overall deadline and a
40-second case deadline; successful runs did not exercise emergency cleanup.
Disposable private-key files were removed in both success and failure cleanup.

The first fixture's mounted bus path produced 18 `Invalid argument`
subscription warnings, retained in its receipt. The later fixture moved
sockets to the container's own `/run` and used the different replacement
artifact; it produced none. No product fix or isolated comparison between
those two changes is claimed. Neither trial injected voice or health traffic.

## Limits and remaining work

- Systemctl readiness was simulated. No actual service, timer, boot or
  automatic deadline recovery was exercised.
- The manifest had no ordinary parts or Body. Fixture options used a
  one-second quiet-wait deadline and watch duration; production 45-second
  quiet admission and 60-second health behavior were not tested here.
- There was no cross-version compatibility or updater-level climb.
- The updater was not killed in these two scenarios. Its fixed-path `back`
  CLI was not invoked; that needs a separate disposable-container test.
- No physical power interruption, kernel-cache loss, flash durability,
  real signing key, internet publication or attended-device acceptance is shown.

First still owns the lamp/person/window and persistent trial setup. The
attended #2810 unplug remains unexecuted, followed by #1812's real signed
release and attended Body run. No device connection or deployment occurred.

The measuring code remains in scratch outside lampOS. These files publish
normalized evidence and source/log hashes only: no Python, binaries, raw
logs, private keys or personal host paths. `SHA256SUMS` binds this proof.
