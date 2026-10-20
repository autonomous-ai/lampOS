#!/usr/bin/env bash
# What a voice/sound tap looks like to Expression's speaking rules (#1759; the
# first builder on #1759, 20 Oct 01:14): why a voice near the meter's top
# nods or not. Counts and percentiles only, no words. Run on the lamp, on a
# file `lamp-bus tap voice/sound` wrote (speaking-ab.sh keeps them in its
# stage, e.g. /var/lib/thirteen-ab/on.sound). Needs jq.
#
#   sound-shape.sh FILE
#
# Prints the levels' percentiles over pieces with sound; how many pieces stand
# over the recent level (Expression's 250 ms average, speaking.rs) as a ratio,
# x1.5 as stress_rise is today and x1.2, and as a step, +0.10, +0.15 and
# +0.20; the pauses of 350 ms or more (pause_us) under each quiet line; and
# how far ahead of its playing each piece was said (its start less its
# stamp): Expression moves the body by the piece playing body_lag_us (300
# ms in the catalogues) from now, so a piece said nearer than that never
# moves it (#1759, 20 Oct 01:52: 0 nods on lamp-4ace with the rises there).
f=${1:?a voice/sound tap file}
pieces=$(jq -r -R 'sub("^[{][^ ]*[}] "; "") | fromjson? | select(.level != null) | "\(.start_us) \(.end_us) \(.level) \((.start_us - (.stamp.at_us // .start_us)) / 1000)"' "$f" | sort -n -k1,1)
[[ -n $pieces ]] || { echo "no sound"; exit 0; }
# Percentiles of the levels with sound in them (plain awk: no asort).
awk '$3>=0.01 {print $3}' <<<"$pieces" | sort -n | awk '{v[NR]=$1} END {if (!NR) exit; printf "level p10 %.2f p25 %.2f p50 %.2f p75 %.2f p90 %.2f p99 %.2f\n", v[int(NR*.1)+1], v[int(NR*.25)+1], v[int(NR*.5)+1], v[int(NR*.75)+1], v[int(NR*.9)+1], v[int(NR*.99)+1]}'
# How far ahead each piece was said, in ms.
awk '{print $4}' <<<"$pieces" | sort -n | awk '{v[NR]=$1; if ($1>=300) far++} END {printf "said ahead (ms): p10 %.0f p50 %.0f p90 %.0f; 300 ms or more %d of %d\n", v[int(NR*.1)+1], v[int(NR*.5)+1], v[int(NR*.9)+1], far, NR}'
awk '
{ s[NR]=$1; e[NR]=$2; l[NR]=$3 }
END {
  n=NR; if (!n) { print "no sound"; exit }
  m=0; for (i=1;i<=n;i++) if (l[i]>=0.01) m++
  printf "pieces %d, with sound %d, piece %.0f ms\n", n, m, (e[1]-s[1])/1000
  # The recent level as Expression keeps it (250 ms), and how far each piece
  # stands over it: as a ratio (stress_rise 1.5) and as a step.
  r=0; last=s[1]
  for (i=1;i<=n;i++) {
    step=s[i]-last; if (step<0) step=0; last=s[i]
    # As speaking.rs: compared before the piece joins the average, which
    # follows every piece, quiet ones too.
    if (l[i]>=0.06) { if (l[i]>=r*1.5) x15++; if (l[i]>=r*1.2) x12++; if (l[i]>=r+0.10) a10++; if (l[i]>=r+0.15) a15++; if (l[i]>=r+0.20) a20++ }
    pace=1-exp(-step/250000); r+=(l[i]-r)*pace
  }
  printf "pieces over the recent level: x1.5 %d, x1.2 %d, +0.10 %d, +0.15 %d, +0.20 %d\n", x15, x12, a10, a15, a20
  # Pauses: runs under each quiet line, and how many last 350 ms or more.
  split("0.06 0.13 0.20 0.30", q, " ")
  for (k=1;k<=4;k++) { run=0; c=0; longest=0
    for (i=1;i<=n;i++) { if (l[i]<q[k]) { if (!run) b=s[i]; run=1; d=e[i]-b; if (d>longest) longest=d } else { if (run && e[i-1]-b>=350000) c++; run=0 } }
    printf "quiet under %.2f: %d pauses of 350 ms or more, longest %.0f ms\n", q[k], c, longest/1000 }
}' <<<"$pieces"
