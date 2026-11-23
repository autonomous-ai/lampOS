# lamp, root sh (common first). READ ONLY. Split kit's READBACK: sha256 of the files #8156/#8213 change, as lamp-update
# wrote them (one run path on: lamp-update's agent_ns::wrapper::write_engine / hermes_install::write_entry_points),
# and of lamp-update's own binary, with the build, the engine and anything of a ROW kit left. Used after round 1
# (piece A's verify: the Mac compares with the build's own files) and as the as-found record (piece A start, piece B end).
#   agent-ns     /usr/local/lib/lampos/agent-ns   (agent_ns::wrapper::NS, ns_text: #8213's one-path start edits)
#   hermes       /usr/local/bin/hermes            (hermes_install::WRAPPER; with the one run path its text is
#                                                  crates/update/src/agent_ns/wrapper/one-path-hermes.sh, test-pinned)
#   lamp-update  /opt/lampOS/update/current/lamp-update
cd /
f(){ if [ -f "$2" ] && [ ! -L "$2" ]; then echo "FILE $1 $(sha256sum "$2" | cut -d' ' -f1) $2"; else echo "FILE $1 absent $2"; fi; }
f agent-ns /usr/local/lib/lampos/agent-ns
f hermes /usr/local/bin/hermes
f lamp-update $UPD/current/lamp-update
n=$(grep -cF /run/lampos-skill-start /usr/local/lib/lampos/agent-ns 2>/dev/null); echo "AGENT_NS_START ${n:-0}"
m=$(head -c 512 /usr/local/lib/lampos/agent-ns 2>/dev/null | grep -cF 'Written by lamp-update.'); echo "AGENT_NS_MARK ${m:-0}"
c=$(grep -cE '^[[:space:]]*(export[[:space:]]+)?HERMES_NONINTERACTIVE=1$' /usr/local/bin/hermes 2>/dev/null); echo "HERMES_NONINTERACTIVE ${c:-0}"
echo "BUILD $(build)"
ES=/var/lib/lampos-tasks/store
echo "ENGINE $(python3 -I -c 'import json,sys
try: print(json.load(open(sys.argv[1])).get("engine") or "none")
except FileNotFoundError: print("absent")
except Exception: print("unreadable")' $ES/engine-default.json)"
# With no app choice the engine is lamp-tasks' own default for its target (hermes on a stable lamp, 23 Nov ee17),
# never guessed: read the last line lamp-tasks wrote about it.
echo "ENGINE_SAID $(journalctl -u lamp-tasks --no-pager -o cat -n 4000 2>/dev/null | sed -nE 's/.*(default engine chosen in the app|on its default engine), ([a-z]+).*/\2/p' | tail -1)"
echo "ENGINE_FILE $([ -f $ES/engine-default.json ] && sha256sum $ES/engine-default.json | cut -c1-16 || echo absent)"
echo "ENGINE_B64 $([ -f $ES/engine-default.json ] && base64 -w0 < $ES/engine-default.json || echo absent)"
echo "KEPT_ENGINE $(python3 -I -c 'import json,sys
try: print(json.load(open(sys.argv[1])).get("engine"))
except Exception: print("unreadable")' $ES/one-session.json)"
# #8302 (lamp-voice e49f607f5): a lamp past G's gate may have /etc/.../lamp-update.service.d/zz-cdn-feed.conf. Every
# lamp-update drop-in systemd loads, with the sha256 of each one outside /run, and the ExecStart argv it loads (only the
# argv: pid and times change with every start).
k=""; for p in $(systemctl show -p DropInPaths --value lamp-update.service 2>/dev/null); do case $p in /run/*) ;; *) k="$k $p=$(sha256sum $p 2>/dev/null | cut -d' ' -f1)";; esac; done
echo "UPD_DROPINS_ALL $(systemctl show -p DropInPaths --value lamp-update.service 2>/dev/null)"
echo "UPD_DROPINS_KEEP${k:- none}"
echo "UPD_ARGV $(systemctl show -p ExecStart --value lamp-update.service | sed -n 's/.*argv\[\]=\([^;]*\);.*/\1/p' | head -1)"
echo "DROPINS $(dropins | tr '\n' ';') lamp-voice: $(systemctl show -p DropInPaths --value lamp-voice.service 2>/dev/null)"
k=""; for p in /tmp/sd8156 /tmp/sd8213 /tmp/t8156 $CH /run/systemd/system/lamp-voice.service.d/zz-sd-8213-first.conf /run/systemd/system/lamp-voice.service.d/zz-sd-split-near.conf; do [ -e $p ] && k="$k $p"; done
echo "KITLEFT${k:- none}"
echo "READBACK DONE"
