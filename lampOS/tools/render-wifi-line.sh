#!/bin/sh
# The Wi-Fi helper's no-Wi-Fi line, or the recovery hotspot's line (#3743), rendered once in the lamp's own voice (#1959, lampOS/docs/setup.md,
# "The no-Wi-Fi line is a recording"): the voice's own route, model and voice, the line as one written
# turn, kept as 24 kHz mono WAV, and the failure-notice manifest that names it with its SHA-256
# (lampOS/docs/failure-notices.md). The folder it writes goes into the notices release, which install places in /opt/lampOS/voice/notices (#1844).
#
#   tools/render-wifi-line.sh /absolute/private/provider.json NEW_FOLDER [production|test] [lost|recovering]
#
# PROVIDER is the voice's provider file (deploy/product/settings/voice-provider.json's fields) with its
# credential file, both mode 0600. Nothing here reads, prints or copies the credential: only the
# lamp-gemini example opens it, as the voice does. NEW_FOLDER must not exist. The approval is `test`
# unless `production` is given: a person listens to the WAV and compares the transcript printed with
# the words before a production manifest goes into the image.
set -eu

[ $# -ge 2 ] && [ $# -le 4 ] || { echo "usage: $0 PROVIDER.json NEW_FOLDER [production|test] [lost|recovering]" >&2; exit 2; }
provider=$1
folder=$2
approval=${3:-test}
case $approval in production|test) ;; *) echo "approval is production or test" >&2; exit 2 ;; esac
# `lost` is the Wi-Fi helper's line, wifi_change::SAY_LOST (crates/setup/src/wifi_change.rs, #3743);
# `recovering` is the recovery hotspot's, recover::SAY_OPENED (crates/setup/src/recover.rs, #3743 with
# #3020). Change each together with its constant.
case ${4:-lost} in
lost)
    KIND=wifi_lost
    LINE="I can't reach any Wi-Fi. If you changed your Wi-Fi's name or password, push a paperclip into the hole marked reset on the back of my base and hold it for five seconds. That erases me and starts setup again."
    ;;
recovering)
    KIND=wifi_recovering
    LINE="I can't reach my Wi-Fi, so I've opened setup. Set my Wi-Fi again in the app."
    ;;
*) echo "the line is lost or recovering" >&2; exit 2 ;;
esac
[ -e "$folder" ] && { echo "$folder is there already; nothing is overwritten" >&2; exit 2; }
here=$(cd "$(dirname "$0")/.." && pwd)

# The example renders the WAV and writes the manifest beside it, with the provider file's voice and
# model, the approval and the WAV's SHA-256; it makes the folder.
case $folder in /*) ;; *) folder=$(pwd)/$folder ;; esac
(cd "$here" && cargo run -q --locked -p lamp-gemini --example render_line -- \
    "$provider" "$folder" "$KIND" "$LINE" "$approval")

# The loader's own check, as the voice will read the folder.
accept=
[ "$approval" = test ] && accept=--accept-test
(cd "$here" && cargo run -q --locked -p lamp-audio -- notices "$folder" $accept)
