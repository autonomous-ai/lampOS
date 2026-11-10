import assert from 'node:assert/strict';
import { test } from 'node:test';
import { differences } from '../conformance/conformance.js';

const row = (name, result, fields = []) => ({ name, result, fields, ms: 1 });

test('conformance: identical rows differ nowhere, and timing is not compared', () => {
  const rows = [row('get lamp', 'ok', ['lamp']), row('light green', 'ok', ['ok', 'to'])];
  assert.deepEqual(differences(rows, rows.map((r) => ({ ...r, ms: 900 }))), []);
});

test('conformance: a refusal, another code or other fields is a difference', () => {
  const left = [row('light green', 'ok', ['ok', 'to']), row('say', 'refused'), row('get body', 'ok', ['known'])];
  const right = [row('light green', 'refused'), row('say', 'bad_request'), row('get body', 'ok', ['joints', 'known'])];
  assert.equal(differences(left, right).length, 3);
});

test('conformance: a step listed as expected is not counted', () => {
  const left = [row('look under a lease', 'ok', ['how', 'why'])];
  const right = [row('look under a lease', 'ok', ['clamped', 'how', 'pitch', 'yaw'])];
  assert.deepEqual(differences(left, right), []);
});
