#!/bin/zsh
# asks-for-me.sh [SINCE_LOCAL_HH:MM] (30 Oct 11:0x, first builder): every comment on the Queue (#3997, then lib.sh's
# COORD_QUEUE_ISSUES: #4710, #5385, #6059, #6809) and the lead's channel (#1711) since SINCE that names the first builder and isn't
# mine, IN FULL, none dropped. Fourteen asked three times (08:42, 09:40, 10:5x) for two closes that my tail-cut scans
# never showed.
# 3 Nov 09:2x: with no SINCE it starts 5 min before its own last run ($LAMP_COORD_STATE/asks-last), so ticks can't leave gaps:
# hand-picked windows (since 08:22, since 08:40) missed the lead's agent's 08:19 and 08:38/08:39 asks, asked 4 times.
# 3 Nov: the time of its last run is state: asks-last in $LAMP_COORD_STATE (default ~/.lamp-coordination).
# 5 Nov: the Queue's issue numbers come from lib.sh, the one place they live.
TOOLS_DIR="${TOOLS_DIR:-${0:A:h}}"; source $TOOLS_DIR/lib.sh || exit 1
STATE_DIR="${LAMP_COORD_STATE:-$HOME/.lamp-coordination}"; mkdir -p $STATE_DIR || exit 1
last=$STATE_DIR/asks-last
s=${1:-}
if [[ -n $s ]]; then
  since=$(date -u -r $(date -j -f "%Y-%m-%d %H:%M:%S" "$(date +%Y-%m-%d) $s:00" +%s) +%Y-%m-%dT%H:%M:%SZ)
elif [[ -s $last ]]; then
  since=$(date -u -r $(( $(cat $last) - 300 )) +%Y-%m-%dT%H:%M:%SZ)
else
  since=$(date -u -v-70M +%Y-%m-%dT%H:%M:%SZ)
fi
now=$(date +%s)
print "asks since $since"
for i in 3997 $COORD_QUEUE_ISSUES 1711; do
  gh api "repos/autonomous-ai/lampOS/issues/$i/comments?since=$since&per_page=100" --paginate \
    --jq '.[]|select(.body|test("first builder|^First[ ,]";"i"))|select(.body|test("^First builder")|not)|"== #'$i' \(.created_at[11:16])Z \(.html_url)\n\(.body)\n"' || exit 1
done
print $now > $last
