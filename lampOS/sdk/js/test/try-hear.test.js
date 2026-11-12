// The try-it page's sound (F3, "Hear the lamp in this page"): the server makes the sound for a page that turned the switch on and
// for that page alone, the hum is a labelled stand-in, a clip plays its own audio, and no microphone byte has a way to a page. Its own file, so a row PR never appends to try.test.js.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { WebSocket } from 'ws';
import { schema } from 'lampos';
import { FakeLamp } from '../src/testing.js';
import { CONTROLS, checked, humWav, quietTone } from '../examples/twin/controls.js';

const serverJs = new URL('../examples/twin/server.js', import.meta.url).pathname;
const freePort = () => new Promise((resolve) => { const s = createServer().listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => resolve(port)); }); });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// A twin to talk to, on its own fake lamp with the try-it panel.
async function page(t) {
  const folder = mkdtempSync(join(tmpdir(), 'try-hear-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const glb = join(folder, 'lamp.glb');
  writeFileSync(glb, Buffer.from('glTF\x02\x00\x00\x00\x0c\x00\x00\x00'));
  const stub = join(folder, 'lamp-sim');
  writeFileSync(stub, '#!/bin/sh\nexit 0\n');
  chmodSync(stub, 0o755);
  const twinPort = await freePort();
  const [file, args, env] = [serverJs, ['--fake', '--port', String(twinPort), '--glb', glb, '--stim-dir', folder, '--lamp-sim', stub], process.env];
  const child = spawn(process.execPath, [file, ...args], { env, stdio: ['ignore', 'pipe', 'inherit'] });
  let text = '';
  child.stdout.on('data', (c) => { text += c; });
  const ended = new Promise((resolve) => child.on('close', resolve));
  t.after(async () => { child.kill('SIGTERM'); await Promise.race([ended, sleep(10000)]); child.kill('SIGKILL'); });
  const token = await new Promise((resolve, reject) => {
    const timer = setInterval(() => { const m = /\?t=(\S+)/.exec(text); if (m) { clearInterval(timer); resolve(m[1]); } }, 50);
    ended.then((code) => reject(new Error(`ended ${code}`)));
  });
  return { url: `http://127.0.0.1:${twinPort}/?t=${token}`, base: `http://127.0.0.1:${twinPort}`, token, twinPort };
}

// One page's socket: every message kept, a press answered by its result.
async function visitor(site) {
  const socket = new WebSocket(`ws://127.0.0.1:${site.twinPort}/live?t=${site.token}`, { headers: { origin: site.base } });
  const messages = [];
  socket.on('message', (data) => { try { messages.push(JSON.parse(data)); } catch { /* not ours */ } });
  await new Promise((resolve, reject) => { socket.once('open', resolve); socket.once('error', reject); });
  let id = 0;
  const waitFor = async (test, ms = 5000) => {
    for (const until = Date.now() + ms; Date.now() < until; await sleep(20)) { const found = messages.find(test); if (found) return found; }
    throw new Error('not seen in time');
  };
  return {
    messages,
    close: () => socket.close(),
    hear: async (on) => { socket.send(JSON.stringify({ type: 'hear', on })); return waitFor((m) => m.type === 'hear'); },
    press: async (name, args = {}) => {
      id += 1;
      const mine = id;
      socket.send(JSON.stringify({ type: 'call', id: mine, name, args }));
      return waitFor((m) => m.type === 'result' && m.id === mine);
    },
    audio: () => messages.filter((m) => m.type === 'audio'),
  };
}

test('the hum is a quiet stand-in the voice\'s caps take: 0.6 s of two low tones, a valid WAV, peak and RMS inside the clip\'s, faded at both ends', () => {
  const wav = humWav();
  assert.equal(wav.subarray(0, 4).toString('ascii'), 'RIFF');
  assert.equal(wav.subarray(8, 12).toString('ascii'), 'WAVE');
  assert.equal(wav.readUInt32LE(24), 24_000);
  assert.equal(wav.readUInt32LE(40), 14_400 * 2, '0.6 s');
  let peak = 0;
  let worst = 0;
  const samples = (wav.length - 44) / 2;
  for (let start = 0; start + 240 <= samples; start += 240) {
    let sum = 0;
    for (let k = start; k < start + 240; k += 1) { const v = wav.readInt16LE(44 + k * 2); peak = Math.max(peak, Math.abs(v)); sum += v * v; }
    worst = Math.max(worst, Math.sqrt(sum / 240));
  }
  assert.ok(peak <= 1000, `peak ${peak} at most the clip's 1000`);
  assert.ok(worst < 1036, `10 ms RMS ${Math.round(worst)} under -30 dBFS`);
  assert.equal(wav.readInt16LE(44), 0, 'it starts at silence');
  assert.equal(wav.readInt16LE(44 + (samples - 1) * 2), 0, 'and ends at silence');
});

test('the sound rows ask the page for the sound and say what was sent; with no page to ask they say nothing new', async (t) => {
  const fake = await FakeLamp.start({ grants: ['speak'] });
  const lamp = await fake.connect();
  t.after(async () => { await lamp.close(); await fake.stop(); });
  const row = (id, values = {}) => CONTROLS.find((c) => c.id === id);
  const run = (id, audio, values = {}) => { const c = row(id); return c.run({ lamp, release: async () => {}, audio }, checked(c.fields, values)); };
  const sent = [];
  const answer = await run('sound', (what, wav) => { sent.push([what, wav]); return wav.length; });
  assert.equal(sent[0][0], 'hum');
  assert.ok(sent[0][1].equals(humWav()));
  assert.equal(answer, `lamp.sound('hum')  // audio: ${humWav().length.toLocaleString('en-US')} bytes to this page (a stand-in for the lamp's hum)`);
  assert.match(await run('sound', () => 0), /^lamp\.sound\('hum'\)  \/\/ no sound sent: turn on 'Hear the lamp in this page'$/);
  assert.equal(await run('sound'), "lamp.sound('hum')", 'no page, no note');
  // The clip: the very WAV the row kept on the lamp.
  sent.length = 0;
  const kept = await run('sound_upload', (what, wav) => { sent.push([what, wav]); return wav.length; }, { ms: 300 });
  assert.equal(sent[0][0], 'clip');
  assert.ok(sent[0][1].equals(quietTone(300)));
  assert.match(kept, /a 300 ms quiet tone\) -> \{.*\}  \/\/ audio: 14,444 bytes to this page \(the clip saved on the lamp\)$/);
  await run('sound_remove');
  // The labels the lead fixed, each at most 200 characters.
  assert.equal(row('say').hint, "spoken by your browser (the lamp's real voice sounds different)");
  assert.match(row('sound').hint, /^a stand-in for the lamp's hum/);
  assert.match(row('turn_start').hint, /^the virtual lamp has no model, so it gives no spoken answer: the log says done; a real lamp answers aloud$/);
  for (const c of CONTROLS) for (const key of ['hint', 'hintReal']) assert.ok(!c[key] || c[key].length <= 200, `${c.id} ${key}`);
});

test('a page with "Hear the lamp in this page" on gets the sound it pressed, a page with it off gets none, and off is the default', async (t) => {
  const site = await page(t);
  const table = await (await fetch(`${site.base}/try.json?t=${site.token}`)).json();
  assert.notEqual(table.mode, 'real');
  const listener = await visitor(site);
  const silent = await visitor(site);
  t.after(() => { listener.close(); silent.close(); });
  // Off on every connection: a press before the switch sends nothing, and says so.
  const before = await listener.press('sound');
  assert.equal(listener.audio().length, 0);
  assert.match(before.call, /no sound sent: turn on 'Hear the lamp in this page'/);
  const answered = await listener.hear(true);
  assert.deepEqual([answered.on, answered.language], [true, 'en'], 'on, and the lamp\'s language for the browser\'s voice');
  const hum = await listener.press('sound');
  const [audio] = listener.audio();
  assert.equal(audio.what, 'hum');
  assert.equal(audio.id, hum.id, 'to the row that asked');
  const wav = Buffer.from(audio.wav, 'base64');
  assert.ok(wav.equals(humWav()));
  assert.match(hum.call, new RegExp(`^lamp\\.sound\\('hum'\\)  // audio: ${wav.length.toLocaleString('en-US')} bytes to this page \\(a stand-in`));
  // A second page that did not turn it on: its own press brings none, and the first page's press brought it none.
  const mute = await silent.press('sound');
  assert.equal(silent.audio().length, 0);
  assert.match(mute.call, /no sound sent/);
  assert.equal(listener.audio().length, 1, 'a press on another page is not sent here');
  // The clip is its own audio.
  await listener.press('sound_upload', { ms: 200 });
  const clip = listener.audio().find((a) => a.what === 'clip');
  assert.ok(Buffer.from(clip.wav, 'base64').equals(quietTone(200)));
  await listener.press('sound_remove');
  // Turned off again: nothing more.
  await listener.hear(false);
  const count = listener.audio().length;
  await listener.press('sound');
  assert.equal(listener.audio().length, count);
});

test('no microphone byte has a way to a page: the subscribed events name no stream, and the server never reads microphone.audio', () => {
  assert.equal(schema.events.some((e) => /^(microphone|camera)\./.test(e)), false);
  const source = readFileSync(serverJs, 'utf8').split('\n').filter((line) => !/^\s*\/\//.test(line)).join('\n');
  assert.equal(/microphone\.audio/.test(source), false, 'no code of the server names the sound');
  assert.equal(CONTROLS.find((c) => c.id === 'microphone_stream').ops.join(), 'microphone.stream');
});
