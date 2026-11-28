#!/usr/bin/env bash
# Offline, local-only preparation of a completed native sensor-5 archive.
set -euo pipefail
umask 077
usage() {
  cat <<'EOF'
Usage: prepare-recorder.sh --archive /absolute/native/clip --clip-id ID --out /fresh/folder
Requires host Node.js >=20 and FFmpeg. Never opens devices or a network service.
Verifies the completed archive and every inventory hash, then retains all camera
frames and makes one labelling folder per continuous PCM segment. Preview timing
is an UNVERIFIED receipt/dequeue estimate, never scoring evidence. Separate local
operator/Reviewer alignment checks are required before scoring. Existing output
is refused. Original files and failed derivations are never overwritten/deleted.
EOF
}
archive= out= clip_id=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --help|-h) usage; exit 0;;
    --archive|--out|--clip-id)
      [ "$#" -ge 2 ] || { usage >&2; exit 2; }
      case "$1" in --archive) archive=$2;; --out) out=$2;; --clip-id) clip_id=$2;; esac
      shift 2;;
    *) printf 'Unknown argument: %s\n' "$1" >&2; exit 2;;
  esac
done
if [ -z "$archive$out$clip_id" ]; then usage; exit 0; fi
[[ "$archive" == /* && "$out" == /* ]] || { printf 'Absolute local paths required\n' >&2; exit 2; }
# Leave room for the segment suffix in the labeller's 64-character identifier.
[[ "$clip_id" =~ ^[A-Za-z0-9][A-Za-z0-9_-]{0,54}$ ]] || { printf 'Unsafe or long clip id\n' >&2; exit 2; }
for bin in node ffmpeg; do command -v "$bin" >/dev/null || { printf 'Missing %s\n' "$bin" >&2; exit 2; }; done
node --input-type=module - "$archive" "$out" "$clip_id" "$0" <<'NODE'
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';

const [archiveArg, outArg, clipId, script] = process.argv.slice(2);
process.umask(0o077);
const requireThat = (ok, message) => { if (!ok) throw new Error(message); };
const isUint = n => Number.isSafeInteger(n) && n >= 0;
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const sha = file => {
  const hash = crypto.createHash('sha256'), fd = fs.openSync(file, 'r');
  const buffer = Buffer.alloc(4 * 1024 * 1024);
  try { for (let n; (n = fs.readSync(fd, buffer)) > 0;) hash.update(buffer.subarray(0, n)); }
  finally { fs.closeSync(fd); }
  return hash.digest('hex');
};
const safeRelative = s => typeof s === 'string' && s.length > 0 && s.length < 512 &&
  !s.startsWith('/') && !/[\\\x00-\x1f\x7f]/.test(s) && s.split('/').every(x => x && x !== '.' && x !== '..');
const walk = dir => {
  const files = [];
  const visit = p => {
    for (const entry of fs.readdirSync(p, {withFileTypes: true})) {
      const file = path.join(p, entry.name);
      requireThat(!entry.isSymbolicLink(), `Symlink refused: ${file}`);
      if (entry.isDirectory()) visit(file);
      else {
        requireThat(entry.isFile(), `Non-regular file refused: ${file}`);
        files.push(path.relative(dir, file));
        requireThat(files.length <= 40000, 'Unbounded file count');
      }
    }
  };
  visit(dir);
  return files.sort();
};
requireThat(Number(process.versions.node.split('.')[0]) >= 20, 'Node.js >=20 required');
requireThat(!/[\x00-\x1f\x7f]/.test(archiveArg + outArg), 'Control characters in paths refused');
requireThat(!fs.lstatSync(archiveArg).isSymbolicLink() && fs.statSync(archiveArg).isDirectory(), 'Archive must be a regular directory');
const archive = fs.realpathSync(archiveArg);
const out = path.join(fs.realpathSync(path.dirname(outArg)), path.basename(outArg));
requireThat(out !== archive && !out.startsWith(archive + path.sep), 'Output must be outside the original archive');
fs.mkdirSync(out, {mode: 0o700});
const put = (name, bytes) => fs.writeFileSync(path.join(out, name), bytes, {flag: 'wx', mode: 0o600});
const json = (name, value) => put(name, JSON.stringify(value, null, 2) + '\n');
const mkdir = name => fs.mkdirSync(path.join(out, name), {mode: 0o700});
const started = new Date().toISOString(), commands = [];
let state = 'failed', failure = null, captureHash = null, frameCount = 0, segmentCount = 0, unknownCount = 0;
const run = (name, args) => {
  const p = spawnSync('ffmpeg', args, {encoding: 'utf8', timeout: 120000, killSignal: 'SIGKILL', maxBuffer: 8 * 1024 * 1024});
  put(`${name}.stdout`, p.stdout ?? ''); put(`${name}.stderr`, p.stderr ?? '');
  commands.push({name, command: ['ffmpeg', ...args], exit_code: p.status, signal: p.signal, error: p.error?.message ?? null});
  requireThat(p.status === 0 && !p.error, `${name} failed; retained logs identify the cause`);
};
try {
  fs.copyFileSync(script, path.join(out, 'source-prepare-recorder.sh'), fs.constants.COPYFILE_EXCL);
  fs.chmodSync(path.join(out, 'source-prepare-recorder.sh'), 0o600);
  json('request.json', {schema_version: 1, archive_path: archive, clip_id: clipId, preview_only: true});
  const files = walk(archive);
  requireThat(files.includes('inventory.jsonl'), 'Missing archive inventory');
  requireThat(fs.statSync(path.join(archive, 'inventory.jsonl')).size <= 8 * 1024 * 1024, 'Unbounded inventory');
  const inventoryBytes = fs.readFileSync(path.join(archive, 'inventory.jsonl'));
  captureHash = digest(inventoryBytes);
  put('source-inventory.jsonl', inventoryBytes);
  const entries = new Map();
  for (const [i, line] of inventoryBytes.toString('utf8').trimEnd().split('\n').entries()) {
    let e;
    try { e = JSON.parse(line); } catch { throw new Error(`Invalid/truncated inventory line ${i + 1}`); }
    requireThat(safeRelative(e.path) && e.path !== 'inventory.jsonl' && !entries.has(e.path), 'Unsafe or duplicate inventory path');
    requireThat(isUint(e.bytes) && e.bytes <= 16 * 1024 * 1024 && /^[0-9a-f]{64}$/.test(e.sha256), 'Invalid inventory size/hash');
    entries.set(e.path, e);
  }
  requireThat(JSON.stringify([...entries.keys()].sort()) === JSON.stringify(files.filter(p => p !== 'inventory.jsonl')), 'Inventory does not cover the exact archive file set');
  const checkInventory = () => {
    requireThat(sha(path.join(archive, 'inventory.jsonl')) === captureHash, 'Source inventory changed');
    requireThat(JSON.stringify(walk(archive)) === JSON.stringify(files), 'Source file set changed');
    for (const e of entries.values()) {
      const p = path.join(archive, e.path);
      requireThat(fs.statSync(p).size === e.bytes && sha(p) === e.sha256, `Source hash/size mismatch: ${e.path}`);
    }
  };
  checkInventory();
  const read = name => {
    const e = entries.get(name);
    requireThat(e, `Missing inventoried file: ${name}`);
    const bytes = fs.readFileSync(path.join(archive, name));
    requireThat(bytes.length === e.bytes && digest(bytes) === e.sha256, `Source changed: ${name}`);
    return bytes;
  };
  const readJSON = name => {
    requireThat(entries.get(name)?.bytes <= 1024 * 1024, `Missing/unbounded JSON: ${name}`);
    const exactNumbers = value => {
      if (typeof value === 'number') requireThat(Number.isSafeInteger(value), 'Noninteger or inexact source metadata number');
      else if (Array.isArray(value)) value.forEach(exactNumbers);
      else if (value && typeof value === 'object') Object.values(value).forEach(exactNumbers);
    };
    try { const value = JSON.parse(read(name)); exactNumbers(value); return value; }
    catch (e) { throw new Error(`Invalid/truncated ${name}: ${e.message}`); }
  };
  const session = readJSON('session.json'), terminal = readJSON('result.json');
  requireThat(entries.has('consent.json') && entries.has('consent.txt'), 'Missing consent provenance');
  requireThat(session.schema_version === 1 && typeof session.clock_id === 'string' && session.clock_id.length > 0 &&
    session.physical_sync_validated === false, 'Unsupported session clock/sync contract');
  requireThat(terminal.schema_version === 1 && terminal.complete === true && terminal.result?.reason === 'duration_elapsed', 'Incomplete archive: valid successful terminal record required');
  put('source-session.json', read('session.json')); put('source-result.json', read('result.json'));
  const sidecars = [...entries.keys()].filter(p => /^records\/\d{8}\.(?:frame\.jpg|audio\.pcm|privacy\.json)\.json$/.test(p)).sort();
  requireThat(sidecars.length > 0 && sidecars.length <= 10000, 'Empty/unbounded record set');
  const allowed = new Set(['README.txt', 'session.json', 'result.json', 'consent.json', 'consent.txt']);
  const records = sidecars.map((name, i) => {
    const record = readJSON(name), payload = name.slice(0, -5);
    requireThat(record.schema_version === 1 && record.record_index === i + 1 && record.payload_file === payload &&
      payload.startsWith(`records/${String(i + 1).padStart(8, '0')}.`) && record.sha256 === entries.get(payload)?.sha256,
      `Record identity/hash mismatch: ${name}`);
    requireThat(['frame', 'audio', 'privacy'].includes(record.kind), `Unsupported record kind: ${name}`);
    const extension = {frame: 'frame.jpg', audio: 'audio.pcm', privacy: 'privacy.json'}[record.kind];
    requireThat(payload.endsWith('.' + extension), `Record kind/path mismatch: ${name}`);
    allowed.add(name); allowed.add(payload);
    return record;
  });
  requireThat([...entries.keys()].every(name => allowed.has(name)), 'Unrecognized or orphan archive file');
  const video = records.filter(r => r.kind === 'frame'), audio = records.filter(r => r.kind === 'audio');
  requireThat(video.length > 0 && audio.length > 0 && terminal.records === records.length &&
    terminal.result.frames_admitted === video.length && terminal.result.audio_blocks_admitted === audio.length &&
    (terminal.frames === undefined || terminal.frames === video.length) &&
    (terminal.audio_blocks === undefined || terminal.audio_blocks === audio.length), 'Terminal media counts do not match actual records');
  json('source-records.json', {schema_version: 1, capture_sha256: captureHash, records});
  const byteArray16 = a => Array.isArray(a) && a.length === 16 && a.every(n => Number.isInteger(n) && n >= 0 && n <= 255);
  video.forEach((r, i) => {
    const f = r.frame, prior = video[i - 1]?.frame;
    requireThat(f && f.source === 'lamp-eyes' && JSON.stringify(f.format) === '[77,74,80,71]' &&
      isUint(f.width) && f.width > 0 && f.width <= 8192 && isUint(f.height) && f.height > 0 && f.height <= 8192 &&
      isUint(f.dequeue_started_at_us) && isUint(f.dequeue_completed_at_us) && isUint(f.copied_at_us) && isUint(r.received_us) &&
      f.dequeue_started_at_us <= f.dequeue_completed_at_us && f.dequeue_completed_at_us <= f.copied_at_us && f.copied_at_us <= r.received_us &&
      isUint(f.frame_sequence) && isUint(f.capture_epoch) && byteArray16(f.camera_worker) && byteArray16(f.grant?.boot) && isUint(f.grant.generation) &&
      isUint(r.source_frames_between), 'Invalid native camera metadata');
    requireThat(!prior || (f.dequeue_completed_at_us > prior.dequeue_completed_at_us && f.frame_sequence > prior.frame_sequence &&
      f.frame_sequence - prior.frame_sequence - 1 === r.source_frames_between && f.capture_epoch === prior.capture_epoch &&
      JSON.stringify(f.camera_worker) === JSON.stringify(prior.camera_worker) && JSON.stringify(f.grant) === JSON.stringify(prior.grant)),
      'Native camera lineage/order/gap contradiction');
    requireThat(f.derived == null && f.driver_timestamp && isUint(f.driver_timestamp.micros) &&
      ['host_monotonic', 'realtime', 'unknown'].includes(f.driver_timestamp.domain) &&
      ['start_of_exposure', 'end_of_frame', 'unknown'].includes(f.driver_timestamp.point), 'Unsupported native camera timestamp/derivation');
    const bytes = read(r.payload_file);
    requireThat(bytes.length >= 4 && bytes[0] === 255 && bytes[1] === 216 && bytes.at(-2) === 255 && bytes.at(-1) === 217, 'Malformed JPEG payload');
  });
  requireThat(video.at(-1).frame.dequeue_completed_at_us - video[0].frame.dequeue_completed_at_us <= 35000000, 'Camera timeline exceeds 35 seconds');
  const segments = [];
  audio.forEach((r, i) => {
    const h = r.header, a = r.audio, prior = audio[i - 1];
    requireThat(h && a && h.source === 'ears' && h.topic === 'mic1/audio' && h.type === 'lamp_msgs/Audio' && h.kind === 'event' &&
      h.enc === 'json+pcm' && /^[0-9a-fA-F]{16}$/.test(h.boot) && isUint(h.seq) && h.seq > 0 && isUint(h.at_us) && isUint(h.at_unix_ms) &&
      h.expires_ms === 50 && isUint(r.received_us) && r.received_us >= h.at_us && isUint(a.sequence) &&
      a.sample_rate === 16000 && a.channels === 1 && a.sample_format === 'S16LE' && entries.get(r.payload_file).bytes === 640 &&
      isUint(r.audio_sequence_gap) && isUint(r.bus_gap) && r.timestamp_boundary === 'ears_capture_read_completed' && r.adc_timestamp === false,
      'Unsupported native PCM metadata');
    requireThat(h.clock == null ? r.clock_provenance === 'existing_ears_privacy_runtime_source' :
      h.clock === session.clock_id && r.clock_provenance === 'bus_header', 'Unsupported PCM clock provenance');
    requireThat(!prior || h.at_us > prior.header.at_us, 'PCM timestamp regression/ambiguity');
    let reasons = [];
    if (r.bus_gap > 0) reasons.push('declared_bus_gap');
    if (r.audio_sequence_gap > 0) reasons.push('declared_audio_sequence_gap');
    if (prior) {
      if (h.boot !== prior.header.boot) reasons.push('publisher_boot_changed');
      if (h.clock !== prior.header.clock) reasons.push('clock_provenance_changed');
      if (h.seq !== prior.header.seq + 1) reasons.push('bus_sequence_discontinuity');
      if (a.sequence !== prior.audio.sequence + 1) reasons.push('audio_sequence_discontinuity');
      const current = segments.at(-1), expected = current.audio_start_us + (current.blocks.length + 1) * 20000;
      if (Math.abs(h.at_us - expected) > 2000) reasons.push('sample_clock_deviation_over_2ms');
    }
    if (!prior || reasons.length) segments.push({index: segments.length, audio_start_us: h.at_us - 20000,
      boundary_reasons: prior ? reasons : ['first_observed_block', ...reasons], blocks: [], frames: []});
    const segment = segments.at(-1), sampleStart = segment.blocks.length * 320;
    segment.blocks.push({sample_start: sampleStart, sample_count: 320,
      completion_minus_pcm_estimate_us: h.at_us - (segment.audio_start_us + (segment.blocks.length + 1) * 20000), source: r});
  });
  requireThat(audio.at(-1).header.at_us - audio[0].header.at_us <= 35000000, 'PCM timeline exceeds 35 seconds');
  segments.forEach(s => { s.audio_end_us = s.audio_start_us + s.blocks.length * 20000; });
  mkdir('frames');
  const mjpeg = fs.openSync(path.join(out, 'source-frames.mjpeg'), 'wx', 0o600);
  try { for (const r of video) fs.writeFileSync(mjpeg, read(r.payload_file)); } finally { fs.closeSync(mjpeg); }
  // MJPEG's demuxer clock is discarded: one decoded PNG maps to each native record.
  run('images', ['-hide_banner', '-nostdin', '-n', '-f', 'mjpeg', '-i', path.join(out, 'source-frames.mjpeg'),
    '-map', '0:v:0', '-an', '-c:v', 'png', '-threads:v', '1', '-fps_mode:v', 'passthrough', '-start_number', '0', path.join(out, 'frames/%06d.png')]);
  requireThat(fs.readdirSync(path.join(out, 'frames')).length === video.length, 'Decoded frame count differs from original records');
  const unpaired = [], allFrames = video.map((r, originalIndex) => {
    const file = `frames/${String(originalIndex).padStart(6, '0')}.png`, bytes = fs.readFileSync(path.join(out, file));
    requireThat(bytes.length >= 24 && bytes.subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10])) &&
      bytes.readUInt32BE(16) === r.frame.width && bytes.readUInt32BE(20) === r.frame.height, 'Decoded image dimensions differ from FrameMeta');
    const f = {original_index: originalIndex, source_record_index: r.record_index, timestamp_us: r.frame.dequeue_completed_at_us,
      timestamp_boundary: 'camera dequeue completion estimate', image_file: file, sha256: digest(bytes), width_px: r.frame.width,
      height_px: r.frame.height, native_frame: r.frame, source_received_us: r.received_us, source_frames_between: r.source_frames_between,
      source_payload_file: r.payload_file, source_payload_sha256: r.sha256};
    const matching = segments.filter(s => f.timestamp_us >= s.audio_start_us && f.timestamp_us < s.audio_end_us);
    if (matching.length === 1) matching[0].frames.push(f);
    else unpaired.push({...f, label: 'unknown', reason: matching.length ? 'overlapping_audio_estimates' : 'no_observed_audio_segment'});
    return f;
  });
  const alignment = {status: 'unverified_estimate', scoring_allowed: false, physical_sync_validated: false,
    video_boundary: 'camera dequeue completion estimate', audio_boundary: 'ears read completion minus 20 ms estimate',
    native_adc_timestamp: false, permitted_pcm_clock_deviation_us: 2000,
    note: 'Read completion includes unknown buffering/driver delay. Driver timestamps are preserved but not used to assert physical alignment. No CFR, gap filling, or time stretching.',
    required_before_scoring: 'Separate local operator and Reviewer alignment record: visible/audible sync anchors at both ends, recorded offset, residual and uncertainty. Unsupported, drifting or ambiguous alignment stays unknown.'};
  mkdir('segments');
  const index = [], pendingManifests = [];
  for (const segment of segments) {
    const id = `segment-${String(segment.index + 1).padStart(4, '0')}`, dir = `segments/${id}`;
    mkdir(dir); mkdir(`${dir}/frames`);
    const dataBytes = segment.blocks.length * 640, wav = Buffer.alloc(44);
    wav.write('RIFF', 0); wav.writeUInt32LE(36 + dataBytes, 4); wav.write('WAVEfmt ', 8); wav.writeUInt32LE(16, 16);
    wav.writeUInt16LE(1, 20); wav.writeUInt16LE(1, 22); wav.writeUInt32LE(16000, 24); wav.writeUInt32LE(32000, 28);
    wav.writeUInt16LE(2, 32); wav.writeUInt16LE(16, 34); wav.write('data', 36); wav.writeUInt32LE(dataBytes, 40);
    const audioFile = `${dir}/audio.wav`, fd = fs.openSync(path.join(out, audioFile), 'wx', 0o600);
    try { fs.writeFileSync(fd, wav); for (const block of segment.blocks) fs.writeFileSync(fd, read(block.source.payload_file)); }
    finally { fs.closeSync(fd); }
    json(`${dir}/audio-timeline.json`, {schema_version: 1, capture_sha256: captureHash, alignment, audio_start_us: segment.audio_start_us,
      audio_end_us: segment.audio_end_us, boundary_reasons: segment.boundary_reasons, blocks: segment.blocks});
    const frames = segment.frames.map((frame, i) => {
      const file = `frames/${String(i).padStart(6, '0')}.png`;
      fs.linkSync(path.join(out, frame.image_file), path.join(out, dir, file));
      return {...frame, index: i, image_file: file};
    });
    const manifest = {schema_version: 1, clip_id: `${clipId}-s${String(segment.index + 1).padStart(4, '0')}`,
      capture_sha256: captureHash, capture_hash_basis: 'exact source inventory.jsonl bytes; every listed file verified',
      source_archive_clip_id: clipId, preview_only: true, scoring_allowed: false, physical_sync_validated: false, ready_us: null,
      timestamp_boundary: 'camera dequeue completion estimate', alignment, frames, audio_file: 'audio.wav', audio_sha256: sha(path.join(out, audioFile)),
      audio_start_us: segment.audio_start_us, audio_duration_us: segment.audio_end_us - segment.audio_start_us,
      audio_sample_rate: 16000, audio_channels: 1, audio_timeline_file: 'audio-timeline.json',
      audio_timeline_sha256: sha(path.join(out, dir, 'audio-timeline.json')), source_clock_id: session.clock_id};
    // A zero-frame segment is retained as evidence but cannot be loaded as a clip.
    pendingManifests.push({name: `${dir}/${frames.length ? 'manifest' : 'no-camera-evidence'}.json`, manifest});
    index.push({folder: dir, frame_count: frames.length, original_frame_indices: frames.map(f => f.original_index),
      audio_blocks: segment.blocks.length, audio_start_us: segment.audio_start_us, audio_end_us: segment.audio_end_us,
      boundary_reasons: segment.boundary_reasons, manifest_file: frames.length ? `${dir}/manifest.json` : null,
      audio_sha256: manifest.audio_sha256});
  }
  json('all-frames.json', {schema_version: 1, alignment, frames: allFrames});
  json('unknown-frames.json', {schema_version: 1, scoring_allowed: false, frames: unpaired});
  json('segments.json', {schema_version: 1, capture_sha256: captureHash, alignment, segments: index,
    original_frames: video.length, original_audio_blocks: audio.length, unpaired_frames: unpaired.length,
    camera_gaps: video.flatMap((r, i) => r.source_frames_between > 0 ? [{before_record_index: video[i - 1]?.record_index ?? null,
      after_record_index: r.record_index, missing_source_frames: r.source_frames_between,
      observed_before_dequeue_us: video[i - 1]?.frame.dequeue_completed_at_us ?? null,
      observed_after_dequeue_us: r.frame.dequeue_completed_at_us, label: 'unknown', gap_duration_known: false}] : []),
    gaps_are_unknown: true, sequence_gaps_are_not_durations: true});
  checkInventory();
  for (const pending of pendingManifests) json(pending.name, pending.manifest);
  frameCount = video.length; segmentCount = segments.length; unknownCount = unpaired.length; state = 'complete';
} catch (error) { failure = error.message; process.exitCode = 1; }
finally {
  json('commands.json', commands);
  json('result.json', {schema_version: 1, state, error: failure, capture_sha256: captureHash,
    preview_only: true, scoring_allowed: false, physical_sync_validated: false, frames: frameCount, audio_segments: segmentCount,
    unpaired_frames: unknownCount, started_utc: started, completed_utc: new Date().toISOString()});
  put('README.txt', 'PRIVATE LOCAL ONLY. Keep original archive and every derivative, including failures. Capture SHA256 is the exact original inventory.jsonl hash, with every listed file and the terminal counts verified. source-records.json preserves all native FrameMeta, Bus/Audio metadata, sequence gaps and receipt times. source-frames.mjpeg is an unclocked ordered concatenation for decoding; its demuxer clock is NOT evidence. All original observed frames are in frames/ and all-frames.json. Each segments/ folder contains only contiguous observed PCM; missing PCM is never silence, interpolated or gap-filled. unknown-frames.json keeps frames with missing/ambiguous audio. Segment PNGs are hard links to the retained frame PNGs. These are UNVERIFIED estimated previews: camera dequeue completion versus audio read completion minus block duration, not native ADC or physical synchronization. Independent local operator AND Reviewer checks with visible/audible sync anchors at both ends, measured offset/residual/uncertainty are mandatory before scoring. Unsupported, drifting or ambiguous sync stays unknown. scoring_allowed is always false here. Node/FFmpeg host processing duration never measures lamp readiness. SHA256SUMS covers all derived files except itself. No originals are modified and no devices or network services are opened.\n');
  put('SHA256SUMS', walk(out).map(name => `${sha(path.join(out, name))}  ${name}`).join('\n') + '\n');
  console.log(JSON.stringify({state, out, capture_sha256: captureHash, frames: frameCount, audio_segments: segmentCount,
    unpaired_frames: unknownCount, scoring_allowed: false, error: failure}));
}
NODE
