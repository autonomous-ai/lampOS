#!/bin/sh
# Mac (the shared Mac whose podman VM is the climb rig): sh run.sh <step>
#   up       a new container $RIG (debian:bookworm, linux/arm64 under qemu), the stand-ins in it
#   push     the kit and the staged channels into it (COPYFILE_DISABLE=1 tar --exclude='._*')
#   make     build B's manifest in the rig with B's own lamp-update (no aarch64 on the Mac)
#   sign     sign A's and B's manifests with the test key (piped from stage/key/ on stdin)
#   install  the lamp on build A: A's own `install --setup on`, key.pub = production key
#   snap     keep the lamp as found (S0)
#   leg1     round r1: A's updater switches to B, self-switches, B confirms and rounds
#   leg2     delete lamp-npu.service, edit two units, rounds r2 and r3
#   judge    read the results (mac/judge.py): LEG1 and LEG2 verdicts
#   putback  S0 back, read back twice
#   clean    remove the container (only $RIG, labelled by this kit)
#   go       up push make sign install snap leg1 leg2 judge putback; putback also on a failure
# Env: PODMAN (default "podman"; e.g. "podman --connection lampos-arm64"), RIG (default el-8400).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ST=$HERE/stage
PODMAN=${PODMAN:-podman}
RIG=${RIG:-el-8400}
IMAGE=docker.io/library/debian:bookworm
K=/var/tmp/el-8400
LABEL=el-8400=kit
[ -f "$ST/STAGE.env" ] || { echo "run: no stage/STAGE.env: sh stage.sh <head> first"; exit 1; }
. "$ST/STAGE.env"
mkdir -p "$HERE/results"
CUR=$HERE/results/CURRENT
die() { echo "run: STOP: $*"; exit 1; }
# shellcheck disable=SC2086
P() { $PODMAN "$@"; }
R() { P exec -i "$RIG" "$@"; }
RS() { P exec -i "$RIG" sh -c "$1"; }
out() { [ -f "$CUR" ] || die "no results/CURRENT: run 'up' first"; cat "$CUR"; }
ours() { [ "$(P inspect --format '{{ index .Config.Labels "el-8400" }}' "$RIG" 2>/dev/null)" = kit ]; }
pull() { O=$(out); mkdir -p "$O"; R tar -cf - -C "$K" out calls.log | tar -xf - -C "$O"; }
push_dir() { # $1 = folder under $2, copied to $K/$1 in the rig
  n=$(find "$2/$1" -name '._*' | wc -l | tr -d ' '); [ "$n" = 0 ] || die "$n ._ files under $2/$1"
  (cd "$2" && COPYFILE_DISABLE=1 tar --exclude='._*' -cf - "$1") | R tar -xf - -C "$K" || die "copy of $1 failed"
}

step() {
  case "$1" in
  up)
    P container exists "$RIG" 2>/dev/null && die "a container $RIG exists: pick another RIG=, or 'clean' it if it is this kit's"
    O=$HERE/results/$RIG-$(date +%m%d-%H%M); mkdir -p "$O"; echo "$O" > "$CUR"
    P ps -a --format '{{.Names}}' | sort > "$O/containers-before.txt"
    P run -d --name "$RIG" --label "$LABEL" --platform linux/arm64 "$IMAGE" sleep infinity >/dev/null || die "podman run failed"
    [ "$(R uname -m)" = aarch64 ] || die "the container is not aarch64"
    RS "mkdir -p $K"
    push_dir lamp "$HERE"
    RS "sh $K/lamp/rig-init.sh" || die "rig-init failed" ;;
  push)
    for f in STAGE.env test.pub prod.pub; do
      (cd "$ST" && COPYFILE_DISABLE=1 tar --exclude='._*' -cf - "$f") | R tar -xf - -C "$K" || die "copy of $f failed"
    done
    push_dir lamp "$HERE"; push_dir chanA "$ST"; push_dir chanB "$ST"; push_dir srcB "$ST"
    [ "$(RS "find $K -name '._*' | wc -l")" = 0 ] || die "._ files in the rig's $K"
    echo "run: pushed: $(RS "du -sh $K | cut -f1")" ;;
  make)
    RS "MODELS=${MODELS:-1} sh $K/lamp/mkchan.sh make" || die "make failed"
    R cat "$K/chanB/manifest.json" > "$(out)/chanB-manifest.json"
    R cat "$K/chanA/manifest.json" > "$(out)/chanA-manifest.json" ;;
  sign)
    for c in A B; do
      line=$(R "$K/chan$c/lamp-update" sign --key-file /dev/stdin --manifest "$K/chan$c/manifest.json" \
             --out "$K/chan$c/lampos-manifest.json" < "$ST/key/test.key") || die "sign $c failed: $line"
      echo "run: chan$c: $line"
      [ "$line" = "signed with key $TEST_FP" ] || die "chan$c was not signed with the test key $TEST_FP"
    done
    RS "sh $K/lamp/mkchan.sh check" || die "the signed channels do not read back" ;;
  install) RS "sh $K/lamp/install-a.sh" || { pull; die "install failed"; }; pull ;;
  snap) RS "sh $K/lamp/snap.sh save" || die "snap failed" ;;
  leg1)
    RS "sh $K/lamp/round.sh r1" || die "round r1 failed to run"
    if ! RS "grep -q 'which the lamp runs, adds' $K/out/r1/journal.txt"; then
      echo "run: r1 has no #7018 line: one more round (r1b), the next up-to-date one"
      RS "sh $K/lamp/round.sh r1b" || die "round r1b failed to run"
    fi
    pull ;;
  leg2)
    RS "sh $K/lamp/leg2-edit.sh" || { pull; die "leg2 edits failed"; }
    RS "sh $K/lamp/round.sh r2" || die "round r2 failed to run"
    RS "sh $K/lamp/round.sh r3" || die "round r3 failed to run"
    pull ;;
  judge) python3 -I "$HERE/mac/judge.py" "$(out)" | tee "$(out)/verdict.txt" ;;
  putback) RS "sh $K/lamp/putback.sh"; s=$?; pull; [ $s = 0 ] || exit $s ;;
  clean)
    ours || die "$RIG is not this kit's container (label $LABEL): left alone"
    P rm -f "$RIG" >/dev/null || die "podman rm failed"
    O=$(out); P ps -a --format '{{.Names}}' | sort > "$O/containers-after.txt"
    if diff "$O/containers-before.txt" "$O/containers-after.txt" > /dev/null; then echo "run: CLEAN: $RIG removed; the VM's containers are as before"
    else echo "run: containers before vs after (only $RIG may differ):"; diff "$O/containers-before.txt" "$O/containers-after.txt"; fi
    echo "run: stage/ ($(du -sh "$ST" | cut -f1)) is kept; remove it by hand when the row is posted" ;;
  *) echo "usage: sh run.sh up|push|make|sign|install|snap|leg1|leg2|judge|putback|clean|go"; exit 2 ;;
  esac
}

case "${1:-}" in
go)
  for s in up push make sign install snap; do echo "== $s"; step $s || exit 1; done
  for s in leg1 leg2 judge; do
    echo "== $s"
    if ! ( step $s ); then echo "run: $s stopped: putting the lamp back"; step putback; exit 1; fi
  done
  echo "== putback"; step putback
  echo "run: done. Post $(out)/verdict.txt and the putback lines; then 'sh run.sh clean'." ;;
*) step "${1:-}" ;;
esac
