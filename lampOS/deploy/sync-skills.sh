#!/usr/bin/env bash
# sync-skills.sh: a hand install's skill folders (the first builder, #3126, 23 Oct 06:39).
#
# A hand install swaps binaries but not skill folders, so a lamp lacks every product skill
# added since its install (06c1 had no startle, 4ace no reminders). This puts each skill of
# lamp-update's install::PRODUCT_SKILLS, as the build REV has it, into the lamp's
# /var/lib/lampOS/skills, the way install does: every file but the skill's tests/ and hidden
# files, 0644 in 0755 folders, and the skills of install::PRODUCT_SKILLS_OFF with
# install::OFF_LINE above their skill.toml. A skill the build ships off (the off line, or its
# own `enabled = false`, as Bedtime and Morning) that the lamp has turned on stays on, as a
# round keeps it (product_skills::keep_turned_on): when the lamp's skill.toml turns it on, or
# lamp-skills' state names it in `on` and the lamp's file is missing or still the build's. It
# is then written as the build's file turned on. A file the lamp turned off again (off, and not
# the build's) wins over `on`: it is left as it is, and said.
#
# Dry-run unless --apply: it lists each file it would add or change. It never deletes: a folder
# the build doesn't name (a skill the person installed) is never read or written, and a file
# in a product folder that the build lacks is left and listed. It never writes lamp-skills'
# state. Each file goes in by a rename, so lamp-skills, which reloads the folder by itself,
# never reads half a file; a changed file keeps its owner.
#
# Usage: sync-skills.sh [--apply] [--rev REV] [--lamp SSH_TARGET] [--dir DIR] [--state-dir DIR]
#   --rev REV         the build the lamp's parts run (default HEAD of this checkout)
#   --lamp TARGET     on that lamp over ssh; with --apply, sudo there: its password from
#                     LAMP_SUDO given at run time, or none if sudo needs none. Without it,
#                     here (the lamp's own shell). LAMP_SSH replaces the ssh command, which is
#                     `ssh -o BatchMode=yes` (key only): `LAMP_SSH="sshpass -e ssh"` with SSHPASS
#                     for a lamp reached by password
#   --dir DIR         the skills folder (default /var/lib/lampOS/skills; another for a test)
#   --state-dir DIR   lamp-skills' --state-dir, where `on` is (default /var/lib/lampos-skills-state)
# After --apply, restart the parts that read the folders at start (lamp-voice, lamp-expression;
# lamp-skills reloads by itself) and check their journals for "skill.toml: No such file".
# Runs on bash 3.2 (a Mac's) as well; the lamp's side needs only POSIX sh and its own tools.
set -euo pipefail

usage="usage: sync-skills.sh [--apply] [--rev REV] [--lamp SSH_TARGET] [--dir DIR] [--state-dir DIR]"
apply=0 rev=HEAD lamp= dir=/var/lib/lampOS/skills state=/var/lib/lampos-skills-state
while [[ $# -gt 0 ]]; do
  case $1 in
    --apply) apply=1 ;;
    --rev) rev=${2:?--rev REV}; shift ;;
    --lamp) lamp=${2:?--lamp SSH_TARGET}; shift ;;
    --dir) dir=${2:?--dir DIR}; shift ;;
    --state-dir) state=${2:?--state-dir DIR}; shift ;;
    *) echo "$usage" >&2; exit 2 ;;
  esac
  shift
done
plain_path='^/[A-Za-z0-9/._-]*$'
[[ $dir =~ $plain_path && $state =~ $plain_path ]] || { echo "sync-skills: --dir and --state-dir are absolute paths of letters, digits and /._-" >&2; exit 2; }

here=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$here" rev-parse --show-toplevel)
commit=$(git -C "$repo" rev-parse --verify "$rev^{commit}")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
sum() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }

# The build's skills and install's lists, as REV has them.
git -C "$repo" archive "$commit" lampOS/skills lampOS/crates/update/src/install.rs | tar -x -C "$work"
install_rs=$work/lampOS/crates/update/src/install.rs
block() {  # the lines of install.rs's `pub const $1: …` list
  awk -v list="pub const $1:" 'index($0, list) == 1 { on = 1; next } on && /^\];/ { exit } on' "$install_rs"
}
names() {  # its skill names, one a line: each alone on its line as rustfmt writes them
  block "$1" | sed -n 's/^ *"\([a-z0-9-]*\)",$/\1/p'
}
product=() off=()
while IFS= read -r name; do product+=("$name"); done < <(names PRODUCT_SKILLS)
while IFS= read -r name; do off+=("$name"); done < <(names PRODUCT_SKILLS_OFF)
# Every quoted name in PRODUCT_SKILLS was read (none two to a line), and every off skill is one.
quoted=$(block PRODUCT_SKILLS | grep -v '^ *//' | grep -o '"[^"]*"' | wc -l | tr -d ' ')
[[ ${#product[@]} -gt 0 && ${#off[@]} -gt 0 && $quoted -eq ${#product[@]} ]] || { echo "sync-skills: install.rs at ${commit:0:9}: PRODUCT_SKILLS read as ${#product[@]} names of $quoted quoted, PRODUCT_SKILLS_OFF as ${#off[@]}" >&2; exit 1; }
listed() {  # whether $1 is one of the names after it
  local name=$1 each; shift
  for each in "$@"; do [[ $each == "$name" ]] && return 0; done
  return 1
}
for name in "${off[@]}"; do
  listed "$name" "${product[@]}" || { echo "sync-skills: $name is in PRODUCT_SKILLS_OFF but not PRODUCT_SKILLS at ${commit:0:9}" >&2; exit 1; }
done
off_line=$(grep -o '"# Installed off until its lamp check[^"]*"' "$install_rs" | head -n 1)
[[ -n $off_line ]] || { echo "sync-skills: install.rs at ${commit:0:9} has no OFF_LINE as read here" >&2; exit 1; }
off_line=${off_line#\"}; off_line=${off_line%\"}
printf '%b' "$off_line" >"$work/off-line"

# skill.toml's top-level `enabled`, as lamp-skills reads it (on when absent); and the file turned on.
says_on() {
  awk '{ line = $0; sub(/^[ \t]+/, "", line) } line ~ /^\[/ { exit }
    index(line, "=") { key = substr(line, 1, index(line, "=") - 1); gsub(/[ \t]/, "", key)
      if (key == "enabled") { v = substr(line, index(line, "=") + 1); sub(/#.*/, "", v); gsub(/[ \t]/, "", v); off = (v == "false"); exit } }
    END { exit off }' "$1"
}
turned_on() {  # $1 the build's own file (no off line): a top-level `enabled = false` made true
  awk '{ line = $0; t = line; sub(/^[ \t]+/, "", t) } t ~ /^\[/ { tables = 1 }
    !tables && index(line, "=") { k = substr(line, 1, index(line, "=") - 1); v = substr(line, index(line, "=") + 1)
      gsub(/[ \t]/, "", k); gsub(/^[ \t]+|[ \t]+$/, "", v); if (k == "enabled" && v == "false") sub(/false/, "true", line) }
    { print line }' "$1"
}

# The lamp's side: its files now by sha256, `on`, and the skill.toml of each skill shipped off.
ssh_cmd=${LAMP_SSH:-ssh -o BatchMode=yes}
on_lamp() {
  if [[ -n $lamp ]]; then $ssh_cmd "$lamp" "$1"; else sh -c "$1"; fi
}
lamp_sums=$(on_lamp "cd $dir 2>/dev/null && find . -type f -exec sh -c 'if command -v sha256sum >/dev/null; then sha256sum \"\$@\"; else shasum -a 256 \"\$@\"; fi' sh {} +" || true)
turned_on_list=$(on_lamp "cat $state/on 2>/dev/null" || true)
lamp_sum_of() {  # the lamp's sha256 of $1 (a path under the folder), or nothing
  awk -v f="./$1" '{ name = substr($0, 67) } name == f { print $1; exit }' <<<"$lamp_sums"
}

# The build's files as the lamp gets them, staged under $work/stage.
stage=$work/stage
mkdir -p "$stage"
: >"$work/kept-off"
for skill in "${product[@]}"; do
  from=$work/lampOS/skills/$skill
  [[ -f $from/skill.toml ]] || { echo "sync-skills: $skill is in PRODUCT_SKILLS but has no skill.toml at ${commit:0:9}" >&2; exit 1; }
  (cd "$from" && find . -path ./tests -prune -o -type f ! -name '.*' ! -path '*/.*' -print) | sed 's#^\./##' | while IFS= read -r file; do
    mkdir -p "$(dirname "$stage/$skill/$file")"
    cp "$from/$file" "$stage/$skill/$file"
  done
  if listed "$skill" "${off[@]}"; then
    cat "$work/off-line" "$from/skill.toml" >"$stage/$skill/skill.toml"
  fi
  says_on "$stage/$skill/skill.toml" && continue
  # Shipped off: kept on if the lamp's file turns it on, or if lamp-skills' `on` names it and
  # the lamp's file is missing or still the build's own. A file the lamp turned off again
  # (off, and not the build's) is the person's last choice, over `on` (keep_turned_on): it is
  # left as it is, not written, and said.
  why=
  named=0
  if grep -qx "$skill" <<<"$turned_on_list"; then named=1; fi
  lamp_toml=$(lamp_sum_of "$skill/skill.toml")
  if [[ -n $lamp_toml ]]; then
    on_lamp "cat $dir/$skill/skill.toml" >"$work/lamp.toml"
    if says_on "$work/lamp.toml"; then
      why="its skill.toml on this lamp turns it on"
    elif [[ $lamp_toml != "$(sum "$stage/$skill/skill.toml" | cut -d' ' -f1)" && $named == 1 ]]; then
      rm "$stage/$skill/skill.toml"
      echo "$skill/skill.toml" >>"$work/kept-off"
      echo "left off: $skill ($state/on names it, but its skill.toml on this lamp was turned off again and isn't the build's: the person's last choice; not written)"
      continue
    fi
  fi
  if [[ -z $why && $named == 1 ]]; then why="named in $state/on"; fi
  if [[ -n $why ]]; then
    turned_on "$from/skill.toml" >"$stage/$skill/skill.toml"
    echo "kept on: $skill ($why; written as the build's file turned on)"
  fi
done

# What changes.
adds=0 changes=0 same=0
: >"$work/write"
while IFS= read -r file; do
  now=$(lamp_sum_of "$file")
  if [[ -z $now ]]; then echo "add: $file"; adds=$((adds + 1)); echo "$file" >>"$work/write"
  elif [[ $now != "$(sum "$stage/$file" | cut -d' ' -f1)" ]]; then echo "change: $file"; changes=$((changes + 1)); echo "$file" >>"$work/write"
  else same=$((same + 1)); fi
done < <(cd "$stage" && find . -type f | sed 's#^\./##' | sort)
for skill in "${product[@]}"; do
  awk -v s="./$skill/" '{ name = substr($0, 67) } index(name, s) == 1 { print substr(name, 3) }' <<<"$lamp_sums" | while IFS= read -r file; do
    [[ -f $stage/$file ]] || grep -qxF "$file" "$work/kept-off" || echo "left: $file (on the lamp, not the build's; never deleted)"
  done
done
echo "build ${commit:0:9}: ${#product[@]} product skills; $adds to add, $changes to change, $same the same, in $dir${lamp:+ on $lamp}"

[[ $apply == 1 ]] || { [[ $((adds + changes)) -eq 0 ]] || echo "dry run: nothing written (--apply writes them)"; exit 0; }
[[ $((adds + changes)) -gt 0 ]] || exit 0

# Write them: unpacked beside the folder, then each renamed into place (0644 in 0755 folders, a
# changed file keeping its owner when run as root); the unpacked copy is removed either way.
# POSIX sh, given the folder, the archive and the folder's parent; no single quotes in it.
put='umask 022
dir=$1 archive=$2 parent=$3
mkdir -p "$dir" || exit 1
in=$(mktemp -d "$parent/.sync-skills.XXXXXX") || exit 1
s=0
if tar -x --no-same-owner --no-same-permissions -C "$in" -f "$archive"; then
  (cd "$in" && find . -type f | while IFS= read -r f; do
    mkdir -p "$dir/$(dirname "$f")" || exit 1
    if [ "$(id -u)" = 0 ] && [ -e "$dir/$f" ]; then chown "$(stat -c %u:%g "$dir/$f")" "$f"; fi
    mv -f "$f" "$dir/$f" || exit 1
  done) || s=1
else
  s=1
fi
rm -rf "$in"
exit $s'
tar -C "$stage" -cf "$work/write.tar" -T "$work/write"
parent=$(dirname "$dir")
if [[ -n $lamp ]]; then
  remote=$(on_lamp "mktemp")
  on_lamp "cat > $remote" <"$work/write.tar"
  # The copy sent over is removed outside sudo, so a refused sudo leaves nothing behind.
  run="sh -c '$put' sh $dir $remote $parent; s=\$?; rm -f $remote; exit \$s"
  if [[ -n ${LAMP_SUDO:-} ]]; then
    printf '%s\n' "$LAMP_SUDO" | on_lamp "sudo -S -p '' $run"
  else
    on_lamp "sudo -n $run"
  fi
else
  sh -c "$put" sh "$dir" "$work/write.tar" "$parent"
fi
echo "written: $((adds + changes)) files. Restart lamp-voice and lamp-expression, then check their journals for \"skill.toml: No such file\"."
