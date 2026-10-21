# Sourced by the played kit's scripts, from the kit's folder, before anything is played or written: refuses when
# LAMP_HOST is not set, and makes the folders results and the spoken lines go to. See README.md.
if [ -z "${LAMP_HOST:-}" ]; then
  echo "played kit: LAMP_HOST is not set. Set it to your own lamp's address, and LAMP_SSH_KEY / LAMP_USER if needed (see lampOS/tools/played/README.md). Nothing was played." >&2
  exit 2
fi
PLAYED_OUT=${PLAYED_OUT:-$HOME/lamp-played}
export PLAYED_OUT
mkdir -p "$PLAYED_OUT/scenarios" "$PLAYED_OUT/prompts/generated" || exit 9
