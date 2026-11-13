// The try-it app's speech grant and its turn controls (#6276): its own file, so a train that appends to try.test.js
// cannot conflict with it.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CONTROLS, checked, logIt } from '../examples/twin/controls.js';
import { TRY_GRANTS } from '../examples/twin/virtual-lamp.js';

test('the try-it lamp has speech (the log shows lamp.said), never camera or microphone; every said line is logged, other events thinned', () => {
  const grants = TRY_GRANTS.split(',');
  assert.equal(grants.includes('speech'), true);
  assert.equal(grants.includes('camera'), false);
  assert.equal(grants.includes('microphone'), false);
  const quiet = new Map();
  assert.equal(logIt('lamp.said', 1000, quiet), true);
  assert.equal(logIt('lamp.said', 1001, quiet), true, 'two lines a millisecond apart are both shown');
  assert.equal(logIt('person.arrived', 1000, quiet), true);
  assert.equal(logIt('person.arrived', 1100, quiet), false, 'another event within 200 ms is thinned');
  assert.equal(logIt('person.arrived', 1250, quiet), true);
  // The turn controls are in the table, in the voice group, and answer with the lamp's words on the fake.
  const turn = CONTROLS.filter((c) => c.id.startsWith('turn_'));
  assert.deepEqual(turn.map((c) => c.id), ['turn_start', 'turn_end']);
  assert.throws(() => checked(turn[0].fields, { prompt: '' }), /prompt: 1 to 200 characters/);
});

test('every done is logged, however close to another: a refusing one must never be thinned away behind the one before it', () => {
  // lamp-75c9, 13 Nov: a done how=dropped why="refused: the lamp is speaking" is what says a turn did not run.
  const quiet = new Map();
  assert.equal(logIt('done', 1000, quiet), true);
  assert.equal(logIt('done', 1010, quiet), true, 'a done 10 ms after another is still shown');
  assert.equal(logIt('done', 1011, quiet), true);
  assert.equal(quiet.has('done'), false, 'and it leaves no thinning window behind');
  assert.equal(logIt('person.arrived', 1000, quiet), true);
  assert.equal(logIt('person.arrived', 1100, quiet), false, 'other events are still thinned');
});
