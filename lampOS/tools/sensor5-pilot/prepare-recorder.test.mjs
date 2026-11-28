import test, {after} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sensor5-native-prepare-tests-'));
process.umask(0o077); fs.chmodSync(root, 0o700);
console.log(`Retained native preparation evidence: ${root}`);
const digest = b => crypto.createHash('sha256').update(b).digest('hex');
const sha = p => digest(fs.readFileSync(p));
const jsonBytes = v => Buffer.from(JSON.stringify(v, null, 2) + '\n');
const readJSON = p => JSON.parse(fs.readFileSync(p));
const filesIn = dir => fs.readdirSync(dir, {withFileTypes: true}).flatMap(e => e.isDirectory() ?
  filesIn(path.join(dir, e.name)).map(n => `${e.name}/${n}`) : [e.name]).sort();
const snapshot = dir => filesIn(dir).map(n => [n, fs.lstatSync(path.join(dir, n)).isSymbolicLink() ?
  `link:${fs.readlinkSync(path.join(dir, n))}` : sha(path.join(dir, n))]);
const receipts = [];
fs.mkdirSync(path.join(root, 'jpeg-seeds'), {mode: 0o700});
const seed = spawnSync('ffmpeg', ['-hide_banner', '-nostdin', '-n', '-f', 'lavfi', '-i',
  'color=red:size=16x16:rate=5:duration=1,hue=H=2*PI*t', '-frames:v', '5', '-c:v', 'mjpeg', '-threads:v', '1', '-q:v', '2',
  path.join(root, 'jpeg-seeds/%02d.jpg')], {encoding: 'utf8', timeout: 10000});
fs.writeFileSync(path.join(root, 'jpeg-seeds.stderr'), seed.stderr ?? '');
assert.equal(seed.status, 0, seed.stderr);
const jpeg = filesIn(path.join(root, 'jpeg-seeds')).map(n => fs.readFileSync(path.join(root, 'jpeg-seeds', n)));
assert.equal(new Set(jpeg.map(digest)).size, 5, 'Distinct source frames are required to test image identity/order');

function fixture(name, options = {}) {
  const dir = path.join(root, `${name}-raw`);
  const frameTimes = options.frameTimes ?? [1000000, 1050000, 1130000];
  const audioTimes = options.audioTimes ?? Array.from({length: 10}, (_, i) => 1000000 + i * 20000);
  const data = new Map([
    ['README.txt', Buffer.from('Synthetic only, no people. Keep all evidence.\n')],
    ['session.json', jsonBytes({schema_version: 1, clock_id: 'synthetic-clock', synthetic: true, physical_sync_validated: false})],
    ['consent.json', jsonBytes({synthetic: true})], ['consent.txt', Buffer.from('Synthetic test consent; no human data.\n')]
  ]);
  const records = [], video = [], audio = [];
  const add = (record, payload, extension) => {
    record.schema_version = 1; record.record_index = records.length + 1;
    record.payload_file = `records/${String(record.record_index).padStart(8, '0')}.${extension}`;
    record.sha256 = digest(payload);
    data.set(record.payload_file, payload); data.set(record.payload_file + '.json', jsonBytes(record));
    records.push(record);
  };
  // Interleave source records so original record index is distinct from frame index.
  const ordered = [...frameTimes.map((time, index) => ({kind: 'frame', time, index})),
    ...audioTimes.map((time, index) => ({kind: 'audio', time, index}))].sort((a, b) => a.time - b.time || a.kind.localeCompare(b.kind));
  for (const item of ordered) {
    const i = item.index, at = item.time;
    if (item.kind === 'frame') {
      const frame = {source: 'lamp-eyes', format: [77,74,80,71], width: 16, height: 16, request: i + 1,
        grant: {boot: Array(16).fill(1), generation: 1}, camera_worker: Array(16).fill(2), capture_epoch: 3,
        frame_sequence: (options.frameSequences ?? [41,42,43,44,45])[i] ?? 41 + i, driver_sequence: 100 + i,
        driver_timestamp: {micros: 1700000000000000 + at, domain: 'realtime', point: 'end_of_frame'},
        dequeue_started_at_us: at - 100, dequeue_completed_at_us: at, copied_at_us: at + 10, derived: null};
      const gap = i ? frame.frame_sequence - video.at(-1).frame.frame_sequence - 1 : 0;
      const r = {kind: 'frame', frame, received_us: at + 100, source_frames_between: gap};
      options.frame?.(r, i); video.push(r); add(r, jpeg[i % jpeg.length], 'frame.jpg');
    } else {
      const r = {kind: 'audio', audio: {channels: 1, sample_format: 'S16LE', sample_rate: 16000, sequence: 30 + i},
        header: {at_unix_ms: 1000 + i * 20, at_us: at, boot: '3333333333333333', enc: 'json+pcm', expires_ms: 50,
          kind: 'event', seq: 14 + i, source: 'ears', topic: 'mic1/audio', type: 'lamp_msgs/Audio'},
        audio_sequence_gap: 0, bus_gap: 0, clock_provenance: 'existing_ears_privacy_runtime_source', received_us: at + 100,
        timestamp_boundary: 'ears_capture_read_completed', adc_timestamp: false};
      options.audio?.(r, i); audio.push(r); add(r, Buffer.alloc(640, i), 'audio.pcm');
    }
  }
  if (options.privacy) add({kind: 'privacy', evidence: 'synthetic'}, Buffer.from('{}'), 'privacy.json');
  const terminal = {schema_version: 1, complete: true, records: records.length, frames: video.length, audio_blocks: audio.length,
    result: {frames_admitted: video.length, audio_blocks_admitted: audio.length, reason: 'duration_elapsed', physical_sync_validated: false}};
  options.terminal?.(terminal);
  data.set('result.json', options.resultBytes ?? jsonBytes(terminal));
  options.edit?.(data, {video, audio, records});
  const inventory = [...data].map(([name, bytes]) => ({path: name, bytes: bytes.length, sha256: digest(bytes)}));
  options.inventory?.(inventory);
  data.set('inventory.jsonl', Buffer.from(inventory.map(e => JSON.stringify(e)).join('\n') + '\n'));
  options.afterInventory?.(data);
  fs.mkdirSync(dir, {mode: 0o700}); fs.mkdirSync(path.join(dir, 'records'), {mode: 0o700});
  for (const [name, bytes] of data) fs.writeFileSync(path.join(dir, name), bytes, {flag: 'wx', mode: 0o600});
  if (options.symlink) fs.symlinkSync(path.join(root, 'jpeg-seeds/01.jpg'), path.join(dir, 'unlisted-link'));
  return {dir, data, video, audio, records, before: snapshot(dir)};
}

function run(name, raw, extra = {}) {
  const out = extra.out ?? path.join(root, `${name}-prepared`);
  const script = extra.script ?? path.join(here, 'prepare-recorder.sh');
  const args = [script, '--archive', raw.dir, '--clip-id', name, '--out', out, ...(extra.args ?? [])];
  const p = spawnSync('bash', args, {encoding: 'utf8', timeout: 15000, env: extra.env ?? process.env});
  fs.writeFileSync(path.join(root, `${name}.stdout`), p.stdout ?? ''); fs.writeFileSync(path.join(root, `${name}.stderr`), p.stderr ?? '');
  receipts.push({name, args, status: p.status, signal: p.signal, error: p.error?.message ?? null});
  assert.deepEqual(snapshot(raw.dir), raw.before, 'Preparation must never modify the archive');
  return {p, out};
}
function verifySeal(dir) {
  const lines = fs.readFileSync(path.join(dir, 'SHA256SUMS'), 'utf8').trim().split('\n');
  assert.deepEqual(lines.map(s => s.slice(66)).sort(), filesIn(dir).filter(n => n !== 'SHA256SUMS'));
  for (const line of lines) {
    assert.match(line, /^[0-9a-f]{64}  /);
    assert.equal(sha(path.join(dir, line.slice(66))), line.slice(0, 64), line.slice(66));
  }
}
function success(result) {
  assert.equal(result.p.status, 0, result.p.stderr + result.p.stdout);
  assert.equal(readJSON(path.join(result.out, 'result.json')).state, 'complete'); verifySeal(result.out);
  return readJSON(path.join(result.out, 'segments.json'));
}
function failed(result, pattern) {
  assert.notEqual(result.p.status, 0);
  const terminal = readJSON(path.join(result.out, 'result.json'));
  assert.equal(terminal.state, 'failed'); assert.equal(terminal.scoring_allowed, false); assert.match(terminal.error, pattern);
  verifySeal(result.out);
}
function manifest(out, segment = 1) {
  const dir = path.join(out, `segments/segment-${String(segment).padStart(4, '0')}`);
  return {dir, value: readJSON(path.join(dir, 'manifest.json'))};
}

test('native frame identities, irregular dequeue times, PCM bytes and hashes survive preparation', () => {
  const raw = fixture('native', {privacy: true}), result = run('native', raw), index = success(result);
  assert.equal(index.segments.length, 1); assert.equal(index.original_frames, 3); assert.equal(index.original_audio_blocks, 10);
  const {dir, value: m} = manifest(result.out);
  assert.equal(m.capture_sha256, sha(path.join(raw.dir, 'inventory.jsonl')));
  assert.equal(m.audio_start_us, 980000); assert.equal(m.audio_duration_us, 200000);
  assert.equal(m.scoring_allowed, false); assert.equal(m.alignment.status, 'unverified_estimate'); assert.equal(m.ready_us, null);
  assert.equal(m.physical_sync_validated, false); assert.equal(m.alignment.native_adc_timestamp, false);
  assert.match(m.timestamp_boundary, /dequeue.*estimate/);
  for (const [i, f] of m.frames.entries()) {
    assert.equal(f.index, i); assert.equal(f.original_index, i); assert.equal(f.source_record_index, raw.video[i].record_index);
    assert.equal(f.timestamp_us, raw.video[i].frame.dequeue_completed_at_us); assert.deepEqual(f.native_frame, raw.video[i].frame);
    assert.notEqual(f.timestamp_us, f.native_frame.driver_timestamp.micros);
    assert.equal(sha(path.join(dir, f.image_file)), f.sha256);
    const expected = path.join(root, `independent-frame-${i}.png`);
    const decoded = spawnSync('ffmpeg', ['-hide_banner', '-nostdin', '-n', '-i', path.join(raw.dir, raw.video[i].payload_file),
      '-frames:v', '1', '-c:v', 'png', '-threads:v', '1', expected], {encoding: 'utf8', timeout: 10000});
    fs.writeFileSync(path.join(root, `independent-frame-${i}.stderr`), decoded.stderr ?? '');
    assert.equal(decoded.status, 0, decoded.stderr); assert.equal(sha(expected), f.sha256, 'PNG must belong to the exact original JPEG');
  }
  const wav = fs.readFileSync(path.join(dir, m.audio_file));
  assert.equal(wav.subarray(0, 4).toString(), 'RIFF'); assert.equal(wav.readUInt32LE(24), 16000);
  assert.equal(wav.readUInt16LE(22), 1); assert.equal(wav.readUInt16LE(34), 16);
  assert.deepEqual(wav.subarray(44), Buffer.concat(raw.audio.map(r => raw.data.get(r.payload_file))));
  assert.equal(sha(path.join(dir, m.audio_file)), m.audio_sha256);
  const timeline = readJSON(path.join(dir, m.audio_timeline_file));
  assert.deepEqual(timeline.blocks.map(b => b.source), raw.audio);
  assert.equal(sha(path.join(dir, m.audio_timeline_file)), m.audio_timeline_sha256);
  for (const name of filesIn(result.out)) assert.equal(fs.statSync(path.join(result.out, name)).mode & 0o777, 0o600, name);
  assert.equal(fs.statSync(result.out).mode & 0o777, 0o700);
});

test('missing PCM splits WAVs and leaves frames in the gap explicitly unknown', () => {
  const raw = fixture('gap', {frameTimes: [1000000,1030000,1050000,1090000], audioTimes: [1000000,1020000,1040000,1100000,1120000],
    audio(r, i) { if (i >= 3) { r.audio.sequence += 2; r.header.seq += 2; } if (i === 3) { r.audio_sequence_gap = 2; r.bus_gap = 2; } }});
  const result = run('gap', raw), index = success(result);
  assert.equal(index.segments.length, 2); assert.equal(index.unpaired_frames, 1);
  assert.deepEqual(index.segments.map(s => s.original_frame_indices), [[0,1], [3]]);
  assert.deepEqual(index.segments.map(s => s.audio_blocks), [3,2]);
  const unknown = readJSON(path.join(result.out, 'unknown-frames.json')).frames;
  assert.equal(unknown[0].original_index, 2); assert.equal(unknown[0].label, 'unknown');
  assert.equal(unknown[0].reason, 'no_observed_audio_segment');
  assert.deepEqual(manifest(result.out, 2).value.frames.map(f => [f.index, f.original_index]), [[0,3]]);
  assert.equal(fs.readFileSync(path.join(manifest(result.out, 1).dir, 'audio.wav')).length, 44 + 3 * 640);
  assert.equal(fs.readFileSync(path.join(manifest(result.out, 2).dir, 'audio.wav')).length, 44 + 2 * 640);
});

test('sample-clock drift alone splits, and overlapping estimates do not assign a face silently', () => {
  const raw = fixture('overlap', {frameTimes: [1000000,1015000,1030000], audioTimes: [1000000,1020000,1035000,1055000]});
  const result = run('overlap', raw), index = success(result);
  assert.equal(index.segments.length, 2); assert.equal(index.unpaired_frames, 1);
  assert.ok(index.segments[1].boundary_reasons.includes('sample_clock_deviation_over_2ms'));
  assert.equal(readJSON(path.join(result.out, 'unknown-frames.json')).frames[0].reason, 'overlapping_audio_estimates');
});

test('sub-2 ms completion jitter remains measured with every original block timestamp retained', () => {
  const raw = fixture('jitter', {audioTimes: [1000000,1021000,1039000,1060000,1081500]});
  const result = run('jitter', raw), index = success(result);
  assert.equal(index.segments.length, 1);
  const timeline = readJSON(path.join(manifest(result.out).dir, 'audio-timeline.json'));
  assert.deepEqual(timeline.blocks.map(b => b.completion_minus_pcm_estimate_us), [0,1000,-1000,0,1500]);
  assert.deepEqual(timeline.blocks.map(b => b.source.header.at_us), [1000000,1021000,1039000,1060000,1081500]);
});

test('bus-only gaps split independently of sample-clock continuity and retain camera sequence gaps', () => {
  const raw = fixture('bus-gap', {frameSequences: [41,44,45], audio(r, i) { if (i >= 5) r.header.seq++; if (i === 5) r.bus_gap = 1; }});
  const result = run('bus-gap', raw), index = success(result);
  assert.equal(index.segments.length, 2); assert.ok(index.segments[1].boundary_reasons.includes('declared_bus_gap'));
  const frames = readJSON(path.join(result.out, 'all-frames.json')).frames;
  assert.deepEqual(frames.map(f => f.source_frames_between), [0,2,0]);
  assert.equal(index.sequence_gaps_are_not_durations, true);
  assert.equal(index.camera_gaps[0].missing_source_frames, 2); assert.equal(index.camera_gaps[0].label, 'unknown');
  assert.equal(index.camera_gaps[0].gap_duration_known, false);
});

test('audio without camera remains retained but has no misleading labeller manifest', () => {
  const raw = fixture('no-camera', {frameTimes: [1000000,1010000], audioTimes: [1000000,1020000,1100000,1120000]});
  const result = run('no-camera', raw), index = success(result);
  assert.equal(index.segments[1].frame_count, 0); assert.equal(index.segments[1].manifest_file, null);
  const dir = path.join(result.out, index.segments[1].folder);
  assert.equal(fs.existsSync(path.join(dir, 'manifest.json')), false);
  assert.equal(readJSON(path.join(dir, 'no-camera-evidence.json')).scoring_allowed, false);
  assert.equal(fs.statSync(path.join(dir, 'audio.wav')).size, 44 + 1280);
});

for (const [name, options, error] of [
  ['incomplete', {terminal(t) { t.complete = false; }}, /Incomplete archive/],
  ['truncated-result', {resultBytes: Buffer.from('{"complete":')}, /Invalid\/truncated result/],
  ['empty-result', {resultBytes: Buffer.alloc(0)}, /Invalid\/truncated result/],
  ['wrong-count', {terminal(t) { t.audio_blocks++; }}, /counts do not match/],
  ['truncated-sidecar', {edit(data, media) { data.set(media.video[0].payload_file + '.json', Buffer.from('{')); }}, /Invalid\/truncated records/],
  ['bad-hash', {inventory(entries) { entries.find(e => e.path.endsWith('.audio.pcm')).sha256 = '0'.repeat(64); }}, /hash\/size mismatch/],
  ['missing-payload', {inventory(entries) { entries.push({path: 'records/99999999.audio.pcm', bytes: 640, sha256: '0'.repeat(64)}); }}, /exact archive file set/],
  ['unlisted-partial', {afterInventory(data) { data.set('partial.tmp', Buffer.from('retained partial')); }}, /exact archive file set/],
  ['unsafe-inventory', {inventory(entries) { entries[0].path = '../outside'; }}, /Unsafe or duplicate/],
  ['duplicate-inventory', {inventory(entries) { entries.push(entries[0]); }}, /Unsafe or duplicate/],
  ['unknown-clock', {audio(r) { r.header.clock = 'different-host-clock'; r.clock_provenance = 'bus_header'; }}, /clock provenance/],
  ['camera-epoch', {frame(r, i) { if (i === 2) r.frame.capture_epoch++; }}, /lineage\/order\/gap/],
  ['camera-order', {frame(r, i) { if (i === 2) { r.frame.dequeue_started_at_us = 1000000; r.frame.dequeue_completed_at_us = 1000010; } }}, /lineage\/order\/gap/],
  ['inexact-identity', {frame(r) { r.frame.request = Number.MAX_SAFE_INTEGER + 1; }}, /inexact source metadata number/],
  ['symlink', {symlink: true}, /Symlink refused/]
]) test(`${name} fails closed and retains a sealed failure without changing raw evidence`, () => {
  const raw = fixture(name, options), result = run(name, raw); failed(result, error);
  assert.equal(fs.existsSync(path.join(result.out, 'segments.json')), false);
});

test('existing output, output symlink and output inside raw archive are refused without mutation', () => {
  const raw = fixture('collision'), out = path.join(root, 'existing');
  fs.mkdirSync(out); fs.writeFileSync(path.join(out, 'keep'), 'preserve');
  const before = snapshot(out);
  assert.notEqual(run('collision', raw, {out}).p.status, 0); assert.deepEqual(snapshot(out), before);
  const link = path.join(root, 'existing-link'); fs.symlinkSync(out, link);
  assert.notEqual(run('collision-link', raw, {out: link}).p.status, 0); assert.deepEqual(snapshot(out), before);
  assert.notEqual(run('inside-raw', raw, {out: path.join(raw.dir, 'derived')}).p.status, 0);
});

test('FFmpeg failure retains its partial image, logs and failure receipt', () => {
  const raw = fixture('decoder-failure'), bin = path.join(root, 'bad-bin'); fs.mkdirSync(bin);
  fs.writeFileSync(path.join(bin, 'ffmpeg'), `#!${process.execPath}\nconst fs=require('node:fs');fs.writeFileSync(process.argv.at(-1).replace('%06d','000000'),'partial');console.error('deliberate decoder failure');process.exit(17);\n`, {mode: 0o700});
  const result = run('decoder-failure', raw, {env: {...process.env, PATH: `${bin}${path.delimiter}${process.env.PATH}`}});
  failed(result, /images failed/);
  assert.equal(fs.readFileSync(path.join(result.out, 'frames/000000.png'), 'utf8'), 'partial');
  assert.match(fs.readFileSync(path.join(result.out, 'images.stderr'), 'utf8'), /deliberate decoder failure/);
  assert.equal(readJSON(path.join(result.out, 'commands.json'))[0].exit_code, 17);
});

test('source-only copied adapter runs outside the repository and supports spaces/quotes in paths', () => {
  const dir = path.join(root, "source only ' copied"); fs.mkdirSync(dir);
  const script = path.join(dir, 'prepare-recorder.sh'); fs.copyFileSync(path.join(here, 'prepare-recorder.sh'), script);
  fs.copyFileSync(fileURLToPath(import.meta.url), path.join(dir, 'prepare-recorder.test.mjs'));
  const raw = fixture('portable');
  const result = run('portable', raw, {script, out: path.join(dir, "prepared ' result")});
  success(result);
  assert.equal(manifest(result.out).value.frames.length, 3);
});

after(() => {
  fs.writeFileSync(path.join(root, 'test-receipts.json'), jsonBytes(receipts));
  fs.writeFileSync(path.join(root, 'retained-symlinks.json'), jsonBytes(filesIn(root).filter(n => fs.lstatSync(path.join(root, n)).isSymbolicLink())
    .map(n => ({path: n, target: fs.readlinkSync(path.join(root, n))}))));
  fs.writeFileSync(path.join(root, 'README.txt'), 'Synthetic native-recorder archive tests only; generated JPEG test patterns and patterned PCM, no people/device access. Every source, deliberate invalid/partial fixture and failed derivative is retained. Do not delete. SHA256SUMS covers regular files except itself; deliberate symlinks are recorded in retained-symlinks.json and remain retained.\n');
  const entries = filesIn(root).filter(n => !fs.lstatSync(path.join(root, n)).isSymbolicLink()).map(n => `${sha(path.join(root, n))}  ${n}`);
  fs.writeFileSync(path.join(root, 'SHA256SUMS'), entries.join('\n') + '\n');
});
