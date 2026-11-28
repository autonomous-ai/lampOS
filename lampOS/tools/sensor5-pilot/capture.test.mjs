import test, {after} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {spawnSync, spawn} from 'node:child_process';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sensor5-capture-tests-'));
fs.chmodSync(root, 0o700);
console.log(`Retained capture-test evidence: ${root}`);
const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const args = name => ['--out', path.join(root, name), '--clip-id', name, '--participant', 'P01',
  '--class', 'A', '--distance', '0.6', '--lighting', 'daylight'];
const devices = ['--video', '/dev/video98765', '--audio', 'hw:999,0', '--video-size', '320x240',
  '--video-format', 'mjpeg', '--video-rate', '25', '--audio-rate', '48000', '--audio-channels', '2'];
const consent = path.join(root, 'consent.local');
fs.writeFileSync(consent, 'Participant alias: P01\nConsent accepted: yes\nSynthetic test record only.\n', {mode: 0o600});
const gates = ['--consent', consent, '--owner-slot', 'synthetic-test-reference', '--privacy-open', '--local-storage-confirmed'];
const receipts = [];
const run = (name, script, argv, options = {}) => {
  const p = spawnSync('bash', [path.join(here, script), ...argv], {encoding: 'utf8', timeout: 45000, ...options});
  fs.writeFileSync(path.join(root, `${name}.stdout`), p.stdout ?? '');
  fs.writeFileSync(path.join(root, `${name}.stderr`), p.stderr ?? '');
  receipts.push({name, argv: [script, ...argv], status: p.status, signal: p.signal, error: p.error?.message ?? null});
  return p;
};
const verifyManifest = dir => {
  for (const line of fs.readFileSync(path.join(dir, 'SHA256SUMS'), 'utf8').trim().split('\n')) {
    const match = /^([0-9a-f]{64})  (.+)$/.exec(line);
    assert.ok(match, line);
    assert.equal(sha(path.join(dir, match[2])), match[1], match[2]);
  }
};
const fakeBin = (name, body) => {
  const dir = path.join(root, name);
  fs.mkdirSync(dir, {mode: 0o700});
  fs.writeFileSync(path.join(dir, 'ffmpeg'), `#!${process.execPath}\nimport fs from 'node:fs';\nimport path from 'node:path';\nif (process.argv[2] === '-version') { console.log('ffmpeg version 8.1.1 fixture'); process.exit(0); }\n${body}\n`, {mode: 0o700});
  fs.writeFileSync(path.join(dir, 'package.json'), '{"type":"module"}\n');
  return {...process.env, PATH: `${dir}${path.delimiter}${process.env.PATH}`};
};

test('default is a non-writing dry-run with explicit devices', () => {
  const p = run('dry', 'capture.sh', [...args('dry'), ...devices]);
  assert.equal(p.status, 0, p.stderr);
  const request = JSON.parse(p.stdout);
  assert.equal(request.mode, 'dry-run');
  assert.equal(request.ready_us, null);
  assert.equal(request.command.filter(x => x === '-i').length, 2);
  assert.ok(request.command.includes('-copyts'));
  assert.ok(request.command.includes('passthrough'));
  assert.equal(fs.existsSync(path.join(root, 'dry')), false);
});

test('live gates refuse missing consent, a mismatched alias and unaccepted consent', () => {
  const absent = run('gate-absent', 'capture.sh', [...args('gate-absent'), ...devices, '--live']);
  assert.notEqual(absent.status, 0);
  assert.equal(fs.existsSync(path.join(root, 'gate-absent')), false);
  const mismatch = run('gate-mismatch', 'capture.sh', [...args('gate-mismatch'), ...devices, ...gates, '--participant', 'P02', '--live']);
  assert.notEqual(mismatch.status, 0);
  assert.match(mismatch.stderr, /does not match/);
  const rejected = path.join(root, 'unaccepted.local');
  fs.writeFileSync(rejected, 'Participant alias: P01\nConsent accepted: no\n');
  const p = run('gate-no', 'capture.sh', [...args('gate-no'), ...devices, ...gates, '--consent', rejected, '--live']);
  assert.notEqual(p.status, 0);
  assert.match(p.stderr, /explicitly accepted/);
});

test('unsafe identifiers, durations and mode ambiguity are refused', () => {
  for (const [suffix, bad] of [['path', ['--clip-id', '../wrong']], ['long', ['--seconds', '600']],
      ['mix', ['--synthetic', '--live']], ['rate', ['--audio-rate', '$(touch /tmp/no)']]]) {
    const p = run(`unsafe-${suffix}`, 'capture.sh', [...args(`unsafe-${suffix}`), ...devices, ...bad]);
    assert.notEqual(p.status, 0, suffix);
    assert.equal(fs.existsSync(path.join(root, `unsafe-${suffix}`)), false);
  }
});

test('full operator acknowledgements cannot bypass missing live privacy supervision', () => {
  const p = run('privacy-hold', 'capture.sh', [...args('privacy-hold'), ...devices, ...gates, '--live']);
  assert.notEqual(p.status, 0);
  assert.match(p.stderr, /verified microphone and camera permit supervisor/);
  assert.equal(fs.existsSync(path.join(root, 'privacy-hold')), false);
});

test('existing output and symlink output are refused without mutation', () => {
  const existing = path.join(root, 'exists');
  fs.mkdirSync(existing); fs.writeFileSync(path.join(existing, 'keep'), 'retain');
  const p = run('collision', 'capture.sh', [...args('exists'), '--synthetic']);
  assert.notEqual(p.status, 0);
  assert.deepEqual(fs.readdirSync(existing), ['keep']);
  fs.symlinkSync(existing, path.join(root, 'link'));
  const q = run('symlink', 'capture.sh', [...args('link'), '--synthetic']);
  assert.notEqual(q.status, 0);
  assert.equal(fs.readFileSync(path.join(existing, 'keep'), 'utf8'), 'retain');
});

test('synthetic capture preserves 250 frames and an actual 125 ms audio offset', () => {
  const p = run('synthetic', 'capture.sh', [...args('synthetic'), '--synthetic']);
  assert.equal(p.status, 0, p.stderr);
  const dir = path.join(root, 'synthetic');
  assert.equal(fs.statSync(dir).mode & 0o777, 0o700);
  const result = JSON.parse(fs.readFileSync(path.join(dir, 'result.json')));
  assert.equal(result.state, 'complete'); assert.equal(result.ready_us, null);
  const f = JSON.parse(fs.readFileSync(path.join(dir, 'frames.json'))).frames;
  const video = f.filter(x => x.media_type === 'video'), audio = f.filter(x => x.media_type === 'audio');
  assert.equal(video.length, 250);
  assert.equal(Number(audio[0].pts_time) - Number(video[0].pts_time), 0.125);
  verifyManifest(dir);
});

test('derivation hashes actual frame/audio bytes and keeps the shared clock origin', () => {
  const capture = path.join(root, 'synthetic/capture.nut'), before = sha(capture);
  const p = run('prepare', 'prepare-labels.sh', ['--capture', capture, '--clip-id', 'synthetic', '--out', path.join(root, 'prepared')]);
  assert.equal(p.status, 0, p.stderr);
  const dir = path.join(root, 'prepared'), m = JSON.parse(fs.readFileSync(path.join(dir, 'manifest.json')));
  assert.equal(m.capture_sha256, before); assert.equal(m.frames.length, 250);
  assert.equal(m.audio_start_us, 125000); assert.equal(m.audio_video_start_offset_us, 125000);
  assert.equal(m.audio_max_pcm_clock_error_us, 0); assert.equal(m.physical_sync_validated, false);
  for (const f of m.frames) assert.equal(sha(path.join(dir, f.image_file)), f.sha256);
  assert.equal(sha(path.join(dir, m.audio_file)), m.audio_sha256);
  assert.equal(sha(capture), before); verifyManifest(dir);
  const again = run('prepare-collision', 'prepare-labels.sh', ['--capture', capture, '--clip-id', 'synthetic', '--out', dir]);
  assert.notEqual(again.status, 0); verifyManifest(dir);
});

test('a failed encoder retains partial data, command, logs and terminal failure', () => {
  const env = fakeBin('failure-bin', `fs.writeFileSync(process.argv.at(-1), 'partial fixture'); console.error('intentional encoder failure'); process.exit(17);`);
  const p = run('failed-encoder', 'capture.sh', [...args('failed-encoder'), '--synthetic'], {env});
  assert.notEqual(p.status, 0);
  const dir = path.join(root, 'failed-encoder');
  assert.equal(fs.readFileSync(path.join(dir, 'capture.nut'), 'utf8'), 'partial fixture');
  const result = JSON.parse(fs.readFileSync(path.join(dir, 'result.json')));
  assert.equal(result.ffmpeg_exit_code, 17); assert.equal(result.state, 'failed');
  assert.match(fs.readFileSync(path.join(dir, 'ffmpeg.stderr'), 'utf8'), /intentional/);
  verifyManifest(dir);
});

for (const ignoresTerm of [false, true]) test(`operator interruption retains and stops encoder (ignores TERM: ${ignoresTerm})`, {timeout: 15000}, async () => {
  const name = ignoresTerm ? 'interrupted-stuck' : 'interrupted';
  const env = fakeBin(`${name}-bin`, `const out=path.dirname(process.argv.at(-1)); fs.writeFileSync(path.join(out,'fixture-pid'),String(process.pid)); fs.writeFileSync(process.argv.at(-1),'interrupted fixture'); process.on('SIGTERM',()=>${ignoresTerm ? '{}' : 'process.exit(143)'}); setInterval(()=>{},1000);`);
  const dir = path.join(root, name);
  const child = spawn('bash', [path.join(here, 'capture.sh'), ...args(name), '--synthetic'], {env});
  let stdout = '', stderr = '';
  child.stdout.on('data', b => { stdout += b; }); child.stderr.on('data', b => { stderr += b; });
  const done = new Promise(resolve => child.once('exit', (code, signal) => resolve({code, signal})));
  const until = Date.now() + 8000;
  while (!fs.existsSync(path.join(dir, 'fixture-pid')) && Date.now() < until) await new Promise(r => setTimeout(r, 25));
  assert.ok(fs.existsSync(path.join(dir, 'fixture-pid')), stderr);
  const stopStarted = Date.now();
  child.kill('SIGTERM');
  const result = await done;
  assert.ok(Date.now() - stopStarted < 5000, 'Stop must not wait for the original take deadline');
  fs.writeFileSync(path.join(root, `${name}.stdout`), stdout); fs.writeFileSync(path.join(root, `${name}.stderr`), stderr);
  assert.notEqual(result.code, 0);
  const terminal = JSON.parse(fs.readFileSync(path.join(dir, 'result.json')));
  assert.equal(terminal.reason, 'operator_interrupted'); assert.equal(terminal.signal, 'TERM');
  const pid = Number(fs.readFileSync(path.join(dir, 'fixture-pid')));
  assert.throws(() => process.kill(pid, 0));
  verifyManifest(dir);
});

test('native variable-frame PTS and a nonzero common origin survive preparation', () => {
  const source = path.join(root, 'vfr.nut');
  const argv = ['-hide_banner', '-nostdin', '-n', '-copyts', '-f', 'lavfi', '-i',
    'testsrc2=size=160x120:rate=25:duration=1,select=not(eq(mod(n\\,3)\\,0)),setpts=PTS+5/TB',
    '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000:duration=1,asetpts=PTS+5.125/TB',
    '-map','0:v:0','-map','1:a:0','-c:v','ffv1','-level','3','-enc_time_base:v','-1','-fps_mode:v','passthrough',
    '-c:a','pcm_s16le','-avoid_negative_ts','disabled','-f','nut',source];
  const made = spawnSync('ffmpeg', argv, {encoding: 'utf8'});
  fs.writeFileSync(path.join(root, 'vfr-fixture.stderr'), made.stderr); assert.equal(made.status, 0, made.stderr);
  const dir = path.join(root, 'vfr-prepared');
  const p = run('vfr-prepare', 'prepare-labels.sh', ['--capture', source, '--clip-id', 'vfr', '--out', dir]);
  assert.equal(p.status, 0, p.stderr);
  const m = JSON.parse(fs.readFileSync(path.join(dir, 'manifest.json')));
  assert.equal(m.frames.length, 16); assert.equal(m.video_start_us, 5040000); assert.equal(m.audio_start_us, 5125000);
  assert.deepEqual([...new Set(m.frames.slice(1).map((f, i) => f.timestamp_us - m.frames[i].timestamp_us))].sort(), [40000,80000]);
  verifyManifest(dir);
});

test('absolute epoch timestamps keep the input duration bound and audio offset', () => {
  const source = path.join(root, 'epoch.nut');
  const made = spawnSync('ffmpeg', ['-hide_banner','-nostdin','-n','-copyts','-f','lavfi','-t','1','-i',
    'testsrc2=size=160x120:rate=25:duration=2,setpts=PTS+1770000000/TB','-f','lavfi','-t','1','-i',
    'sine=frequency=440:sample_rate=48000:duration=2,asetpts=PTS+1770000000.125/TB',
    '-map','0:v:0','-map','1:a:0','-c:v','ffv1','-level','3','-enc_time_base:v','-1','-fps_mode:v','passthrough',
    '-c:a','pcm_s16le','-avoid_negative_ts','disabled','-f','nut',source],{encoding:'utf8'});
  fs.writeFileSync(path.join(root,'epoch-fixture.stderr'),made.stderr); assert.equal(made.status,0,made.stderr);
  const dir = path.join(root,'epoch-prepared');
  const p=run('epoch-prepare','prepare-labels.sh',['--capture',source,'--clip-id','epoch','--out',dir]);
  assert.equal(p.status,0,p.stderr);
  const m=JSON.parse(fs.readFileSync(path.join(dir,'manifest.json')));
  assert.equal(m.frames.length,25); assert.equal(m.video_start_us,1770000000000000);
  assert.equal(m.frames.at(-1).timestamp_us-m.frames[0].timestamp_us,960000);
  assert.equal(m.audio_video_start_offset_us,125000); verifyManifest(dir);
});

test('audio discontinuities are rejected and retained instead of resynchronized silently', () => {
  const source = path.join(root, 'audio-gap.nut');
  const made = spawnSync('ffmpeg', ['-hide_banner','-nostdin','-n','-copyts','-f','lavfi','-i','testsrc2=size=160x120:rate=25:duration=1',
    '-f','lavfi','-i','sine=frequency=440:sample_rate=48000:duration=1,asetpts=PTS+0.010*floor(N/24000)/TB',
    '-map','0:v:0','-map','1:a:0','-c:v','ffv1','-level','3','-enc_time_base:v','-1','-fps_mode:v','passthrough',
    '-c:a','pcm_s16le','-avoid_negative_ts','disabled','-f','nut',source], {encoding:'utf8'});
  fs.writeFileSync(path.join(root, 'gap-fixture.stderr'), made.stderr); assert.equal(made.status,0,made.stderr);
  const dir = path.join(root, 'gap-prepared');
  const p = run('gap-prepare','prepare-labels.sh',['--capture',source,'--clip-id','gap','--out',dir]);
  assert.notEqual(p.status,0);
  const r = JSON.parse(fs.readFileSync(path.join(dir,'result.json')));
  assert.match(r.error,/>2 ms/); assert.equal(fs.existsSync(path.join(dir,'manifest.json')),false);
  verifyManifest(dir);
});

after(() => {
  fs.writeFileSync(path.join(root, 'commands.json'), JSON.stringify(receipts, null, 2) + '\n');
  fs.writeFileSync(path.join(root, 'README.txt'), 'Retained synthetic-only capture/preparation test evidence. No hardware camera or microphone was opened. Preserve successful and failed fixtures; the test source is capture.test.mjs.\n');
  const walk = dir => fs.readdirSync(dir, {withFileTypes:true}).flatMap(e => e.isDirectory() ? walk(path.join(dir,e.name)) : e.isSymbolicLink() ? [] : [path.join(dir,e.name)]);
  fs.writeFileSync(path.join(root, 'SHA256SUMS'), walk(root).sort().map(p => `${sha(p)}  ${path.relative(root,p)}`).join('\n')+'\n');
});
