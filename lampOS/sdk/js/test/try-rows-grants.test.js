// Every row of the try-it table works or SAYS WHY: in lamp-sim mode, for a lamp that holds every
// grant and then each grant removed in turn, a row either runs with its defaults and answers, or is disabled with a reason.
// None is silent: no row is enabled and then refused for a grant it was said to have.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { WebSocket } from 'ws';
import { CONTROLS, checked, describe } from '../examples/twin/controls.js';
import { FakeLamp } from '../src/testing.js';

// The lamp's own refusals of a row run with its defaults, which the page says in its words.
// `play` needs the body alone at the door; with no skill pushed (a lamp that gave no `skills` grant cannot take one from this
// page) the lamp says which move it lacks, and the row's hint says to push the skill first.
const ANSWERS = { turn_end: /not this computer's turn/, play: /no move small-nod in your pushed skills/ };
// Rows that, pressed with their defaults on a fresh run, answer without a call to the lamp.
const ASKS_NOTHING = ['sound_remove'];
const ALL = ['body', 'light', 'speak', 'look', 'hear', 'sound', 'air', 'device', 'notes', 'camera', 'microphone', 'skills', 'speech', 'speaker'];

async function scenario(mode, granted) {
  const fake = await FakeLamp.start({ grants: granted });
  const lamp = await fake.connect();
  try {
    const table = describe(mode, { grants: lamp.grants });
    const silent = [];
    const stricter = [];
    const answered = [];
    for (const row of table.controls) {
      const control = CONTROLS.find((c) => c.id === row.id);
      if (row.disabled !== undefined) {
        if (typeof row.disabled !== 'string' || row.disabled.trim() === '') silent.push(`${row.id}: disabled with no reason`);
        // The other direction: a row drawn off for a grant must be one the lamp itself refuses for that grant. A row the
        // lamp would take (`play` needs the body alone at the door) may not be off here: no row is stricter than the lamp.
        // (A row that asks the lamp nothing with its defaults cannot show it: `sound_remove` with no clip kept answers by itself.)
        if (ASKS_NOTHING.includes(row.id)) continue;
        try {
          await control.run({ lamp, release: async () => { await lamp.following?.stop().catch(() => {}); }, show: () => {}, mode }, checked(control.fields, {}));
          stricter.push(`${row.id}: drawn off (${row.disabled}) but the lamp took it`);
        } catch (error) {
          if (!(error.code === 'no_grant' || /not granted/.test(error.message))) stricter.push(`${row.id}: drawn off (${row.disabled}) but the lamp refused it for another reason: ${error.message}`);
        }
        continue;
      }
      try {
        // `release` is the twin's: it also ends a hand-following (`follow_hand`) that still holds the body lease.
        await control.run({ lamp, release: async () => { await lamp.following?.stop().catch(() => {}); }, show: () => {}, mode }, checked(control.fields, {}));
        answered.push(row.id);
      } catch (error) {
        // A refusal for want of a grant is the silent failure this test is for; the lamp's other words (a turn that is not this
        // computer's to end) are its own answer, said on the page, and only those listed here.
        if (error.code === 'no_grant' || /not granted/.test(error.message)) silent.push(`${row.id}: enabled but refused for a grant: ${error.message}`);
        else if (!ANSWERS[row.id]?.test(error.message)) silent.push(`${row.id}: enabled but refused: ${error.message}`);
      }
    }
    return { table, silent, answered, stricter };
  } finally { await lamp.close(); await fake.stop(); }
}

for (const mode of ['lamp-sim']) {
  test(`${mode}: with every grant, each row runs with its defaults or says why it cannot`, async () => {
    const { silent, answered, table } = await scenario(mode, ALL);
    assert.deepEqual(silent, []);
    const disabled = table.controls.filter((c) => c.disabled).map((c) => c.id);
    assert.deepEqual(disabled, [], `no row is off with every grant (${mode})`);
    assert.ok(answered.length >= table.controls.length - disabled.length - Object.keys(ANSWERS).length, 'every other enabled row answered');
  });

  test(`${mode}: with each grant removed in turn, no row is both enabled and refused for want of a grant`, async () => {
    const results = await Promise.all(ALL.map(async (removed) => ({ removed, ...(await scenario(mode, ALL.filter((g) => g !== removed))) })));
    for (const { removed, silent, stricter, table } of results) {
      assert.deepEqual(silent, [], `without ${removed}`);
      assert.deepEqual(stricter, [], `without ${removed}: no row is stricter than the lamp`);
      // And the rows that needed it are the ones that say so.
      for (const row of table.controls) {
        if ((row.needs ?? []).includes(removed)) assert.match(row.disabled ?? '', new RegExp(`did not grant .*${removed}`), `${row.id} without ${removed}`);
      }
    }
  });
}

test('without grants the table is static, and the stimuli are never disabled', () => {
  assert.equal(describe('lamp-sim').controls.some((c) => c.disabled), false, 'without grants no row is off');
  const table = describe('lamp-sim', { grants: ['body'] });
  assert.ok(table.stimuli.length > 0 && table.stimuli.every((c) => c.disabled === undefined));
  assert.equal(table.controls.find((c) => c.id === 'say').disabled, 'the lamp did not grant speak');
});

// The server, not the page, is what refuses: a raw /live call of a row the table draws off gets no run at all.
test('the server refuses a raw call of a disabled row, and runs an enabled one', async (t) => {
  const folder = mkdtempSync(join(tmpdir(), 'try-grants-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const port = await new Promise((resolve) => { const p = createServer(); p.listen(0, '127.0.0.1', () => { const { port: n } = p.address(); p.close(() => resolve(n)); }); });
  const server = fileURLToPath(new URL('../examples/twin/server.js', import.meta.url));
  // The fake lamp grants no microphone.
  const child = spawn(process.execPath, [server, '--fake', '--port', String(port), '--glb', glb, '--stim-dir', folder, '--lamp-sim', '/bin/true'], { stdio: ['ignore', 'pipe', 'inherit'] });
  t.after(() => child.kill('SIGTERM'));
  const token = await new Promise((resolve, reject) => {
    let text = '';
    child.stdout.on('data', (chunk) => { text += chunk; const found = /\?t=(\S+)/.exec(text); if (found) resolve(found[1]); });
    child.on('close', (code) => reject(new Error(`the server ended (${code})`)));
  });
  const socket = new WebSocket(`ws://127.0.0.1:${port}/live?t=${token}`, { headers: { origin: `http://127.0.0.1:${port}` } });
  const got = [];
  socket.on('message', (data) => got.push(JSON.parse(data)));
  await new Promise((resolve, reject) => { socket.on('open', resolve); socket.on('error', reject); });
  t.after(() => socket.close());
  socket.send(JSON.stringify({ type: 'call', id: 1, name: 'microphone_stream', args: {} }));
  socket.send(JSON.stringify({ type: 'call', id: 2, name: 'language', args: {} }));
  for (let i = 0; i < 100 && got.filter((m) => m.type === 'result').length < 2; i++) await new Promise((r) => setTimeout(r, 50));
  const by = Object.fromEntries(got.filter((m) => m.type === 'result').map((m) => [m.id, m]));
  assert.equal(by[1].ok, false);
  assert.match(by[1].answer, /n\/a: the lamp did not grant microphone/);
  assert.equal(by[2].ok, true, 'an enabled row runs');
});
