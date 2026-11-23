// An app package of this computer's own put on its own lamp (`app.push`, `app.list`, `app.remove`; docs/apps.md):
// checked here before a byte is sent, sent in parts as a skill is, listed and taken off, against the fake lamp.
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gzipSync } from 'node:zlib';
import { test } from 'node:test';
import { Keyring, Lamp, byCode } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';

const folder = () => mkdtempSync(join(tmpdir(), 'lampos-app-'));
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

async function withLamp(options, body) {
  const fake = await FakeLamp.start(options);
  const keyring = new Keyring(folder());
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, name: 'test laptop', keyring });
    const lamp = await Lamp.connect(undefined, { keyring });
    try { await body(lamp, fake, keyring); } finally { await lamp.close(); }
  } finally { await fake.stop(); }
}

// One ustar entry: a header and its data, padded to 512 bytes.
function entry(path, data) {
  const header = Buffer.alloc(512);
  const octal = (value, width) => `${value.toString(8).padStart(width - 1, '0')}\0`;
  header.write(path, 0, 100, 'utf8');
  header.write(octal(0o644, 8), 100);
  header.write(octal(0, 8), 108);
  header.write(octal(0, 8), 116);
  header.write(octal(data.length, 12), 124);
  header.write(octal(0, 12), 136);
  header.fill(' ', 148, 156);
  header.write('0', 156);
  header.write('ustar\0', 257);
  header.write('00', 263);
  let sum = 0;
  for (const byte of header) sum += byte;
  header.write(octal(sum, 8), 148);
  return Buffer.concat([header, data, Buffer.alloc((512 - (data.length % 512)) % 512)]);
}

// A small valid unsigned format 2 package (docs/apps.md): `lamp-skill.json` beside one folder named for the app.
function appTar({ name = 'english-lessons', version = '1.2.0', grants = ['speak'], signed = false, extra = null, gzip = false, format = 2, kind = 'app' } = {}) {
  const files = new Map([['src/lamp_learn/app.py', Buffer.from("from lampos import Lamp\n\nprint('hello')\n")]]);
  if (extra) files.set('src/lamp_learn/blob.py', extra);
  const manifest = {
    format, kind, name, title: 'English lessons', version, min_build: 1,
    runtime: { name: 'python3', entry: 'lamp_learn.app' }, grants, net: false, logs: true,
    phrases: [], triggers: [], settings: [], skills: [], assets: [],
    files: [...files].map(([path, data]) => ({ path, size: data.length, sha256: sha256(data) })),
  };
  const tar = Buffer.concat([
    entry('lamp-skill.json', Buffer.from(JSON.stringify(manifest, null, 2))),
    ...(signed ? [entry('lamp-skill.sig', Buffer.from('{"key":"ab","signature":"c2ln"}'))] : []),
    ...[...files].map(([path, data]) => entry(`${name}/${path}`, data)),
    Buffer.alloc(1024),
  ]);
  const path = join(folder(), `${name}.tar`);
  writeFileSync(path, gzip ? gzipSync(tar) : tar);
  return path;
}

test('an app is pushed in parts, listed, replaced by a re-push and removed', async () => {
  await withLamp({ grants: ['speak', 'light', 'skills'] }, async (lamp, fake) => {
    const path = appTar({ extra: randomBytes(100_000) });
    const app = await lamp.pushApp(path);
    assert.deepEqual(app, {
      name: 'english-lessons', title: 'English lessons', version: '1.2.0', sha256: app.sha256, grants: ['speak'],
      net: false, logs: true, updated: false, started: true, from: 'computer',
    });
    const pushes = fake.requests.filter((r) => r.op === 'app.push');
    assert.ok(pushes.length > 1);
    assert.deepEqual(pushes.map((p) => p.part), [...pushes.keys()]);
    assert.ok(pushes.every((p) => p.parts === pushes.length && p.data.length <= 49152));
    const listed = await lamp.apps();
    assert.deepEqual(listed.map((a) => Object.keys(a).sort()), [['grants', 'logs', 'name', 'net', 'sha256', 'stopped', 'title', 'version']]);
    // A re-push of the name replaces it, any version (an older one too).
    const again = await lamp.pushApp(appTar({ version: '1.0.0', grants: ['speak', 'light'] }));
    assert.deepEqual([again.version, again.updated, again.grants], ['1.0.0', true, ['speak', 'light']]);
    assert.deepEqual((await lamp.apps()).map((a) => [a.name, a.version]), [['english-lessons', '1.0.0']]);
    assert.equal((await lamp.removeApp('english-lessons')).removed, 'english-lessons');
    assert.deepEqual(await lamp.apps(), []);
    await assert.rejects(lamp.removeApp('english-lessons'), (e) => e instanceof byCode.refused && /not one of this computer's pushed apps/.test(e.why));
  });
});

test('an app push without the skills grant, or beyond the pairing, is refused before a byte is sent', async () => {
  await withLamp({ grants: ['speak'] }, async (lamp, fake) => {
    await assert.rejects(lamp.pushApp(appTar()), (e) => e instanceof byCode.no_grant && e.why === 'not granted: skills');
    assert.deepEqual(fake.requests.filter((r) => r.op === 'app.push'), []);
  });
  await withLamp({ grants: ['speak', 'skills'] }, async (lamp, fake) => {
    await assert.rejects(lamp.pushApp(appTar({ grants: ['speak', 'camera'] })), (e) => e instanceof byCode.no_grant && e.why === 'not granted: camera');
    // `remember` is an installed Store app's alone: never a pushed app's.
    await assert.rejects(lamp.pushApp(appTar({ grants: ['remember'] })), (e) => e instanceof byCode.no_grant && e.why === 'not granted: remember');
    assert.deepEqual(fake.requests.filter((r) => r.op === 'app.push'), []);
  });
});

test('a signed, oversize, gzip or skill package is refused before a byte is sent', async () => {
  await withLamp({ grants: ['speak', 'skills'] }, async (lamp, fake) => {
    await assert.rejects(lamp.pushApp(appTar({ signed: true })), (e) => e instanceof byCode.refused && e.why === 'it carries lamp-skill.sig: a pushed app is unsigned');
    await assert.rejects(lamp.pushApp(appTar({ extra: randomBytes(4 << 20) })), (e) => e instanceof byCode.bad_request && /a pushed app is 4 MiB at most/.test(e.why));
    await assert.rejects(lamp.pushApp(appTar({ gzip: true })), (e) => e instanceof byCode.bad_request && /not a tar/.test(e.why));
    await assert.rejects(lamp.pushApp(appTar({ format: 1, kind: undefined })), (e) => e instanceof byCode.bad_request && /format 2, kind "app"/.test(e.why));
    await assert.rejects(lamp.pushApp(appTar({ name: 'English' })), (e) => e instanceof byCode.bad_request && /an app's name/.test(e.why));
    assert.deepEqual(fake.requests.filter((r) => r.op === 'app.push'), []);
  });
});

test('the lamp refuses an app push in its own words', async () => {
  for (const [options, why] of [[{ storeApps: false }, 'apps are off on this lamp'], [{ appsHelper: false }, 'app push is not on this lamp']]) {
    await withLamp({ grants: ['speak', 'skills'], ...options }, async (lamp) => {
      await assert.rejects(lamp.pushApp(appTar()), (e) => e instanceof byCode.refused && e.why === why);
      await assert.rejects(lamp.apps(), (e) => e instanceof byCode.refused && e.why === why);
    });
  }
  await withLamp({ grants: ['speak', 'skills'] }, async (lamp, fake) => {
    fake.storeAppNames.add('english-lessons');
    await assert.rejects(lamp.pushApp(appTar()), (e) => e instanceof byCode.refused && e.why === 'a Store app has that name');
  });
});

test('lamp app push, list and remove print as the skill commands do', async () => {
  const CLI = new URL('../src/cli.js', import.meta.url).pathname;
  const fake = await FakeLamp.start({ grants: ['speak', 'skills'] });
  const home = folder();
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring: new Keyring(home) });
    const lamp = (...args) => new Promise((resolve) => {
      execFile(process.execPath, [CLI, ...args], { env: { ...process.env, LAMPOS_HOME: home } }, (error, stdout, stderr) => {
        resolve([error ? error.code : 0, stdout.trim(), stderr.trim()]);
      });
    });
    const path = appTar();
    assert.deepEqual(await lamp('app', 'push', path), [0, 'english-lessons 1.2.0 pushed, running', '']);
    assert.equal((await lamp('app', 'push', path))[1], 'english-lessons 1.2.0 pushed, replacing the one before, running');
    assert.equal((await lamp('app', 'list'))[1], 'english-lessons\t1.2.0\trunning');
    assert.equal((await lamp('app', 'remove', 'english-lessons'))[1], 'english-lessons removed');
    assert.equal((await lamp('app', 'list'))[1], '');
    const signed = await lamp('app', 'push', appTar({ signed: true }));
    assert.deepEqual([signed[0], signed[2]], [1, 'lamp: refused: it carries lamp-skill.sig: a pushed app is unsigned']);
  } finally { await fake.stop(); }
});
