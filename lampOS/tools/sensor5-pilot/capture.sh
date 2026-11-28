#!/usr/bin/env bash
# Explicit, local-only pilot capture. Never installs, stops services, or uploads.
set -u
set -o pipefail
umask 077

usage() {
  cat <<'EOF'
Usage: capture.sh --out FRESH_DIRECTORY --clip-id ID --participant P01..P04
  --class A|B|playback --distance 0.6|1.0|1.5 --lighting daylight|lamplight
  [--seconds 10|30] [--dry-run | --synthetic | --live]

Default is dry-run: prints the request and command, opens no device, writes no data.
Live additionally requires all of:
  --video /dev/videoN --video-size WIDTHxHEIGHT --video-format mjpeg|yuyv422
  --video-rate FPS --audio hw:CARD,DEVICE --audio-rate HZ --audio-channels N
  --consent NONEMPTY_LOCAL_FILE --owner-slot REFERENCE
  --privacy-open --local-storage-confirmed
Live currently refuses: a verified microphone-and-camera permit supervisor must
be connected before capture can be enabled. Operator acknowledgements are not
runtime permission. Synthetic capture and host preparation work without it.
Synthetic mode uses generated colour bars/tone, never a camera/microphone.
  --synthetic-audio-offset-ms 0..500  (fixture only; default 125)

Requires Bash, FFmpeg >=5.1, ffprobe, jq, SHA256 utility; live also Linux/fuser.
The operator must reserve a free lamp, confirm consent/privacy and encrypted local
storage, and stop on loss of consent or privacy (Ctrl-C). No automatic retakes.
Each attempt uses a new directory, including failures. Keep every attempt.
EOF
}
die() { printf 'capture: %s\n' "$*" >&2; exit 2; }
need_value() { [ "$#" -ge 2 ] && [ -n "$2" ] || die "Missing value for $1"; }
is_uint() { [[ "$1" =~ ^[0-9]+$ ]]; }
sha() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  else shasum -a 256 "$1" | awk '{print $1}'; fi
}
mode=dry-run
mode_set=0
out= clip_id= participant= class= distance= lighting=
seconds=10
video= audio= video_size= video_format= video_rate= audio_rate= audio_channels=
consent= owner_slot= privacy_open=false local_storage=false
synthetic_offset=125
while [ "$#" -gt 0 ]; do
  case "$1" in
    --help|-h) usage; exit 0 ;;
    --dry-run|--synthetic|--live)
      [ "$mode_set" -eq 0 ] || die 'Choose exactly one mode'
      mode=${1#--}; mode_set=1; shift ;;
    --privacy-open) privacy_open=true; shift ;;
    --local-storage-confirmed) local_storage=true; shift ;;
    --out|--clip-id|--participant|--class|--distance|--lighting|--seconds|--video|--audio|--video-size|--video-format|--video-rate|--audio-rate|--audio-channels|--consent|--owner-slot|--synthetic-audio-offset-ms)
      need_value "$@"
      case "$1" in
        --out) out=$2;; --clip-id) clip_id=$2;; --participant) participant=$2;;
        --class) class=$2;; --distance) distance=$2;; --lighting) lighting=$2;;
        --seconds) seconds=$2;; --video) video=$2;; --audio) audio=$2;;
        --video-size) video_size=$2;; --video-format) video_format=$2;;
        --video-rate) video_rate=$2;; --audio-rate) audio_rate=$2;;
        --audio-channels) audio_channels=$2;; --consent) consent=$2;;
        --owner-slot) owner_slot=$2;; --synthetic-audio-offset-ms) synthetic_offset=$2;;
      esac
      shift 2 ;;
    *) die "Unknown argument: $1" ;;
  esac
done
if [ -z "$out$clip_id$participant$class$distance$lighting" ]; then usage; exit 0; fi
[[ "$out" == /* && "$out" != *$'\n'* && "$out" != *$'\r'* ]] || die '--out must be an absolute local path'
[[ "$clip_id" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$ ]] || die 'Unsafe or missing clip id'
[[ "$participant" =~ ^P0[1-4]$ ]] || die 'Participant must be P01..P04'
[[ "$class" == A || "$class" == B || "$class" == playback ]] || die 'Class must be A, B or playback'
[[ "$distance" == 0.6 || "$distance" == 1.0 || "$distance" == 1.5 ]] || die 'Distance must match the frozen schedule'
[[ "$lighting" == daylight || "$lighting" == lamplight ]] || die 'Lighting must match the frozen schedule'
[[ "$seconds" == 10 || "$seconds" == 30 ]] || die 'Duration must be 10 or 30 seconds'
[[ "$class" == playback && "$seconds" == 30 || "$class" != playback && "$seconds" == 10 ]] || die 'Use 30 seconds for playback source, 10 seconds for A/B'
is_uint "$synthetic_offset" && [ "$synthetic_offset" -le 500 ] || die 'Synthetic offset must be 0..500 ms'
for bin in jq ffmpeg ffprobe; do command -v "$bin" >/dev/null 2>&1 || die "Missing dependency: $bin"; done
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 || die 'Missing SHA256 utility'
[ ! -e "$out" ] && [ ! -L "$out" ] || die 'Output already exists; preserve it and use a fresh take directory'

if [ "$mode" != synthetic ]; then
  [[ "$video" =~ ^/dev/video[0-9]+$ ]] || die 'Explicit /dev/videoN is required for a live request/dry-run'
  [[ "$audio" =~ ^hw:([0-9]+),([0-9]+)$ ]] || die 'Explicit hardware audio hw:CARD,DEVICE is required'
  audio_node="/dev/snd/pcmC${BASH_REMATCH[1]}D${BASH_REMATCH[2]}c"
  [[ "$video_size" =~ ^([0-9]+)x([0-9]+)$ ]] || die 'Explicit video size required'
  width=${BASH_REMATCH[1]}; height=${BASH_REMATCH[2]}
  [ "$width" -ge 16 ] && [ "$width" -le 1920 ] && [ "$height" -ge 16 ] && [ "$height" -le 1080 ] || die 'Video size outside 16..1920 x 16..1080 bounds'
  [[ "$video_format" == mjpeg || "$video_format" == yuyv422 ]] || die 'Video format must be mjpeg or yuyv422'
  is_uint "$video_rate" && [ "$video_rate" -ge 1 ] && [ "$video_rate" -le 30 ] || die 'Video rate must be 1..30'
  [[ "$audio_rate" == 16000 || "$audio_rate" == 44100 || "$audio_rate" == 48000 ]] || die 'Audio rate must be 16000, 44100 or 48000'
  is_uint "$audio_channels" && [ "$audio_channels" -ge 1 ] && [ "$audio_channels" -le 8 ] || die 'Audio channels must be 1..8'
fi
if [ "$mode" == live ]; then
  [ -n "$owner_slot" ] && [ "${#owner_slot}" -le 200 ] && [[ "$owner_slot" != *$'\n'* && "$owner_slot" != *$'\r'* ]] || die 'An owner-approved slot reference is required'
  [ -f "$consent" ] && [ -s "$consent" ] || die 'A nonempty local consent record is required'
  grep -Fxq "Participant alias: $participant" "$consent" || die 'Consent record does not match this participant alias'
  grep -Fxq 'Consent accepted: yes' "$consent" || die 'Consent must be explicitly accepted in the local signed record'
  [ "$privacy_open" == true ] || die 'Confirm the physical privacy controls are open'
  [ "$local_storage" == true ] || die 'Confirm encrypted local storage outside cloud sync'
  # A physical switch observation is not a microphone/camera grant. Do not
  # remove this hold until a verified supervisor owns the actual Voice/Camera
  # lease checks and the lifetime of the FFmpeg child. The live branch below
  # remains reviewable without silently making direct device access available.
  die 'Live capture is held: verified microphone and camera permit supervisor is not connected'
fi

command=(ffmpeg -hide_banner -nostdin -n -copyts -threads 2)
if [ "$mode" == synthetic ]; then
  offset=$(awk -v ms="$synthetic_offset" 'BEGIN {printf "%.3f",ms/1000}')
  command+=(-f lavfi -i "testsrc2=size=320x240:rate=25:duration=$seconds"
    -itsoffset "$offset" -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=$seconds")
else
  # mono2abs maps the V4L2 monotonic clock to wall time. ALSA supplies wall-time
  # PTS. This mapping is retained, not a claim of physical acoustic sync.
  command+=(-thread_queue_size 64 -f v4l2 -timestamps mono2abs -input_format "$video_format"
    -video_size "$video_size" -framerate "$video_rate" -t "$seconds" -i "$video"
    -thread_queue_size 64 -f alsa -sample_rate "$audio_rate" -channels "$audio_channels" -t "$seconds" -i "$audio")
fi
command+=(-map 0:v:0 -map 1:a:0 -c:v ffv1 -level 3 -threads:v 2 -enc_time_base:v -1
  -fps_mode:v passthrough -c:a pcm_s16le -avoid_negative_ts disabled -progress "$out/progress.txt" -nostats -f nut "$out/capture.nut")
command_json=$(jq -cn --args '$ARGS.positional' -- "${command[@]}") || die 'Could not encode command'
request=$(jq -cn --arg mode "$mode" --arg clip "$clip_id" --arg participant "$participant" --arg class "$class" \
  --arg distance "$distance" --arg lighting "$lighting" --argjson seconds "$seconds" \
  --arg out "$out" --arg slot "$owner_slot" --arg consent "$consent" \
  --argjson privacy "$privacy_open" --argjson local "$local_storage" --argjson command "$command_json" \
  '{schema_version:1,mode:$mode,clip_id:$clip,participant:$participant,planned_class:$class,distance_m:($distance|tonumber),lighting:$lighting,planned_seconds:$seconds,out:$out,owner_slot:$slot,consent_path:$consent,privacy_open:$privacy,local_storage_confirmed:$local,command:$command,ready_us:null,physical_sync_validated:false}') || die 'Could not encode request'
if [ "$mode" == dry-run ]; then printf '%s\n' "$request"; exit 0; fi

mkdir -m 700 "$out" || die 'Could not create a fresh output directory (parent must exist)'
printf '%s\n' "$request" > "$out/request.json"
cp "$0" "$out/source-capture.sh" || die 'Could not retain capture source'
if [ "$mode" == live ]; then cp "$consent" "$out/consent-record.local" || die 'Could not retain consent record'; fi
started=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
reason=initialization_failed
state=failed
capture_pid=
watchdog_pid=
signal_seen=
ffmpeg_exit=null
finalize() {
  code=$?
  trap - EXIT INT TERM HUP
  if [ -n "$capture_pid" ] && kill -0 "$capture_pid" 2>/dev/null; then kill -TERM "$capture_pid" 2>/dev/null || :; wait "$capture_pid" 2>/dev/null || :; fi
  if [ -n "$watchdog_pid" ]; then kill -TERM "$watchdog_pid" 2>/dev/null || :; wait "$watchdog_pid" 2>/dev/null || :; fi
  completed=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
  jq -cn --arg state "$state" --arg reason "$reason" --arg started "$started" --arg completed "$completed" \
    --arg signal "$signal_seen" --argjson code "$code" --argjson ffmpeg_exit "$ffmpeg_exit" \
    '{schema_version:1,state:$state,reason:$reason,started_utc:$started,completed_utc:$completed,exit_code:$code,ffmpeg_exit_code:$ffmpeg_exit,signal:$signal,ready_us:null,physical_sync_validated:false}' > "$out/result.json"
  printf '%s\n' 'Private sensor 5 capture attempt. Preserve every file, including failures. No upload. Original full decoded frames and configured microphone channels are retained losslessly as FFV1/PCM16 in NUT, with passthrough timestamps. Consult request.json, result.json, probe.json and frames.json. Physical acoustic synchronization and sensor readiness are unverified. SHA256SUMS covers every file except itself. No service or device configuration was changed.' > "$out/README.txt"
  (cd "$out" && for file in *; do [ -f "$file" ] || continue; [ "$file" != SHA256SUMS ] || continue; printf '%s  %s\n' "$(sha "$file")" "$file"; done) > "$out/SHA256SUMS"
  printf 'Capture %s: %s (%s)\n' "$state" "$out" "$reason"
  exit "$code"
}
on_signal() {
  signal_seen=$1
  reason=operator_interrupted
  if [ -n "$capture_pid" ]; then
    kill -TERM "$capture_pid" 2>/dev/null || :
    if [ -n "$watchdog_pid" ]; then kill -TERM "$watchdog_pid" 2>/dev/null || :; wait "$watchdog_pid" 2>/dev/null || :; fi
    # Stop gets its own short bound, independent of the remaining take length.
    (
      sleeper=
      trap '[ -z "$sleeper" ] || kill "$sleeper" 2>/dev/null; exit 0' TERM INT HUP
      sleep 3 & sleeper=$!; wait "$sleeper" || exit 0
      kill -KILL "$capture_pid" 2>/dev/null || :
    ) &
    watchdog_pid=$!
  fi
}
trap finalize EXIT
trap 'on_signal INT' INT
trap 'on_signal TERM' TERM
trap 'on_signal HUP' HUP
ffmpeg -version > "$out/ffmpeg-version.txt" 2>&1 || die 'Could not read FFmpeg version'
ffprobe -version > "$out/ffprobe-version.txt" 2>&1 || die 'Could not read ffprobe version'
if [ "$mode" == live ]; then
  [ "$(uname -s)" == Linux ] || die 'Live mode requires Linux; no device was opened'
  command -v fuser >/dev/null 2>&1 || die 'fuser is required for the busy-device check'
  fuser "$video" "$audio_node" > "$out/device-users.txt" 2> "$out/device-users.stderr"
  busy_code=$?
  if [ "$busy_code" -eq 0 ]; then reason=device_busy; die 'Camera or microphone is busy; nothing is stopped automatically'; fi
  [ "$busy_code" -eq 1 ] && [ ! -s "$out/device-users.stderr" ] || die 'Could not establish that both devices are free'
  [ -c "$video" ] && [ -r "$video" ] || die 'Camera is not a readable character device'
  [ -c "$audio_node" ] && [ -r "$audio_node" ] || die 'Microphone is not a readable character device'
fi
if [ -n "$signal_seen" ]; then reason=operator_interrupted; exit 1; fi
reason=capture_failed
"${command[@]}" > "$out/ffmpeg.stdout" 2> "$out/ffmpeg.stderr" &
capture_pid=$!
# One bounded watchdog, not a retry loop. Its own sleeping child is reaped.
(
  sleeper=
  trap '[ -z "$sleeper" ] || kill "$sleeper" 2>/dev/null; exit 0' TERM INT HUP
  sleep "$((seconds + 15))" & sleeper=$!; wait "$sleeper" || exit 0
  printf '%s\n' 'Capture exceeded duration + 15 seconds; terminated.' > "$out/deadline.txt"
  kill -TERM "$capture_pid" 2>/dev/null || exit 0
  sleep 3 & sleeper=$!; wait "$sleeper" || exit 0
  kill -KILL "$capture_pid" 2>/dev/null || :
) &
watchdog_pid=$!
wait "$capture_pid"
ffmpeg_exit=$?
if kill -0 "$capture_pid" 2>/dev/null; then wait "$capture_pid"; ffmpeg_exit=$?; fi
capture_pid=
kill -TERM "$watchdog_pid" 2>/dev/null || :
wait "$watchdog_pid" 2>/dev/null || :
watchdog_pid=
if [ -n "$signal_seen" ]; then reason=operator_interrupted; exit 1; fi
if [ -e "$out/deadline.txt" ]; then reason=deadline_exceeded; exit 1; fi
[ "$ffmpeg_exit" -eq 0 ] || exit 1
reason=invalid_capture
ffprobe -v error -show_streams -show_format -of json "$out/capture.nut" > "$out/probe.json" 2> "$out/probe.stderr" || exit 1
ffprobe -v error -show_frames -of json "$out/capture.nut" > "$out/frames.json" 2> "$out/frames.stderr" || exit 1
jq -e --argjson seconds "$seconds" '
  [.frames[] | select(.media_type=="video") | (.pts_time|tonumber)] as $v |
  [.frames[] | select(.media_type=="audio") | (.pts_time|tonumber)] as $a |
  ($v|length)>1 and ($a|length)>1 and ($v[-1]-$v[0])>=($seconds-0.6) and
  ($a[-1]-$a[0])>=($seconds-0.6) and ($v[-1]-$v[0])<=($seconds+1) and
  ($a[-1]-$a[0])<=($seconds+1)
' "$out/frames.json" > "$out/validation.txt" || exit 1
state=complete
reason=bounded_capture_finished
exit 0
