# Keeps a tap's line only when its payload changes (stamps aside), plus one
# line a minute, so a 5 Hz topic stays small over a night. The header's
# at_unix_ms is the publisher's wall clock.
{
  i = index($0, "} {")
  if (i == 0) { print; fflush(); next }        # lamp-bus notes (gaps, restarts)
  p = substr($0, i + 2)
  gsub(/"stamp":\{[^}]*\},?/, "", p)
  gsub(/"(at_us|at_unix_ms|since_us|seq|expires_ms)":[0-9]+,?/, "", p)
  t = 0
  if (match($0, /"at_unix_ms":[0-9]+/)) t = substr($0, RSTART + 13, RLENGTH - 13) + 0
  if (p != prev || t - last >= 60000) { print; fflush(); prev = p; last = t }
}
