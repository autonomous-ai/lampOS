#!/bin/zsh
# #8671 row on lamp-75c9 (a bench lamp; its owner's person said yes to a real reset, from 28 Nov 10:00): a factory
# reset empties every engine's home, the voice's marked copies and sent turns' ids, and the text logs with their
# eMMC copy. For El. Structure from rows/8933. ONE real reset, the product's own command, the PR's lamp-setup.
#   ./run.sh check        Mac only: zsh -n / sh -n / dash -n, py_compile, the forbidden-command grep, every judged
#                         string against the PR head cf7dacac6d (git grep), logcount.py on made-up logs, the judge
#                         on made-up dirs, the tgz
#   ./run.sh stage        Mac only: CI's ARM64 artifact (lampos-arm64-<head>) lamp-setup, lamp-voice -> lamp/payload, sha256
#                         against the artifact's SHA256SUMS, the binary grepped for the new commands and lines; the
#                         PR's product plan from git at the head, its sha = RESET_PLAN_SHIPPED's first line
#   ./run.sh preflight    lamp, read-only
#   ./run.sh go           install (dead-man first), leg 1 (dry runs), leg 2b (the PR's voice removes the old clips
#                         folder; the stock voice back), leg 2 (seed), the journal window and the
#                         kit's files copied off, then (only with CONFIRM_RESET=lamp-75c9, at or after 28 Nov
#                         10:00) leg 3, the reset; then waits up to 10 min for the lamp and runs `after`
#   ./run.sh after [DIR]  the lamp again (LAMP_HOST_AFTER, else LAMP_HOST): a live snapshot, the recorder's files,
#                         the journal window since the reset, the put-back (the lamp stays reset), the judge
#   ./run.sh putback [DIR] the journal window and kit files copied off FIRST, then lamp/putback.sh
#   ./run.sh judge DIR    re-judge a saved (or made-up) results dir; writes DIR/row.md
# Never reboots (the reset's own fallback is the reset's); never touches lamp-update, Wi-Fi, NetworkManager or wpa;
# no pattern kills; never opens a DB file; log lines counted, never printed; no sound on the Mac;
# tar with COPYFILE_DISABLE=1 --exclude='._*'.
set -u
zmodload zsh/datetime 2>/dev/null   # optional: the wait loop uses date +%s
HERE=${0:A:h}; KR=/root/el-8671/kit; K=/root/el-8671
EXPECT_HOST=lamp-75c9
REPO=autonomous-ai/lampOS; PR=8671; HEAD=cf7dacac6dd32a1e791919300e19912470ce8c68; SRC=lampos-arm64-$HEAD
PLAN_SHA=07bc294005f1f8a3868eabbafc6c2fef1a4c5119d172124316f7d6eb3ed64360
NOT_BEFORE=202611281000   # the person's yes: from 10:00 on 28 Nov (the Mac's local time, GMT+7)
GITREPO=${GITREPO:?set GITREPO to your lampOS checkout (the repo root)}
EXPECT_BUILD=${EXPECT_BUILD:-}
WAIT_S=${WAIT_S:-600}
need_host() {
  [[ -n ${LAMP_HOST:-} ]] || { echo "REFUSE: LAMP_HOST is not set (lamp-75c9's SSH host, e.g. orangepi@<ip> or an alias): no default"; exit 2; }
  [[ -n ${LAMP_PASS:-} ]] || { echo "REFUSE: LAMP_PASS is not set"; exit 2; }
  [[ $LAMP_HOST == *4ace* ]] && { echo "REFUSE: lamp-4ace is read-only"; exit 2; }
  HOST=$LAMP_HOST; PASS=$LAMP_PASS
}
lamp() {
  local b; b=$(printf '%s' "$1" | base64 | tr -d '\n')
  sshpass -p $PASS ssh -o ConnectTimeout=8 -o ServerAliveInterval=10 -o ServerAliveCountMax=3 $HOST \
    "echo $PASS | sudo -S -p '' env EXPECT_HOST=$EXPECT_HOST sh -c \"\$(echo $b | base64 -d)\""
}
tgz() {
  local out=$HERE/8671-kit.tgz n
  (cd $HERE && rm -f $out && COPYFILE_DISABLE=1 tar --exclude='._*' --exclude='__pycache__' -czf $out lamp) || return 1
  n=$(tar -tzf $out | grep -c '\._'); echo "kit $out: $(tar -tzf $out | wc -l | tr -d ' ') entries, $(du -h $out | cut -f1), ._ files: $n"
  [[ $n == 0 ]]
}
stage() {
  local head art run A=$HERE/stage-art P=$HERE/lamp/payload
  head=$(gh pr view $PR --repo $REPO --json headRefOid --jq .headRefOid) || return 1
  [[ $head = $HEAD ]] || { echo "REFUSE: the PR's head is now $head, not $HEAD: re-read the lines before re-staging"; return 1; }
  art=$(gh api "repos/$REPO/actions/artifacts?name=$SRC&per_page=5" \
    --jq '.artifacts | map(select(.expired|not)) | sort_by(.created_at) | last | "\(.id) \(.workflow_run.id)"')
  [[ -n $art && $art != "null null" ]] || { echo "REFUSE: no unexpired artifact $SRC (CI not done?)"; return 1; }
  run=${art#* }; echo "artifact ${art% *} from run $run"
  gh run view $run --repo $REPO --json jobs --jq '.jobs[] | select(.name|test("ARM64")) | "  \(.name): \(.status) \(.conclusion)"' | tee $HERE/jobs.txt
  grep -qE 'build and test on Linux ARM64 \((release|1/1)\): completed success' $HERE/jobs.txt || { echo "REFUSE: the ARM64 build job is not green"; return 1; }
  rm -rf $A; mkdir -p $A
  gh run download $run --repo $REPO -n $SRC -D $A || return 1
  local s f want got; s=$(find $A -type f -name SHA256SUMS | head -1); [[ -n $s ]] || { echo "REFUSE: no SHA256SUMS"; return 1; }
  rm -rf $P; mkdir -p $P
  f=$(find $A -type f -name lamp-setup | head -1); [[ -n $f ]] || { echo "REFUSE: lamp-setup missing in the artifact"; return 1; }
  want=$(awk '$2=="lamp-setup"||$2=="./lamp-setup"||$2=="*lamp-setup"||$2~"/lamp-setup$"{print $1}' $s | head -1); got=$(shasum -a 256 $f | cut -d' ' -f1)
  [[ -n $want && $want = $got ]] || { echo "REFUSE: lamp-setup $got != SHA256SUMS ${want:-<no line>}"; return 1; }
  file $f | grep -q 'ARM aarch64' || { echo "REFUSE: lamp-setup is not aarch64"; return 1; }
  install -m 755 $f $P/lamp-setup
  local vf vwant; vf=$(find $A -type f -name lamp-voice | head -1); [[ -n $vf ]] || { echo "REFUSE: lamp-voice missing in the artifact"; return 1; }
  vwant=$(awk '$2=="lamp-voice"||$2=="./lamp-voice"||$2=="*lamp-voice"||$2~"/lamp-voice$"{print $1}' $s | head -1)
  [[ -n $vwant && $vwant = $(shasum -a 256 $vf | cut -d' ' -f1) ]] || { echo "REFUSE: lamp-voice != SHA256SUMS ${vwant:-<no line>}"; return 1; }
  file $vf | grep -q 'ARM aarch64' || { echo "REFUSE: lamp-voice is not aarch64"; return 1; }
  install -m 755 $vf $P/lamp-voice
  # leg 2b's lines in the voice (ring_clips.rs remove_old_dir)
  for pc in 'lamp-voice: the old offline ring clips folder was removed (' 'lamp-voice: the old offline ring clips folder was left: not empty '; do
    n=$(LC_ALL=C grep -caF -- "$pc" $P/lamp-voice); echo "  lamp-voice has '$pc': $n"; (( n >= 1 )) || { echo "REFUSE: lamp-voice lacks leg 2b's line"; return 1; }
  done
  # The new commands' and lines' literal pieces (main.rs:45/:51 usage, reset.rs:2182/:2194, logs.rs, reset.rs:151)
  local pc n bad=0
  for pc in 'forget-engine-homes' 'forget-logs' " entries of the engines' homes" ' log entries' \
            'reset: a log could not be emptied: ' 'LAMP_SETUP_RESET_ROOT' ' runs only with --dry-run' 'started in place, no reboot: '; do
    n=$(LC_ALL=C grep -caF -- "$pc" $P/lamp-setup); echo "  binary has '$pc': $n"; (( n >= 1 )) || bad=1
  done
  (( bad )) && { echo "REFUSE: the binary lacks a piece of the PR's lines"; return 1; }
  # The PR's product plan, from git at the head; its sha must be the one the PR ships first
  git -C $GITREPO cat-file -e "${HEAD}^{commit}" 2>/dev/null || git -C $GITREPO fetch -q origin $HEAD || return 1
  git -C $GITREPO show "${HEAD}:lampOS/crates/setup/fixtures/reset-plan-product.json" > $P/reset-plan.json || return 1
  got=$(shasum -a 256 $P/reset-plan.json | cut -d' ' -f1)
  git -C $GITREPO show "${HEAD}:lampOS/crates/update/src/install.rs" | grep -A1 'pub const RESET_PLAN_SHIPPED' | tail -1 | grep -qF "\"$got\"" \
    || { echo "REFUSE: the plan's sha $got is not RESET_PLAN_SHIPPED's first line"; return 1; }
  [[ $got = $PLAN_SHA ]] || { echo "REFUSE: the plan's sha $got != the kit's $PLAN_SHA"; return 1; }
  python3 -I - $P/reset-plan.json <<'EOF' || { echo "REFUSE: the plan is not as the PR says"; return 1; }
import json, sys
w = json.load(open(sys.argv[1]))['wipe']
ok = (len(w) == 64 and '/var/lib/lampOS/voice/marked' in w and '/var/lib/lampOS/voice/history-marks.json' in w
      and '/var/lib/lampOS/voice/history-forget.json' not in w and '/var/lib/lampOS/voice/ring-clips' not in w
      and '/var/lib/lampOS/voice/schedule-delivery' in w)
print('  plan: %d paths; marked, history-marks.json, schedule-delivery in; history-forget.json, voice/ring-clips out: %s' % (len(w), ok))
sys.exit(0 if ok else 1)
EOF
  printf '%s  %s\n%s  %s\n%s  %s\n' $want lamp-setup $vwant lamp-voice $got reset-plan.json > $P/SHA256SUMS
  (cd $P && shasum -a 256 -c SHA256SUMS) || { echo "REFUSE: the payload's SHA256SUMS does not verify"; return 1; }
  print -r -- $HEAD > $P/TAG.txt; print -r -- "artifact ${art% *} run $run lamp-setup $want lamp-voice $vwant plan $got" > $P/ARTIFACT.txt
  rm -rf $A
  tgz
}
push() { # push [kit]: "kit" pushes the scripts only (preflight before staging)
  [[ ${1:-} == kit ]] || [[ -f $HERE/lamp/payload/TAG.txt && $(cat $HERE/lamp/payload/TAG.txt) = $HEAD ]] || { echo "REFUSE: nothing staged for $HEAD: ./run.sh stage"; return 1; }
  local head; head=$(gh pr view $PR --repo $REPO --json headRefOid --jq .headRefOid)
  [[ $head = $HEAD ]] || { echo "REFUSE: the PR's head is $head, the kit is for $HEAD"; return 1; }
  tgz >/dev/null || return 1
  sshpass -p $PASS ssh -o ConnectTimeout=8 $HOST "cat > /tmp/el-8671.tgz" < $HERE/8671-kit.tgz || return 1
  lamp "set -e; mkdir -p $KR; chmod 700 $K; rm -rf $KR/*; tar -xzf /tmp/el-8671.tgz -C $KR --strip-components=1; rm -f /tmp/el-8671.tgz
        find $K -name '._*' -delete; chown -R root:root $K; echo pushed: \$(ls $KR | tr '\n' ' ')"
}
step() { local f=$1; shift; echo "== $f $(date +%T)" >> $RES/go.txt; lamp "$*" 2>&1 | tee -a $RES/$f.txt; }
# The owner's keep-all-data rule: the whole journal window (every unit) and the kit's own files leave the lamp
# BEFORE the reset (its vacuum empties the RAM journal) and before any put-back. Saved, never printed.
pull_window() { # pull_window NAME SINCE_SPEC
  lamp "journalctl --since '$2' --until now -o short-iso-precise --no-pager | gzip -c | base64" | base64 -d > $RES/$1.txt.gz
  echo "$1: $(gunzip -c $RES/$1.txt.gz | wc -l | tr -d ' ') lines, $(du -h $RES/$1.txt.gz | cut -f1) gz (since $2)" | tee -a $RES/go.txt
}
pull_files() { # pull_files NAME: /root/el-8671 without the kit
  lamp "cd $K 2>/dev/null && COPYFILE_DISABLE=1 tar --exclude='._*' --exclude=kit -czf - . 2>/dev/null | base64" | base64 -d > $RES/$1.tgz \
    && mkdir -p $RES/$1 && tar -xzf $RES/$1.tgz -C $RES/$1 && rm -f $RES/$1.tgz && echo "$1: $(find $RES/$1 -type f | wc -l | tr -d ' ') files"
}
since_spec() { local s; s=$(lamp "sed -n 's/^$1=//p' $K/$2 2>/dev/null" | tr -cd 0-9); [[ -n $s ]] && print "@$(( s / 1000000 - ${3:-5} ))" || print '-2 hours'; }
putback() {
  pull_window journal-window-putback-pre "$(since_spec ROW_START_WALL_US row-start.txt 600)"
  pull_files lamp-files-pre-putback
  lamp "test -f $KR/putback.sh && KEEP_RECORDER=${KEEP_RECORDER:-0} sh $KR/putback.sh || echo 'no kit on the lamp'" 2>&1 | tee $RES/putback.log
  grep -q '^PUTBACK-OK' $RES/putback.log
}
judge() { python3 -I $HERE/mac/judge.py $1 > $1/judge.txt; head -1 $1/row.md; echo "row: $1/row.md, the judge's print: $1/judge.txt"; }
newres() { mkdir -p $HERE/results; RES=$HERE/results/run-$(date +%H%M); mkdir -p $RES; echo "results: $RES"; }
lastres() { RES=${1:-$(ls -d $HERE/results/run-*(/om[1]) 2>/dev/null)}; [[ -n $RES && -d $RES ]] || { echo "REFUSE: no results dir"; exit 2; }; echo "results: $RES"; }
after() {
  [[ -n ${LAMP_HOST_AFTER:-} ]] && HOST=$LAMP_HOST_AFTER
  lamp "hostname" 2>/dev/null | grep -qx $EXPECT_HOST || { echo "AFTER: $EXPECT_HOST does not answer at $HOST" | tee -a $RES/go.txt; return 1; }
  echo "AFTER $(date +%T) via $HOST" | tee -a $RES/go.txt
  step snap-live "sh $KR/snap.sh live"
  pull_files after-files
  pull_window journal-window-after "$(since_spec RESET_WALL_US reset-start.txt 5)"
  putback
  judge $RES
}
go() {
  newres; echo "GO $(date +%T); results $RES; host $HOST" | tee $RES/go.txt
  local armed=0 triggered=0
  on_exit() { (( armed )) || return 0; armed=0
    if (( triggered )); then echo "the lamp was reset: nothing to put back now; ./run.sh after $RES once it answers"
    else echo "putting back $(date +%T) (no reset)"; putback; judge $RES; fi; }
  trap 'echo "interrupted"; on_exit; exit 130' INT TERM
  trap 'on_exit' EXIT
  push || return 1
  armed=1
  lamp "EXPECT_BUILD=$EXPECT_BUILD sh $KR/install.sh" 2>&1 | tee $RES/install.txt
  grep -q '^INSTALLED' $RES/install.txt || { echo "INSTALL STOPPED: putting back"; on_exit; return 1; }
  # -- leg 1: the dry runs; any refusal stops the row before the seed
  step leg1 "sh $KR/leg1.sh"
  pull_files leg1-files
  python3 -I $HERE/mac/judge.py --leg1 $RES || { echo "LEG 1 NOT OK: no seed, no reset; putting back" | tee -a $RES/go.txt; on_exit; return 1; }
  # -- leg 2b (the first builder's): the PR's voice removes the old clips folder at its start; the stock voice back
  step oldclips "CLEAR_OK=${CLEAR_OK:-0} sh $KR/oldclips.sh"
  grep -q '^LEG2B-DONE' $RES/oldclips.txt && grep -q '^STOCK_BACK=yes' $RES/oldclips.txt && grep -q '^STOCK_MUTED=no' $RES/oldclips.txt \
    || { echo "LEG 2B NOT DONE or the stock voice not back: no reset; putting back" | tee -a $RES/go.txt; on_exit; return 1; }
  # -- leg 2: the seed and the before
  step seed "SEED_HOMES=${SEED_HOMES:-0} sh $KR/seed.sh"
  grep -q '^SEED-OK' $RES/seed.txt || { echo "SEED NOT OK: no reset; putting back" | tee -a $RES/go.txt; on_exit; return 1; }
  # -- the journal window and the kit's files, before the reset's vacuum
  pull_window journal-window-before "$(since_spec ROW_START_WALL_US row-start.txt 600)"
  pull_files lamp-files-before
  # -- leg 3: the reset, only with the explicit confirm and the person's time
  if [[ ${CONFIRM_RESET:-} != $EXPECT_HOST ]]; then echo "NO RESET: CONFIRM_RESET is not $EXPECT_HOST: legs 1-2 only; putting back" | tee -a $RES/go.txt; on_exit; return 0; fi
  [[ $(date +%Y%m%d%H%M) -ge $NOT_BEFORE ]] || { echo "NO RESET: before the person's 10:00; putting back" | tee -a $RES/go.txt; on_exit; return 1; }
  step trigger "CONFIRM_RESET=$CONFIRM_RESET sh $KR/trigger.sh"
  if grep -q '^TRIGGERED' $RES/trigger.txt || lamp "test -s $K/reset-start.txt && echo yes" 2>/dev/null | grep -q yes; then triggered=1
  else echo "TRIGGER NOT OK: putting back" | tee -a $RES/go.txt; on_exit; return 1; fi
  echo "RESET STARTED $(date +%T): waiting up to $WAIT_S s for the lamp" | tee -a $RES/go.txt
  local t0=$(date +%s) st
  [[ -n ${LAMP_HOST_AFTER:-} ]] && HOST=$LAMP_HOST_AFTER
  while (( $(date +%s) - t0 < WAIT_S )); do
    st=$(lamp "cat $K/after/status.txt 2>/dev/null" 2>/dev/null)
    if [[ $st == *SNAP\ 0* ]]; then echo "back after $(( $(date +%s) - t0 )) s; the recorder: $(print -r -- $st | awk '{print $1}' | tr '\n' ' ')" | tee -a $RES/go.txt; break; fi
    sleep 10
  done
  if [[ $st != *SNAP\ 0* ]]; then
    echo "FAIL: not back in $WAIT_S s at $HOST (setup mode is off this network by design: the recorder keeps the after legs on the lamp); then: LAMP_HOST_AFTER=<its address> ./run.sh after $RES" | tee -a $RES/go.txt $RES/wait.txt
    armed=0; judge $RES; return 1
  fi
  armed=0
  after
}
check() {
  local ok=0 f n=0 bad=0 l
  zsh -n $HERE/run.sh && echo "zsh -n ok: run.sh" || ok=1
  for f in $HERE/lamp/*.sh; do zsh -n $f && sh -n $f && dash -n $f && echo "zsh -n, sh -n, dash -n ok: ${f#$HERE/}" || ok=1; done
  if command -v shellcheck >/dev/null; then shellcheck -s sh -e SC1091,SC2086,SC2009,SC2015,SC2046 $HERE/lamp/*.sh || ok=1; else echo "shellcheck: not installed (skipped)"; fi
  for f in $HERE/mac/*.py $HERE/lamp/*.py; do python3 -I -m py_compile $f && echo "py_compile ok: ${f#$HERE/}" || ok=1; done
  rm -rf $HERE/mac/__pycache__ $HERE/lamp/__pycache__
  grep -nE 'sqlite|\.db\b|(^|[;&|(] *)(reboot|shutdown|pkill|killall|nmcli|wpa_cli|say|osascript)\b|systemctl (start|stop|restart|reload|enable|disable|mask|kill) (lamp-update|NetworkManager|wpa_supplicant|lampos|lamp-voice|lamp-setup )|ifdown|ip link set|nft (add|-f)|iptables -[AIN]|lamp-keeper' $HERE/lamp/*.sh $HERE/lamp/*.py | grep -vE ':[0-9]+: *#' && { echo "FAIL: a forbidden command"; ok=1; } || echo "kit: no DB, reboot, Wi-Fi, pattern kill, updater, unit restart, firewall or sound command"
  grep -nE '(^|[;&|(] *)(say|osascript|afplay)\b' $HERE/run.sh | grep -v 'grep -nE' && { echo "FAIL: a sound command on the Mac"; ok=1; }
  grep -nE 'reset-now' $HERE/lamp/*.sh | grep -vE ':[0-9]+: *#' | grep -v '^.*/trigger.sh:' && { echo "FAIL: reset-now outside trigger.sh"; ok=1; } || echo "reset-now: only in trigger.sh"
  grep -nE "(cat|head|tail|less|sed -n|awk)[^|]*(/var/log|syslog|log\.hdd)" $HERE/lamp/*.sh | grep -vE ':[0-9]+: *#' && { echo "FAIL: a log read that is not a count"; ok=1; } || echo "logs: counted only (grep -c, logcount.py)"
  git -C $GITREPO cat-file -e "${HEAD}^{commit}" 2>/dev/null || git -C $GITREPO fetch -q origin $HEAD 2>/dev/null
  echo "PR head $(git -C $GITREPO rev-parse --short=10 "${HEAD}^{commit}")"
  while IFS=$'\t' read -r f s; do
    [[ $f == \#* || -z $f ]] && continue; n=$((n + 1))
    l=$(git -C $GITREPO grep -nF -e "$s" $HEAD -- "lampOS/${f%%:*}" 2>/dev/null | cut -d: -f3 | head -3 | tr '\n' ',')
    [[ -n $l ]] || { echo "STRING GONE: $f: $s"; bad=1; continue; }
    [[ $f == *:* && ",$l" != *",${f##*:},"* ]] && echo "MOVED: $f now at line(s) $l: $s"
  done < $HERE/mac/strings.tsv
  (( bad )) && ok=1 || echo "strings: all $n found at $HEAD with git grep"
  python3 -I $HERE/mac/fake_logs.py $HERE/lamp/logcount.py || ok=1
  python3 -I $HERE/mac/fake.py $HERE/results || ok=1
  for d in $HERE/results/fake-*(/); do python3 -I $HERE/mac/judge.py $d > $d/judge.txt; done
  python3 -I $HERE/mac/fake.py --verify $HERE/results || ok=1
  rm -rf $HERE/mac/__pycache__
  tgz || ok=1
  (( ok )) && echo "CHECK: FAIL" || echo "CHECK: OK"
  return $ok
}
RES=${RES:-}
case ${1:-} in
  check) check;;
  stage) stage;;
  preflight) need_host; newres; push kit && lamp "EXPECT_BUILD=$EXPECT_BUILD sh $KR/preflight.sh" 2>&1 | tee $RES/preflight.txt;;
  go) need_host; go;;
  after) need_host; lastres ${2:-}; after;;
  putback) need_host; lastres ${2:-}; putback;;
  judge) judge ${2:?dir};;
  *) sed -n '2,24p' $0; exit 2;;
esac
