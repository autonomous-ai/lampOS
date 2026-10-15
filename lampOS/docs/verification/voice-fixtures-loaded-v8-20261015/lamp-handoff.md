# Fourth fixture handoff: lamp checks to include when qualification is complete

This is an uncommitted planning note, not a passed gate or deployment request.

The fixture clock and overrides require explicitly selected tests; normal Voice keeps
the existing monotonic clock, 50,000 us privacy freshness, 60,000 us reopening,
100,000 us input/channel freshness and current process cleanup limits. There
is no runtime flag to enable the fixture.

First owns the lamp. After reviewing the final non-draft PR and its exact-head
native ARM64 artifact, use the normal unchanged lamp-4ace unit for the required
one-minute compatibility run. Fourth has not accessed that machine. Keep
actual ExecStart/hash and run boundaries with First's evidence. In one shell
on the lamp, collect these read-only readings immediately before the minute:

```sh
fixture_check_started_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
date -u +%Y-%m-%dT%H:%M:%SZ
cat /proc/uptime
systemctl show lamp-voice.service -p ActiveState -p SubState -p ExecStart -p MainPID -p NRestarts -p CPUSchedulingPolicy -p CPUSchedulingPriority
fixture_check_pid=$(systemctl show lamp-voice.service -p MainPID --value)
test "$fixture_check_pid" -gt 0 && sha256sum "/proc/$fixture_check_pid/exe"
cat /proc/loadavg
for fixture_check_zone in /sys/class/thermal/thermal_zone*; do
  test -r "$fixture_check_zone/type" && cat "$fixture_check_zone/type"
  test -r "$fixture_check_zone/temp" && cat "$fixture_check_zone/temp"
done
```

Leave the installed configuration unchanged for the required minute, then
repeat those readings without resetting `fixture_check_started_utc`. Keep
before/after results, actual elapsed uptime and the installed binary's hash
with the CI artifact hash. This minute qualifies the task build only when
the running executable's hash matches the intended exact-head CI artifact.
If it does not match, label the result as an observation of the existing
installation; it does not qualify this PR. Check that the same service
process stayed active and `NRestarts` did not increase. Keep the journal interval from that start
time, filtering for the relevant failures:

```sh
fixture_check_finished_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
journalctl -u lamp-voice.service --since "$fixture_check_started_utc" --until "$fixture_check_finished_utc" --no-pager -o cat --grep 'physical privacy closed or lost freshness|service privacy freshness lost|physical privacy unavailable|capture discontinuity'
```

Retain the command's exit status. A journal read failure is not evidence of
no faults; redact any conversation content in First's evidence as required.

This check alone cannot establish observation-to-consumption latency, bound
thermal behavior, or prove an acoustic or mechanical stop. Normal accepted
unmuted observations are not fully recorded by today's coordinator, and a
producer send gap is not consumption age. First needs a separate measurement
at the actual physical acquisition and coordinator consumption boundaries to
qualify the real 50 ms bound under lamp load and temperature. Do not widen a
production bound from these synthetic tests.

Most likely wrong on the lamp: its real 50 ms privacy margin under blocking
work or load, even though the checked-in unit requests RR priority 20. The
installed scheduler settings have not been read by Fourth. The known motor
fixture parent/child clock question is separate and remains pending in #739;
no Body change is included unless First authorizes it.
