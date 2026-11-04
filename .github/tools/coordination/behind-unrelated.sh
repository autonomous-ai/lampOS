#!/bin/zsh
# behind-unrelated.sh [--no-crates] TRAIN_HEAD [LAMP_VOICE] (4 Nov, #5212 part 2): may a green train that is behind lamp-voice merge as
# it is, by rebase, instead of being rebuilt from scratch (~30 min)? Train 597 (#5201) went green at 13:26 and was
# thrown away because f5f3dda32, a factory script only (robots/lamp/hardware/test/tools/factory-run.sh), landed on
# lamp-voice under it.
# Read-only: it runs git merge-base, git diff and git show, and changes nothing (DRY=1 or not).
# Prints one line, the decision, and exits 0 for "merge as is" or 1 for "rebuild" (an error rebuilds: fail closed).
# Merge as is only when, from the train's base (its merge base with LAMP_VOICE, default origin/lamp-voice):
#   - the files lamp-voice gained and the files the train changes are disjoint (renames count as both paths), and
#   - no gained file is a build input: nothing under lampOS/crates/, no Cargo.* anywhere, nothing under
#     .github/workflows/, and no other file the lampOS ARM64 run takes (lampos-arm64.yml's pull_request paths: lampOS/**
#     but the docs' Markdown and lampOS/builders/, with the docs it takes back in, plus its .github scripts and the
#     promote tools). The crates' tests read lampOS/deploy, lampOS/skills, lampOS/etc and those docs, and ci_paths.rs
#     keeps that list complete, so a gained change there could break the train's tests unseen.
# Anything else rebuilds, as before. The input test is lib.sh's coord_input, shared with the lanes (part 3); it also
# counts every path lampos-sdk.yml takes (lampOS/docs/control* and skill-format*, Thirteen's P3 on #5289).
# --no-crates (part 3, the no-crates lane's trains): when the train itself changes no build input, a gained input no
# longer rebuilds it: none of its checks builds or reads one, so a crate train's merge doesn't throw away a no-crates
# train. It still rebuilds on any overlap, on a gained workflow (its checks changed), and, when it changes a lampOS doc,
# on any gained lampOS doc (the doc-links check reads them all). A train that does change an input gets the test above.
# REPO_DIR from the environment (README.md), default: the repo holding this folder.
source ${0:A:h}/lib.sh
nc=0; [[ $1 == --no-crates ]] && { nc=1; shift; }
cd "${REPO_DIR:-$(git -C ${0:A:h} rev-parse --show-toplevel)}" || { echo "rebuild: no repository"; exit 1; }
h=${1:?TRAIN_HEAD}; lv=${2:-origin/lamp-voice}
p=""; [[ ${DRY:-0} == 1 ]] && p="dry: "
say() { print -r -- "$p$*"; }
# first3 ITEM...: the first three, comma-separated, then ", ..." when there are more.
first3() { local a; a=("$@"); print -rn -- "${(j:, :)a[1,3]}"; (( $# > 3 )) && print -rn -- ", ..."; return 0; }
git rev-parse -q --verify "$h^{commit}" >/dev/null || { say "rebuild: train head $h can't be read"; exit 1; }
git rev-parse -q --verify "$lv^{commit}" >/dev/null || { say "rebuild: lamp-voice $lv can't be read"; exit 1; }
base=$(git merge-base $lv $h) || { say "rebuild: no merge base of the train and lamp-voice"; exit 1; }
if git merge-base --is-ancestor $lv $h; then say "rebuild: not behind lamp-voice (nothing to decide)"; exit 1; fi
gained=$(git diff --no-renames --name-only $base $lv) || { say "rebuild: lamp-voice's gain since ${base:0:9} can't be read"; exit 1; }
own=$(git diff --no-renames --name-only $base $h) || { say "rebuild: the train's files can't be read"; exit 1; }
[[ -n $gained ]] || { say "rebuild: lamp-voice moved but gained no file (a merge commit?)"; exit 1; }
[[ -n $own ]] || { say "rebuild: the train changes no file"; exit 1; }
g=(${(f)gained}); o=(${(f)own})
# The inputs as lamp-voice's own workflows name them (lib.sh; unread workflows: every lampOS doc is an input).
coord_inputs_init $lv
input() { coord_input $1; }
both=(${g:*o})
if (( ${#both} )); then
  say "rebuild: lamp-voice gained ${#g} files since ${base:0:9}, ${#both} also in the train: $(first3 $both)"
  exit 1
fi
if (( nc )); then
  mine=(); for f in $o; do k=$(input $f); [[ -n $k ]] && { mine+="$f ($k)"; break; }; done
  if (( ${#mine} )); then
    nc=0; say "no-crates test not taken: the train changes a build input, ${mine[1]}"
  else
    bad=(); for f in $g; do [[ $f == .github/workflows/* ]] && bad+="$f (workflows)"; done
    if (( ${#bad} )); then
      say "rebuild: lamp-voice gained ${#g} files since ${base:0:9}, ${#bad} under .github/workflows (the checks changed): $(first3 $bad)"
      exit 1
    fi
    if (( ${o[(I)lampOS/docs/*]} )); then
      bad=(); for f in $g; do [[ $f == lampOS/docs/* ]] && bad+=$f; done
      if (( ${#bad} )); then
        say "rebuild: the no-crates train changes lampOS docs and lamp-voice gained ${#bad} more (the doc-links check reads them all): $(first3 $bad)"
        exit 1
      fi
    fi
    say "merge as is (no-crates train): gained ${#g} files since ${base:0:9} ($(first3 $g)), none overlapping the train's ${#o}, none a workflow; the train changes no build input"
    exit 0
  fi
fi
bad=(); for f in $g; do k=$(input $f); [[ -n $k ]] && bad+="$f ($k)"; done
if (( ${#bad} )); then
  say "rebuild: lamp-voice gained ${#g} files since ${base:0:9}, ${#bad} in crates/Cargo/workflows or another ARM64 input: $(first3 $bad)"
  exit 1
fi
say "merge as is: gained ${#g} files since ${base:0:9} ($(first3 $g)), none overlapping the train's ${#o}, none in crates/Cargo/workflows or another ARM64 input"
exit 0
