import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { buildSchedule, createPlan, consentTemplate, sha256 } from './schedule.mjs';
const p = JSON.parse(fs.readFileSync(new URL('./protocol.json', import.meta.url)));
test('planned class/split denominators and pair playback match', () => {
  const rows = buildSchedule(p);
  assert.equal(rows.length, 240);
  for (const split of ['tune', 'test']) for (const c of ['A', 'B']) {
    assert.equal(rows.filter(r => r.split === split && r.intended_class === c).length, 60);
  }
  const groups = Map.groupBy(rows, r => r.pair_id);
  for (const pair of groups.values()) {
    assert.equal(pair.length, 2);
    assert.deepEqual(new Set(pair.map(r => r.intended_class)), new Set(['A', 'B']));
    assert.equal(pair[0].playback_speaker, pair[1].playback_speaker);
    assert.equal(pair[0].playback_start_us, pair[1].playback_start_us);
  }
});
test('no person or source crosses split; changed split/conditions refuse', () => {
  for (const row of buildSchedule(p)) assert.equal(p.participants.find(x => x.alias === row.playback_speaker).split, row.split);
  for (const mutate of [q => q.participants[0].split = 'test', q => q.participants[0].playback_speaker = 'P03',
    q => q.clip_seconds = 11, q => q.distances_m[0] = 0.7, q => q.decision_midpoint_us = 4000000]) {
    const changed = structuredClone(p); mutate(changed); assert.throws(() => buildSchedule(changed), /frozen/);
  }
});
test('consent template defaults to no and binds only valid alias', () => {
  assert.match(consentTemplate('P01'), /^Participant alias: P01\nConsent accepted: no\n/);
  assert.throws(() => consentTemplate('P05'));
});
test('plan preserves earlier files and hashes unrecorded output', () => {
  const parent = fs.mkdtempSync(path.join(os.tmpdir(), 'sensor5-schedule-test-'));
  const dir = path.join(parent, 'plan');
  const result = createPlan(dir, new URL('./protocol.json', import.meta.url));
  assert.equal(result.recorded, false);
  assert.equal(result.clips, 240);
  assert.throws(() => createPlan(dir, new URL('./protocol.json', import.meta.url)), /exist/);
  for (const line of fs.readFileSync(path.join(dir, 'SHA256SUMS'), 'utf8').trim().split('\n')) {
    const [digest, name] = line.split('  '); assert.equal(sha256(fs.readFileSync(path.join(dir, name))), digest);
  }
  assert.equal(fs.statSync(dir).mode & 0o777, 0o700);
});
