// The try-it app covers the control protocol: every op and event of control.schema.json is named by an entry of
// examples/twin/controls.js (`ops` for an op; `shows` for an event, on a control or a stimulus) or listed, with its
// reason, in examples/twin/controls-exceptions.js. A new op in the schema fails this until it has a row or a line.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CONTROLS, STIMULI } from '../examples/twin/controls.js';
import { COVERAGE_EXCEPTIONS } from '../examples/twin/controls-exceptions.js';
import { events, ops } from '../src/schema.js';

// What is missing, as sentences naming the cause: pure, so the test can run it on a schema with an op added.
export function gaps({ ops, events, controls, stimuli, exceptions }) {
  const named = { op: new Set(controls.flatMap((c) => c.ops ?? [])), event: new Set([...controls, ...stimuli].flatMap((e) => e.shows ?? [])) };
  const listed = { op: new Set(), event: new Set() };
  const problems = [];
  for (const { kind, name, reason } of exceptions) {
    if (!listed[kind]) { problems.push(`exception ${name}: kind ${kind} is not op or event`); continue; }
    if (listed[kind].has(name)) problems.push(`exception ${kind} ${name} is listed twice`);
    listed[kind].add(name);
    if (typeof reason !== 'string' || reason.trim() === '') problems.push(`exception ${kind} ${name} has no reason`);
    if (!(kind === 'op' ? ops : events).includes(name)) problems.push(`exception ${kind} ${name} is not in control.schema.json: delete the line`);
    if (named[kind].has(name)) problems.push(`${kind} ${name} has a row now: delete its line in controls-exceptions.js`);
  }
  for (const [kind, all] of [['op', ops], ['event', events]]) {
    for (const name of all) {
      if (!named[kind].has(name) && !listed[kind].has(name)) {
        problems.push(`${kind} ${name} has no row: add an entry to controls.js with ${kind === 'op' ? `ops: ['${name}']` : `shows: ['${name}']`}, or a line with its reason to controls-exceptions.js`);
      }
    }
  }
  const known = new Set([...ops, ...events]);
  for (const entry of [...controls, ...stimuli]) {
    for (const name of [...(entry.ops ?? []), ...(entry.shows ?? [])]) {
      if (!known.has(name)) problems.push(`entry ${entry.id} names ${name}, which is not an op or event of control.schema.json`);
    }
  }
  return problems;
}

const now = { ops, events, controls: CONTROLS, stimuli: STIMULI, exceptions: COVERAGE_EXCEPTIONS };

test('every op and event of the schema has a row or a named exception', () => {
  assert.deepEqual(gaps(now), []);
});

test('a new op or event in the schema fails the guard, saying what to do', () => {
  assert.match(gaps({ ...now, ops: [...ops, 'volume.sweep'] }).join('\n'), /op volume\.sweep has no row: add an entry to controls\.js with ops: \['volume\.sweep'\]/);
  assert.match(gaps({ ...now, events: [...events, 'lamp.tilted'] }).join('\n'), /event lamp\.tilted has no row/);
});

test('an exception goes when its row is written, and one needs a real name and a reason', () => {
  // An op that still has its line (listen.set: its row is not written yet).
  const row = { id: 'listen', ops: ['listen.set'], shows: [] };
  assert.match(gaps({ ...now, controls: [...CONTROLS, row] }).join('\n'), /op listen\.set has a row now: delete its line/);
  assert.match(gaps({ ...now, exceptions: [...COVERAGE_EXCEPTIONS, { kind: 'op', name: 'no.such', reason: 'x' }] }).join('\n'), /not in control\.schema\.json/);
  assert.match(gaps({ ...now, exceptions: [...COVERAGE_EXCEPTIONS, { kind: 'op', name: 'pair', reason: '' }] }).join('\n'), /listed twice[\s\S]*no reason|no reason[\s\S]*listed twice/);
  assert.match(gaps({ ...now, controls: [...CONTROLS, { id: 'typo', ops: ['gestur'], shows: [] }] }).join('\n'), /entry typo names gestur/);
});
