# BASH_ENV for mock runs: every sleep 1/SLEEPDIV as long (wall-clock waits are capped by WAITMAX in row.sh)
sleep(){ command sleep "$(awk -v s="$1" -v d="${SLEEPDIV:-25}" 'BEGIN{printf "%.3f", s/d}')"; }
