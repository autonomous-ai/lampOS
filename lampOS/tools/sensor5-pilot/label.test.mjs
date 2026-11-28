import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
const require = createRequire(import.meta.url);
const api = require('./label.js');
const digest = n => n.toString(16).padStart(64, '0');
const manifest = () => ({ schema_version: 1, clip_id: 'P01-D060-daylight-A01', capture_sha256: digest(1),
  audio_file: 'audio.wav', audio_sha256: digest(2), audio_start_us: 1080000,
  frames: [1000000, 1040000, 1120000, 1160000, 1240000].map((t, i) => ({ index: i, timestamp_us: t,
    image_file: `frames/${String(i).padStart(6, '0')}.png`, sha256: digest(10 + i), width_px: 640, height_px: 360 })) });
const project = () => api.createProject(manifest(), 'R01', digest(3));
const face = (id = 'T01', speaking = 'speaking_audible') => ({ track_id: id, visibility: 'visible', speaking, box: [.1, .2, .3, .4] });
const row = (p, i = 0, changes = {}) => ({ frame_index: i, timestamp_us: p.manifest.frames[i].timestamp_us,
  scene: 'faces', faces: [face()], audible_sources: ['visible', 'playback'], ...changes });

function estimatedManifest() {
  const m = manifest();
  m.alignment = { status: 'unverified_estimate', camera_boundary: 'dequeue_completed_at_us', audio_boundary: 'read_completion_minus_20000_us' };
  m.scoring_allowed = false;
  m.physical_sync_validated = false;
  m.timestamp_boundary = 'camera dequeue completion estimate';
  m.frames.forEach((f, i) => {
    f.original_index = 100 + i * 3;
    f.source_record_index = 200 + i * 5;
    f.native_frame = { frame_sequence: f.original_index, dequeue_completed_at_us: f.timestamp_us,
      driver_timestamp: { micros: f.timestamp_us - 1000, domain: 'host_monotonic', point: 'end_of_frame' } };
  });
  return m;
}

test('estimated preview keeps its no-scoring flag, native timing and original joins through export/import', () => {
  const p = api.createProject(estimatedManifest(), 'R01', digest(3));
  const changed = api.applyRange(p, 1, 3, { ...p.rows[0], scene: 'no_face', audible_sources: ['none'] });
  const exported = api.exportProject(changed);
  assert.equal(exported.scoring_allowed, false);
  assert.equal(exported.alignment.status, 'unverified_estimate');
  assert.equal(exported.timestamp_boundary, 'camera dequeue completion estimate');
  assert.equal(exported.frames[2].original_index, 106);
  assert.equal(exported.frames[2].source_record_index, 210);
  assert.equal(exported.frames[2].native_frame.driver_timestamp.micros, 1119000);
  assert.equal(exported.frames[2].timestamp_us, 1120000);
  assert.equal(api.importProject(p, exported).rows[2].original_index, 106);
  assert.match(api.alignmentNotice(p.manifest), /UNVERIFIED.*not eligible for scoring/);
});

test('estimated labels cannot lose or change source joins and alignment eligibility on import', () => {
  const p = api.createProject(estimatedManifest(), 'R01', digest(3));
  for (const change of [
    v => { v.scoring_allowed = true; }, v => { delete v.alignment; },
    v => { delete v.frames[0].original_index; }, v => { v.frames[0].source_record_index += 1; },
    v => { v.frames[0].native_frame.driver_timestamp.micros += 1; },
  ]) { const output = api.exportProject(p); change(output); assert.throws(() => api.importProject(p, output)); }
});

test('unverified preview manifest refuses scoring and inconsistent original identities', () => {
  for (const change of [
    m => { m.scoring_allowed = true; }, m => { delete m.scoring_allowed; },
    m => { m.physical_sync_validated = true; }, m => { delete m.timestamp_boundary; },
    m => { m.frames[2].original_index = m.frames[1].original_index; },
    m => { m.frames[0].source_record_index = -1; },
  ]) { const m = estimatedManifest(); change(m); assert.throws(() => api.validateManifest(m)); }
});

test('every native frame begins unlabelled; nonuniform timestamps are retained', () => {
  const p = project();
  assert.deepEqual(p.rows.map(x => x.timestamp_us), [1000000, 1040000, 1120000, 1160000, 1240000]);
  assert.equal(p.rows.length, 5);
  assert(p.rows.every(x => x.scene === 'unlabelled' && !x.faces.length && !x.audible_sources.length));
  assert.deepEqual(api.summary(p), { frames: 5, reviewed: 0, unlabelled: 5, uncertain: 0, unknown: 5 });
});

test('manifest rejects missing hashes, reordered/duplicate timestamps and unsafe paths', () => {
  for (const mutate of [
    m => { delete m.audio_sha256; }, m => { m.capture_sha256 = 'wrong'; },
    m => { m.frames[1].timestamp_us = m.frames[0].timestamp_us; }, m => { m.frames[1].index = 4; },
    m => { m.frames[0].image_file = '../face.png'; }, m => { m.audio_file = 'https://example.test/a.wav'; },
    m => { m.frames[0].sha256 = ''; }, m => { m.frames[1].image_file = m.frames[0].image_file; },
    m => { m.audio_start_us = 1.25; }, m => { m.frames[0].timestamp_us = Number.MAX_SAFE_INTEGER + 1; },
    m => { m.clip_id = '<script>'; },
  ]) {
    const m = manifest(); mutate(m); assert.throws(() => api.validateManifest(m));
  }
});

test('audio mapping preserves source offset, including video before audio', () => {
  assert.equal(api.audioTime(1000000, 1080000), -.08);
  assert.equal(api.audioTime(1120000, 1080000), .04);
  assert.equal(api.audioTime(-200000, -250000), .05);
});

test('playback selects the latest native frame rather than fabricating CFR indices', () => {
  const f = manifest().frames;
  assert.equal(api.frameAtTime(f, 999999), 0);
  assert.equal(api.frameAtTime(f, 1119999), 1);
  assert.equal(api.frameAtTime(f, 1120000), 2);
  assert.equal(api.frameAtTime(f, 1240001), 4);
  assert.throws(() => api.frameAtTime(f, NaN));
});

test('single-frame assignment does not infer adjacent labels or mutate original state', () => {
  const p = project(), next = api.applyRange(p, 2, 2, row(p));
  assert.equal(p.rows[2].scene, 'unlabelled');
  assert.deepEqual(next.rows.map(x => x.scene), ['unlabelled', 'unlabelled', 'faces', 'unlabelled', 'unlabelled']);
  assert.equal(next.rows[2].timestamp_us, 1120000);
  assert.equal(next.rows[2].frame_index, 2);
});

test('explicit ranges are inclusive and each copied box is independent', () => {
  const p = project(), next = api.applyRange(p, 1, 3, row(p));
  assert.deepEqual(next.rows.map(x => x.scene), ['unlabelled', 'faces', 'faces', 'faces', 'unlabelled']);
  next.rows[1].faces[0].box[0] = .5;
  assert.equal(next.rows[2].faces[0].box[0], .1);
  assert.deepEqual(next.rows.map(x => x.timestamp_us), p.rows.map(x => x.timestamp_us));
});

test('range rejects invalid bounds and existing labels unless overwrite is explicit', () => {
  const p = project();
  for (const bounds of [[-1, 1], [0, 5], [3, 2], [1.1, 2], [0, Infinity]]) assert.throws(() => api.applyRange(p, ...bounds, row(p)));
  const labelled = api.applyRange(p, 2, 2, row(p));
  assert.throws(() => api.applyRange(labelled, 0, 4, row(p)));
  assert.equal(api.summary(api.applyRange(labelled, 0, 4, row(p), true)).reviewed, 5);
  assert.equal(labelled.rows[0].scene, 'unlabelled');
});

test('multiple stable tracks and mixed visible/playback speech are represented', () => {
  const p = project(), r = row(p, 0, { faces: [face('T01'), face('T02', 'not_speaking')] });
  const next = api.applyRange(p, 0, 1, r);
  assert.deepEqual(next.rows[1].faces.map(x => x.track_id), ['T01', 'T02']);
  assert.deepEqual(next.rows[1].audible_sources, ['visible', 'playback']);
});

test('silent visible face with off-screen speech is different from no speech', () => {
  const p = project();
  const b = row(p, 0, { faces: [face('T01', 'not_speaking')], audible_sources: ['playback'] });
  assert.equal(api.validateRow(b, p.manifest.frames[0]).faces[0].speaking, 'not_speaking');
  assert.deepEqual(api.validateRow({ ...b, audible_sources: ['none'] }, p.manifest.frames[0]).audible_sources, ['none']);
});

test('inaudible speech, occlusion and out-of-view tracks survive export', () => {
  const p = project();
  const r = row(p, 0, { faces: [{ ...face(), visibility: 'occluded', speaking: 'speaking_inaudible' },
    { track_id: 'T02', visibility: 'out_of_view', box: null, speaking: 'uncertain' }], audible_sources: ['playback'] });
  const next = api.applyRange(p, 0, 0, r);
  const exported = api.exportProject(next);
  assert.equal(exported.frames[0].faces[0].speaking, 'speaking_inaudible');
  assert.equal(exported.frames[0].faces[1].visibility, 'out_of_view');
  assert.equal(exported.summary.uncertain, 1);
});

test('explicit no-face preserves absent track identity and is not an unlabelled frame', () => {
  const p = project();
  const r = row(p, 0, { scene: 'no_face', faces: [{ track_id: 'T01', visibility: 'out_of_view', speaking: 'not_speaking', box: null }], audible_sources: ['playback'] });
  const next = api.applyRange(p, 0, 0, r);
  assert.equal(api.summary(next).reviewed, 1);
  assert.equal(api.summary(next).unknown, 4);
  assert.equal(next.rows[0].faces[0].track_id, 'T01');
});

test('contradictory scenes, sources, tracks and invalid boxes are rejected', () => {
  const p = project(), f = p.manifest.frames[0];
  for (const changes of [
    { scene: 'no_face' }, { scene: 'unlabelled' }, { audible_sources: [] }, { audible_sources: ['none', 'visible'] },
    { audible_sources: ['playback'] }, { faces: [face('T01'), face('T01')] },
    { faces: [{ ...face(), box: [0, 0, 0, .5] }] }, { faces: [{ ...face(), box: [.8, 0, .4, .5] }] },
    { faces: [{ ...face(), box: null }] }, { faces: [{ ...face(), visibility: 'out_of_view' }] },
    { faces: [face('person_name')] }, { faces: [face('T01', 'not_speaking')] },
  ]) assert.throws(() => api.validateRow(row(p, 0, changes), f));
});

test('unknown scene/source/speaking labels remain explicit unknowns', () => {
  const p = project(), r = row(p, 0, { scene: 'uncertain', faces: [face('T01', 'uncertain')], audible_sources: ['uncertain'] });
  const next = api.applyRange(p, 0, 0, r);
  assert.deepEqual(api.summary(next), { frames: 5, reviewed: 1, unlabelled: 4, uncertain: 1, unknown: 5 });
});

test('JSON round-trip retains all frame identities, unknowns and rater', () => {
  const p = project(), next = api.applyRange(p, 1, 3, row(p));
  const output = api.exportProject(next, '2026-11-28T07:00:00.000Z');
  const loaded = api.importProject(p, JSON.parse(JSON.stringify(output)));
  assert.deepEqual(loaded, next);
});

test('import rejects different capture, clip, manifest, audio, images, frame timing or count', () => {
  const p = project();
  for (const mutate of [
    x => { x.capture_sha256 = digest(99); }, x => { x.clip_id = 'other'; }, x => { x.prepared_manifest_sha256 = digest(99); },
    x => { x.audio_start_us += 1; }, x => { x.audio_sha256 = digest(99); }, x => { x.image_sha256[0] = digest(99); },
    x => { x.frames[0].timestamp_us += 1; }, x => { x.frames[1].frame_index = 0; }, x => { x.frames.pop(); },
    x => { x.rater_id = 'not a pseudonym'; }, x => { x.summary.unknown = 0; },
  ]) { const exported = api.exportProject(p); mutate(exported); assert.throws(() => api.importProject(p, exported)); }
});

test('clear affects exactly one frame and preserves adjacent annotations', () => {
  const p = project(), labelled = api.applyRange(p, 0, 4, row(p)), next = api.clearFrame(labelled, 2);
  assert.deepEqual(next.rows.map(x => x.scene), ['faces', 'faces', 'unlabelled', 'faces', 'faces']);
  assert.equal(labelled.rows[2].scene, 'faces');
  assert.throws(() => api.clearFrame(p, 5));
});

test('local HTML has every referenced control and a network-denying CSP', () => {
  const html = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
  const js = readFileSync(new URL('./label.js', import.meta.url), 'utf8');
  const ids = new Set([...html.matchAll(/\bid="([^"]+)"/g)].map(x => x[1]));
  for (const [, id] of js.matchAll(/\$\('([^']+)'\)/g)) assert(ids.has(id), `Missing control ${id}`);
  assert(html.includes("connect-src 'none'"));
  assert(html.includes("default-src 'none'"));
  assert(!/\b(fetch|XMLHttpRequest|WebSocket|sendBeacon|localStorage|sessionStorage|indexedDB)\s*[(.]/.test(js));
  assert(!/<(?:script|link)[^>]+https?:/i.test(html));
});
