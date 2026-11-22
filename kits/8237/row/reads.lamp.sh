# BODY reads
# lamp, root sh (common first, then SECS=). READ ONLY. Every 0.5 s for SECS s: the epoch and the pids of OpenClaw's
# approvals read (gateway.rs:225-239 APPROVALS_READ, `openclaw approvals get --gateway --json`), one line a sample:
# READS <epoch.ms> <pid,pid..|->
end=$(( $(date +%s) + ${SECS:-300} ))
while [ $(date +%s) -lt $end ]; do p=$(pgrep -f 'openclaw approvals get --gateway' | tr '\n' ',' | sed 's/,$//'); echo "READS $(date +%s.%N | cut -c1-14) ${p:--}"; sleep 0.5; done
echo "READS END"
