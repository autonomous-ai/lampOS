#!/usr/bin/env bash
# Host-only derivation for the local frame labeller. Raw capture is never edited.
set -euo pipefail
umask 077
usage() {
  cat <<'EOF'
Usage: prepare-labels.sh --capture /absolute/capture.nut --clip-id ID --out /fresh/folder
Requires host Node.js >=20, FFmpeg and ffprobe. Extracts one lossless PNG per
native frame, PCM WAV with the retained original audio start PTS, and a hashed
manifest. No browser/server/network is opened. Raw input is never changed.
Existing output is refused. Failed derivations are retained with their logs.
EOF
}
capture= out= clip_id=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --help|-h) usage; exit 0;;
    --capture|--out|--clip-id)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      case "$1" in --capture) capture=$2;; --out) out=$2;; --clip-id) clip_id=$2;; esac
      shift 2;;
    *) printf 'Unknown argument: %s\n' "$1" >&2; exit 2;;
  esac
done
if [ -z "$capture$out$clip_id" ]; then usage; exit 0; fi
[[ "$capture" == /* && "$out" == /* ]] || { printf 'Absolute local paths required\n' >&2; exit 2; }
[[ "$clip_id" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$ ]] || { printf 'Unsafe clip id\n' >&2; exit 2; }
for bin in node ffmpeg ffprobe; do command -v "$bin" >/dev/null || { printf 'Missing %s\n' "$bin" >&2; exit 2; }; done
node --input-type=module - "$capture" "$out" "$clip_id" "$0" <<'NODE'
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';

const [capture, out, clipId, script] = process.argv.slice(2);
process.umask(0o077);
const started = new Date().toISOString();
const sha = p => {
  const hash = crypto.createHash('sha256'), fd = fs.openSync(p, 'r');
  const buffer = Buffer.alloc(4 * 1024 * 1024);
  try { for (let n; (n = fs.readSync(fd, buffer)) > 0;) hash.update(buffer.subarray(0, n)); }
  finally { fs.closeSync(fd); }
  return hash.digest('hex');
};
const put = (name, content) => fs.writeFileSync(path.join(out, name), content, {flag: 'wx', mode: 0o600});
const json = (name, value) => put(name, JSON.stringify(value, null, 2) + '\n');
const requireThat = (ok, reason) => { if (!ok) throw new Error(reason); };
requireThat(fs.statSync(capture).isFile(), 'Capture must be a regular local file');
requireThat(!fs.lstatSync(capture).isSymbolicLink(), 'Capture symlinks are refused');
requireThat(path.resolve(capture) !== path.resolve(out), 'Output cannot replace capture');
fs.mkdirSync(out, {mode: 0o700});
fs.copyFileSync(script, path.join(out, 'source-prepare-labels.sh'), fs.constants.COPYFILE_EXCL);
const commands = [];
const run = (name, bin, args) => {
  const result = spawnSync(bin, args, {encoding: 'utf8', timeout: 120000, maxBuffer: 32 * 1024 * 1024});
  put(name + '.stdout', result.stdout ?? '');
  put(name + '.stderr', result.stderr ?? '');
  commands.push({name, command: [bin, ...args], exit_code: result.status, signal: result.signal,
                 error: result.error?.message ?? null});
  requireThat(result.status === 0 && !result.error, `${name} failed; retained logs identify the cause`);
  return result.stdout;
};
const allFiles = dir => fs.readdirSync(dir, {withFileTypes: true}).flatMap(e =>
  e.isDirectory() ? allFiles(path.join(dir, e.name)) : [path.join(dir, e.name)]).sort();
let status = 'failed', failure = null, captureHash;
try {
  captureHash = sha(capture);
  json('request.json', {schema_version: 1, capture_path: capture, capture_sha256: captureHash, clip_id: clipId});
  const probe = JSON.parse(run('probe', 'ffprobe', ['-v', 'error', '-show_streams', '-show_frames', '-of', 'json', capture]));
  const video = probe.streams.filter(s => s.codec_type === 'video');
  const audio = probe.streams.filter(s => s.codec_type === 'audio');
  requireThat(video.length === 1 && audio.length === 1, 'Exactly one video and one audio stream required');
  requireThat(video[0].codec_name === 'ffv1' && audio[0].codec_name === 'pcm_s16le', 'Expected unmodified FFV1/PCM16 pilot capture');
  const toUs = (pts, timeBase) => {
    requireThat(typeof pts !== 'number' || Number.isSafeInteger(pts), 'Source PTS outside exact JSON integer range');
    requireThat(/^-?\d+$/.test(String(pts)), 'Missing integer source PTS');
    const [num, den] = timeBase.split('/').map(BigInt);
    const value = BigInt(pts) * num * 1000000n;
    const rounded = value >= 0n ? (value + den / 2n) / den : -((-value + den / 2n) / den);
    const result = Number(rounded);
    requireThat(Number.isSafeInteger(result), 'Timestamp outside exact JSON integer range');
    return result;
  };
  const vf = probe.frames.filter(f => f.media_type === 'video');
  const af = probe.frames.filter(f => f.media_type === 'audio');
  requireThat(vf.length > 1 && vf.length <= 10000 && af.length > 1, 'Empty or unbounded capture');
  const times = vf.map(f => toUs(f.pts, video[0].time_base));
  requireThat(times.every((t, i) => i === 0 || t > times[i - 1]), 'Video timestamps must be strictly increasing; keep this failed take');
  requireThat(times.at(-1) - times[0] <= 35000000, 'Capture exceeds the approved 30-second source limit');
  const sampleRate = Number(audio[0].sample_rate);
  requireThat(Number.isInteger(sampleRate) && sampleRate > 0, 'Missing audio sample rate');
  const audioStart = toUs(af[0].pts, audio[0].time_base);
  let samples = 0, maxClockError = 0;
  const audioSegments = af.map(f => {
    requireThat(Number.isSafeInteger(f.nb_samples) && f.nb_samples > 0, 'Missing audio sample count');
    const timestamp = toUs(f.pts, audio[0].time_base);
    const error = timestamp - (audioStart + Math.round(samples * 1000000 / sampleRate));
    maxClockError = Math.max(maxClockError, Math.abs(error));
    const segment = {timestamp_us: timestamp, sample_start: samples, sample_count: f.nb_samples, clock_error_us: error};
    samples += f.nb_samples;
    return segment;
  });
  json('audio-timeline.json', {sample_rate: sampleRate, audio_start_us: audioStart, samples,
    max_pcm_clock_error_us: maxClockError, segments: audioSegments});
  requireThat(maxClockError <= 2000, 'Audio PTS differs from continuous PCM by >2 ms; do not silently flatten this take');
  fs.mkdirSync(path.join(out, 'frames'), {mode: 0o700});
  run('images', 'ffmpeg', ['-hide_banner', '-nostdin', '-n', '-copyts', '-i', capture, '-map', '0:v:0',
    '-an', '-c:v', 'png', '-threads:v', '1', '-fps_mode:v', 'passthrough', '-start_number', '0',
    path.join(out, 'frames/%06d.png')]);
  run('audio', 'ffmpeg', ['-hide_banner', '-nostdin', '-n', '-i', capture, '-map', '0:a:0', '-vn',
    '-c:a', 'copy', '-f', 'wav', path.join(out, 'audio.wav')]);
  const images = fs.readdirSync(path.join(out, 'frames')).sort();
  requireThat(images.length === vf.length, 'Exported image count does not match native frames');
  const frames = vf.map((frame, index) => {
    const file = `frames/${String(index).padStart(6, '0')}.png`;
    requireThat(images[index] === path.basename(file), 'Unexpected derived frame ordering');
    return {index, timestamp_us: times[index], source_pts: frame.pts, source_time_base: video[0].time_base,
      image_file: file, sha256: sha(path.join(out, file)), width_px: frame.width, height_px: frame.height};
  });
  requireThat(sha(capture) === captureHash, 'Capture changed during derivation');
  json('manifest.json', {schema_version: 1, clip_id: clipId, capture_sha256: captureHash,
    frames, video_start_us: times[0], audio_file: 'audio.wav', audio_sha256: sha(path.join(out, 'audio.wav')),
    audio_start_us: audioStart, audio_duration_us: Math.round(samples * 1000000 / sampleRate),
    audio_sample_rate: sampleRate, audio_channels: audio[0].channels, audio_max_pcm_clock_error_us: maxClockError,
    audio_timeline_file: 'audio-timeline.json', audio_video_start_offset_us: audioStart - times[0],
    physical_sync_validated: false, ready_us: null,
    timing_note: 'All timestamps share the retained capture PTS origin. WAV time zero maps to audio_start_us. PCM clock deviation is measured, not physical acoustic calibration.'});
  status = 'complete';
} catch (error) { failure = error.message; process.exitCode = 1; }
finally {
  json('commands.json', commands);
  json('result.json', {schema_version: 1, state: status, error: failure, started_utc: started,
    completed_utc: new Date().toISOString(), capture_sha256: captureHash ?? null});
  put('README.txt', 'Private local-only frame-labelling derivative. Preserve all files, including failed outputs. Original capture is unchanged. manifest.json binds every PNG and WAV to SHA256 and actual source PTS. WAV is sample-preserving PCM with audio_start_us; discontinuity over 2 ms is refused and retained. Full audio packet timing is in audio-timeline.json. No frame-rate resampling or model output is used. Physical sync remains unverified. SHA256SUMS covers every file except itself.\n');
  const entries = allFiles(out).map(p => `${sha(p)}  ${path.relative(out, p)}`);
  put('SHA256SUMS', entries.join('\n') + '\n');
  console.log(JSON.stringify({state: status, out, capture_sha256: captureHash, error: failure}));
}
NODE
