// The try-it checklist (examples/twin/checklist.js): its ticks, its key, storage that fails, the count, and an
// export that carries only the rows and the marks. Its own file, so a row PR never appends to try.test.js.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { exportChecklist, keyFor, openChecklist, rowsOf, tickable } from '../examples/twin/checklist.js';
import { describe } from '../examples/twin/controls.js';

const memory = () => { const data = new Map(); return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => data.set(k, String(v)) }; };
const rows = () => { const { controls, stimuli } = describe(); return [...controls.map((c) => ({ ...c, kind: 'call' })), ...stimuli.map((s) => ({ ...s, kind: 'stim' }))].map((r) => ({ id: `${r.kind}:${r.id}`, label: r.label, group: r.group, kind: r.kind })); };
const at = () => '2026-11-12T01:00:00.000Z';

test('the key is the lamp and the mode, so two lamps and two modes never share ticks', () => {
  assert.notEqual(keyFor('lamp-75c9', 'real'), keyFor('lamp-fake', 'real'));
  assert.notEqual(keyFor('lamp-75c9', 'real'), keyFor('lamp-75c9', 'lamp-sim'));
  assert.equal(keyFor('a/b c', 'real').includes(' '), false);
  const storage = memory();
  const ids = rows().map((r) => r.id);
  const one = openChecklist({ storage, key: keyFor('lamp-75c9', 'real'), rowIds: ids, now: at });
  one.set('call:gesture', { result: 'pass', note: ' nodded ' });
  const other = openChecklist({ storage, key: keyFor('lamp-fake', 'real'), rowIds: ids, now: at });
  assert.equal(other.get('call:gesture'), undefined, 'another lamp has no ticks');
  const again = openChecklist({ storage, key: keyFor('lamp-75c9', 'real'), rowIds: ids, now: at });
  assert.deepEqual(again.get('call:gesture'), { result: 'pass', note: 'nodded', at: at() }, 'the same lamp keeps them across a reload');
});

test('a row the lamp or the run cannot do is drawn, n/a with its reason, not tickable, and not counted', () => {
  const table = {
    controls: [{ id: 'gesture', label: 'Gesture', group: 'Body' }, { id: 'camera_snapshot', label: 'Camera', group: 'C', disabled: 'the lamp did not grant camera' }, { id: 'microphone_stream', label: 'Mic', group: 'C', disabled: 'start the run with --microphone' }],
    stimuli: [{ id: 'arrive', label: 'Arrives', group: 'People' }],
  };
  const shown = rowsOf(table);
  assert.deepEqual(shown.map((r) => r.id), ['call:gesture', 'call:camera_snapshot', 'call:microphone_stream', 'stim:arrive'], 'every row is a row of the page');
  assert.deepEqual(tickable(shown), ['call:gesture', 'stim:arrive']);
  const list = openChecklist({ storage: memory(), key: 'k', rowIds: tickable(shown), now: at });
  list.set('call:gesture', { result: 'pass' });
  assert.deepEqual(list.count(), { ticked: 1, of: 2 }, 'N counts the enabled rows');
  assert.throws(() => list.set('call:camera_snapshot', { result: 'pass' }), /no row/, 'a disabled row cannot be ticked');
  const out = exportChecklist({ lamp: { id: 'l', name: 'L' }, mode: 'real', rows: shown, marks: list.marks(), now: at });
  const json = JSON.parse(out.json);
  assert.deepEqual(json.rows.find((r) => r.row === 'call:camera_snapshot'), { row: 'call:camera_snapshot', label: 'Camera', group: 'C', kind: 'call', result: 'n/a', note: 'the lamp did not grant camera', time: '' });
  assert.match(out.markdown, /\| call:microphone_stream \| n\/a \| start the run with --microphone \| {2}\|/);
  assert.match(out.markdown, /1 of 2 ticked, 2 n\/a/);
});

test('n of N counts rows with a result; a note alone is not a tick; a result pressed again clears', () => {
  const ids = rows().map((r) => r.id);
  const list = openChecklist({ storage: memory(), key: 'k', rowIds: ids, now: at });
  assert.deepEqual(list.count(), { ticked: 0, of: ids.length });
  list.set('call:say', { result: 'fail', note: 'no sound' });
  list.set('stim:hear', { result: null, note: 'only a note' });
  assert.deepEqual(list.count(), { ticked: 1, of: ids.length });
  list.set('call:say', { result: null, note: '' });
  assert.equal(list.get('call:say'), undefined);
  assert.throws(() => list.set('call:nope', { result: 'pass' }), /no row/);
  assert.equal(list.get('stim:hear').note.length > 0, true);
});

test('storage that is missing, throws or holds garbage gives a working list for this page', () => {
  const ids = rows().map((r) => r.id);
  const broken = { getItem() { throw new Error('blocked'); }, setItem() { throw new Error('quota'); } };
  const list = openChecklist({ storage: broken, key: 'k', rowIds: ids, now: at });
  list.set('call:say', { result: 'pass' });
  assert.deepEqual(list.count(), { ticked: 1, of: ids.length }, 'held in memory');
  assert.doesNotThrow(() => openChecklist({ storage: null, key: 'k', rowIds: ids }));
  const storage = memory();
  storage.setItem('k', '{not json');
  assert.deepEqual(openChecklist({ storage, key: 'k', rowIds: ids }).count(), { ticked: 0, of: ids.length });
  // A stored row that is no longer in the table, or a result that is not pass or fail, is dropped.
  storage.setItem('k', JSON.stringify({ 'call:gone': { result: 'pass' }, 'call:say': { result: 'maybe', note: 'n' } }));
  const kept = openChecklist({ storage, key: 'k', rowIds: ids });
  assert.equal(kept.get('call:gone'), undefined);
  assert.deepEqual(kept.get('call:say'), { note: 'n' });
});

test('the export is JSON and a markdown table of the rows and marks, and holds no token, code, pin or keyring', () => {
  const ids = rows();
  const list = openChecklist({ storage: memory(), key: 'k', rowIds: ids.map((r) => r.id), now: at });
  list.set('call:gesture', { result: 'pass', note: 'nodded | twice' });
  list.set('call:say', { result: 'fail' });
  const secrets = ['TOKEN-abc123_XYZ', '482913', 'PIN-7788', '/home/user/.lampos-try-real-4242'];
  // Secrets live on the page (its URL) and in the process; the export function is given neither, and a note the
  // person types is theirs. Prove the function takes only what it is given: nothing else appears.
  // Even a lamp object that carries them (a pairing, a keyring path) exports its id and name only.
  const lamp = { id: 'lamp-75c9', name: 'Lamp 75c9', token: secrets[0], code: secrets[1], pin: secrets[2], keyring: secrets[3] };
  const out = exportChecklist({ lamp, mode: 'real', rows: ids, marks: list.marks(), now: at });
  for (const secret of secrets) { assert.equal(out.json.includes(secret), false, secret); assert.equal(out.markdown.includes(secret), false, secret); }
  const json = JSON.parse(out.json);
  assert.deepEqual(Object.keys(json), ['lamp', 'mode', 'exported_at', 'rows']);
  assert.equal(json.rows.length, ids.length);
  assert.deepEqual(json.rows.find((r) => r.row === 'call:gesture'), { row: 'call:gesture', label: 'Gesture', group: 'Body', kind: 'call', result: 'pass', note: 'nodded | twice', time: at() });
  assert.equal(json.rows.find((r) => r.row === 'stim:arrive').result, 'not run');
  assert.match(out.markdown, /^# Try-it checklist: Lamp 75c9 \(lamp-75c9, real\)/);
  assert.match(out.markdown, /\| call:gesture \| pass \| nodded \\\| twice \| 2026-11-12T01:00:00\.000Z \|/);
  assert.match(out.markdown, /2 of \d+ ticked, 0 n\/a/);
  assert.equal(out.markdown.split('\n').filter((l) => l.startsWith('| call:')).length, ids.filter((r) => r.kind === 'call').length);
});

test('the served page asks for the checklist with its token, and the table names the lamp and the mode', async (t) => {
  const { mkdtempSync, rmSync, writeFileSync, chmodSync } = await import('node:fs');
  const { spawn } = await import('node:child_process');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const { createServer } = await import('node:net');
  const server = new URL('../examples/twin/server.js', import.meta.url).pathname;
  const folder = mkdtempSync(join(tmpdir(), 'try-checklist-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const stub = join(folder, 'lamp-sim');
  writeFileSync(stub, '#!/bin/sh\nexit 0\n');
  chmodSync(stub, 0o755);
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const port = await new Promise((resolve) => { const s = createServer().listen(0, '127.0.0.1', () => { const { port: p } = s.address(); s.close(() => resolve(p)); }); });
  const child = spawn(process.execPath, [server, '--fake', '--port', String(port), '--glb', glb, '--stim-dir', folder, '--lamp-sim', stub], { stdio: ['ignore', 'pipe', 'inherit'] });
  t.after(() => child.kill('SIGTERM'));
  const token = await new Promise((resolve, reject) => {
    let text = '';
    child.stdout.on('data', (chunk) => { text += chunk; const found = /\?t=(\S+)/.exec(text); if (found) resolve(found[1]); });
    child.on('close', (code) => reject(new Error(`the server ended (${code})`)));
  });
  const base = `http://127.0.0.1:${port}`;
  assert.equal((await fetch(`${base}/checklist.js`)).status, 403, 'not without the token');
  assert.equal((await fetch(`${base}/checklist.js?t=${token}`)).status, 200);
  const table = await (await fetch(`${base}/try.json?t=${token}`)).json();
  assert.deepEqual(table.lamp, { id: 'lamp-fake', name: 'Fake lamp (twin)' });
  assert.equal(table.mode, 'fake');
  const panel = await (await fetch(`${base}/panel.js?t=${token}`)).text();
  assert.match(panel, /import\(`\/checklist\.js\$\{location\.search\}`\)/, 'asked for with the page\'s own query');
});
