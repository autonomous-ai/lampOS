#!/bin/sh
# In the rig: the lamp as found (after A's install, before any round), kept and put back.
#   save     keeps S0: a tar of the small trees, /opt/lampOS without its build folders (each
#            /opt/lampOS/<part>/<sha256>/ is content-addressed and never rewritten), the list
#            of every path under /opt/lampOS, and a digest of all of it.
#   digest   prints the digest now (paths, modes, owners, link targets, sizes, SHA-256 of
#            every regular file outside the build folders).
#   check    the digest now against S0's: SAME or the differences.
#   restore  removes what S0 did not have, puts S0's files and links back.
set -u
. /var/tmp/el-8400/lamp/common.sh
rig_only
S0=$K/s0
TREES="/etc/systemd/system /etc/lampOS /etc/tmpfiles.d /usr/local/lib/lampos /var/lib/el-8400-stub"
HASH='.*/[0-9a-f]\{64\}'

trees() {
  for t in $TREES /var/lib/lampOS /var/lib/lampos*; do [ -e "$t" ] && echo "$t"; done | sort -u
}

digest() {
  for t in $(trees); do
    find "$t" -printf '%y %m %u:%g %s %p -> %l\n' | sort
    find "$t" -type f -exec sha256sum {} + | sort -k2
  done
  for f in /etc/passwd /etc/group; do sha256sum "$f"; done
  find /opt/lampOS -printf '%y %m %u:%g %s %p -> %l\n' | sort
  find /opt/lampOS -regex "$HASH" -prune -o -type f -exec sha256sum {} + | sort -k2
}

case "${1:-}" in
save)
  [ -e "$S0/digest.txt" ] && die "S0 is kept already ($S0): put back first, or remove it by hand if this is a new rig"
  [ -e /opt/lampOS/update/current ] || die "nothing installed to keep"
  mkdir -p "$S0"
  trees > "$S0/trees.txt"
  tar --numeric-owner -cpf "$S0/trees.tar" -T "$S0/trees.txt" || die "tar of the trees failed"
  find /opt/lampOS -mindepth 1 | sort > "$S0/opt.list"
  find /opt/lampOS -regex "$HASH" -prune -o -print | sort > "$S0/opt.small"
  tar --numeric-owner --no-recursion -cpf "$S0/opt.tar" -T "$S0/opt.small" || die "tar of /opt/lampOS failed"
  ls /var/lib | grep '^lampos' > "$S0/varlib.txt" || true
  digest > "$S0/digest.txt"
  say "S0-SAVED: $(wc -l < "$S0/digest.txt") digest lines, $(du -sh "$S0" | cut -f1)"
  ;;
digest) digest ;;
check)
  [ -f "$S0/digest.txt" ] || die "no S0"
  digest > "$K/digest.now"
  if cmp -s "$S0/digest.txt" "$K/digest.now"; then say "SAME as S0"; else
    say "DIFFERS from S0:"; diff "$S0/digest.txt" "$K/digest.now" | head -40; exit 1; fi
  ;;
restore)
  [ -f "$S0/digest.txt" ] || die "no S0"
  # What S0 did not have under /opt/lampOS: removed deepest first.
  find /opt/lampOS -mindepth 1 | sort > "$K/opt.now"
  comm -13 "$S0/opt.list" "$K/opt.now" | sort -r | while IFS= read -r p; do rm -rf -- "$p"; done
  # New /var/lib/lampos* folders a round made.
  ls /var/lib | grep '^lampos' | while read -r d; do grep -qx "$d" "$S0/varlib.txt" || rm -rf -- "/var/lib/$d"; done
  # The small trees whole, as they were.
  while IFS= read -r t; do case "$t" in /etc/*|/usr/local/lib/lampos|/var/lib/*) rm -rf -- "$t";; *) die "odd tree $t";; esac; done < "$S0/trees.txt"
  tar --numeric-owner -xpf "$S0/trees.tar" -C / || die "untar of the trees failed"
  tar --numeric-owner -xpf "$S0/opt.tar" -C / || die "untar of /opt/lampOS failed"
  say "RESTORED from S0"
  ;;
*) echo "usage: sh snap.sh save|digest|check|restore"; exit 2;;
esac
