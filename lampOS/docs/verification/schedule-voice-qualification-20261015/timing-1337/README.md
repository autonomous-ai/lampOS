# Current-source host polling measurements

Both existing ignored probes ran once after the local task's build/test jobs
and isolated motor check ended. Each passed its functional assertions; neither
asserts a timing threshold. They used the same debug Voice test binary at
`d794cf2aca8fd5a7891af6363f2155e9fd3091fe`, SHA-256
`7759d868c24c2f83ea512b3eabcfd2bcd0dba94e98f9a32a27e77bc82fbd550f`.
All 700 source hashes and the binary stayed unchanged.

Machine: Third's Intel i7-10700K x86_64 Mac, macOS 26.6.2 / Darwin 25.6.0,
64 GiB RAM, Rust/Cargo 1.96.0. Each condition has one first sample and 100
warm samples. There are 303 samples per probe, 606 total. Raw samples, exact
commands, timestamps and boundaries are in [probes.json](probes.json), with
[parent output](parent.txt), [transport output](transport.txt) and
[retention hashes](retention.json). The separate full gate still reports
13 ignored tests; these two explicit executions do not rewrite that count.

| `Reminders::poll` condition | First, ms | Warm p50, ms | Warm p95, ms | Warm max, ms |
| --- | ---: | ---: | ---: | ---: |
| Idle | 0.070806 | 0.002027 | 0.002070 | 0.002781 |
| Incoming offers full | 0.325789 | 0.074473 | 0.108830 | 0.144009 |
| Outgoing full, completion waiting | 0.042396 | 0.0281505 | 0.028336 | 0.029363 |

This interval is `Instant` immediately before `Reminders::poll` through its
return, including bounded datagram receipt, JSON decoding, state checks and
settlement encode/send. Initial idle/incoming samples include claim receipt;
outgoing setup already received it. Queue filling is outside timing.

| `Transport::poll` condition | First, ms | Warm p50, ms | Warm p95, ms | Warm max, ms |
| --- | ---: | ---: | ---: | ---: |
| Idle peer | 0.063886 | 0.0009265 | 0.000976 | 0.001585 |
| Incoming saturated | 0.023563 | 0.002345 | 0.002768 | 0.003097 |
| Outgoing full, peer withholds reads | 0.011426 | 0.000936 | 0.001030 | 0.002777 |

This interval covers `Transport::poll` with an anonymous in-process Unix
datagram peer. The first poll includes claim-descriptor receipt/CLOEXEC; it
excludes serialization/deserialization and coordinator work. Every observed
poll was below the probes' 2 ms reference, which is a cooperative target,
not a hard OS guarantee. No before/after speed improvement is claimed.

## Not shown

Neither probe measures child startup, file/lock acquisition, Schedule/storage
work, a provider, the full coordinator iteration, audio or a lamp. These
numbers do not establish acoustic latency or responsiveness on target hardware.
