# BASH_ENV for mock runs: every sleep 1/SLEEPDIV as long.
sleep(){ command sleep "$(awk -v s="$1" -v d="${SLEEPDIV:-50}" 'BEGIN{printf "%.3f", s/d}')"; }
