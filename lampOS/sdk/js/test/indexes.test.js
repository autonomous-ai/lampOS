// Shared-skill indexes (decision 4, #3997), as the Python SDK's
// tests/test_index.py has them: a signed list of signed packages, checked on
// this computer, then installed as any shared package against the fake lamp.
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { sign } from 'node:crypto';
import { mkdirSync, mkdtempSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Keyring, Lamp } from '../src/index.js';
import { FakeLamp } from '../src/testing.js';
import { Indexes, checkPackage, find, makeIndex, older, openIndex, refresh } from '../src/indexes.js';
import { fingerprint, openPackage, pack, rawPublic } from '../src/push.js';

const BASE = 'https://skills.example/desk/';
const folder = () => mkdtempSync(join(tmpdir(), 'lampos-index-'));

function made() {
  const publisher = new Keyring(folder()).authorKey({ make: true });
  const author = new Keyring(folder()).authorKey({ make: true });
  const packageOf = (name, rules) => {
    const dir = join(folder(), name);
    mkdirSync(dir);
    writeFileSync(join(dir, 'SKILL.md'), `---\nname: ${name}\ndescription: A test skill.\n---\n`);
    writeFileSync(join(dir, 'skill.toml'), rules);
    return pack(dir, { author }).data;
  };
  const wave = packageOf('desk-wave', 'version = "1.0.0"\n[permissions]\nlight = true\nspeak = true\n');
  const nod = packageOf('desk-nod', 'version = "0.2.0"\n[permissions]\nbody = true\n');
  return { publisher, author, wave, nod, index: makeIndex('desk-things', [wave, nod], BASE, 3, publisher) };
}

// The same index with another listing, signed by `key`.
function resign(listing, key) {
  const text = JSON.stringify(listing);
  const publicBytes = rawPublic(key);
  return Buffer.from(JSON.stringify({ format: 1, index: text, key: fingerprint(publicBytes), public: publicBytes.toString('base64'), signature: sign(null, Buffer.from(text), key).toString('base64') }));
}
const listingOf = (data) => JSON.parse(JSON.parse(data.toString()).index);

test('an index lists each signed package as it is, and a change is refused', () => {
  const m = made();
  const listing = openIndex(m.index);
  assert.deepEqual([listing.name, listing.seq, listing.publisher], ['desk-things', 3, fingerprint(rawPublic(m.publisher))]);
  const wave = listing.skills.find((e) => e.name === 'desk-wave');
  assert.deepEqual([wave.version, wave.size, wave.url, wave.asks, wave.author], ['1.0.0', m.wave.length, `${BASE}desk-wave-1.0.0.tar`, ['light', 'speak'], openPackage(m.wave).author]);
  assert.equal(checkPackage(wave, m.wave).name, 'desk-wave');
  const outer = JSON.parse(m.index.toString());
  assert.throws(() => openIndex(Buffer.from(JSON.stringify({ ...outer, index: outer.index.replace('"seq":3', '"seq":4') }))), /changed after it was signed/);
  assert.throws(() => openIndex(Buffer.from(JSON.stringify({ format: 1, index: outer.index }))), /signed by its publisher/);
});

test('the pinned key and the last seq hold, and an entry out of shape is refused', () => {
  const m = made();
  const listing = listingOf(m.index);
  const pinned = openIndex(m.index).public;
  const other = new Keyring(folder()).authorKey({ make: true });
  assert.throws(() => openIndex(resign(listing, other), { pinned }), (e) => e.code === 'refused' && /remove it and add it again/.test(e.why));
  assert.throws(() => openIndex(resign({ ...listing, seq: 2 }, m.publisher), { pinned, lastSeq: 3 }), /went back from seq 3 to 2/);
  for (const [change, says] of [[{ url: 'http://skills.example/w.tar' }, /https only/], [{ author: null }, /signed packages only/], [{ name: '../wave' }, /not a skill's name/], [{ size: 2 << 20 }, /size is 1 to/],
    [{ url: 'https://me:secret@skills.example/w.tar' }, /no name or password/], [{ withdrawn: true }, /withdrawn is not a field of a format 1 entry/],
    [{ public: Buffer.alloc(32, 1).toString('base64') }, /whose fingerprint is author/]]) {
    const changed = { ...listing, skills: [{ ...listing.skills[0], ...change }, listing.skills[1]] };
    assert.throws(() => openIndex(resign(changed, m.publisher)), says);
  }
});

test('a package that isn\'t the entry\'s is refused', () => {
  const m = made();
  const wave = openIndex(m.index).skills.find((e) => e.name === 'desk-wave');
  assert.throws(() => checkPackage(wave, m.nod), /not the one the index lists/);
  const stranger = rawPublic(new Keyring(folder()).authorKey({ make: true })).toString('base64');
  assert.throws(() => checkPackage({ ...wave, public: stranger }, m.wave), /the index says key/);
  assert.throws(() => checkPackage({ ...wave, asks: ['light'] }, m.wave), /the index says \[light\]/);
});

test('install by name checks the entry, then pushes with asks', async () => {
  const m = made();
  const served = new Map([['https://lan.example/lamp-index.json', m.index], [`${BASE}desk-wave-1.0.0.tar`, m.wave], [`${BASE}desk-nod-0.2.0.tar`, m.nod]]);
  const kept = new Indexes(folder());
  kept.keep('https://lan.example/lamp-index.json', openIndex(m.index));
  const fake = await FakeLamp.start({ grants: ['light', 'speak', 'skills'] });
  const keyring = new Keyring(folder());
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring });
    const lamp = await Lamp.connect(undefined, { keyring });
    try {
      const { entry, data } = await find(kept, 'desk-wave', { fetch: async (url) => served.get(url) });
      const skill = await lamp.installPackage(data);
      assert.deepEqual([skill.name, skill.author, skill.asks], ['desk-wave', entry.author, ['light', 'speak']]);
      assert.deepEqual(fake.asked('skill.push')[0].asks, ['light', 'speak']);
      const swapped = new Map(served).set(`${BASE}desk-wave-1.0.0.tar`, m.nod);
      await assert.rejects(find(kept, 'desk-wave', { fetch: async (url) => swapped.get(url) }), /not the one the index lists/);
      await assert.rejects(find(kept, 'desk-lift', { fetch: async (url) => served.get(url) }), /in no index/);
    } finally { await lamp.close(); }
  } finally { await fake.stop(); }
});

test('lamp skill index make, add, list, browse and remove run as the Python CLI\'s do', async () => {
  const m = made();
  const home = folder();
  const lamp = (typed, ...args) => new Promise((done) => {
    const child = execFile(process.execPath, [new URL('../src/cli.js', import.meta.url).pathname, ...args], { env: { ...process.env, LAMPOS_HOME: home } },
      (error, stdout, stderr) => done({ code: error?.code ?? 0, said: `${stdout}${stderr}`.trim() }));
    child.stdin.end(typed);
  });
  const dir = folder();
  const packages = ['wave', 'nod'].map((name) => { const path = join(dir, `${name}.tar`); writeFileSync(path, m[name]); return path; });
  const out = join(dir, 'lamp-index.json');
  assert.equal((await lamp('', 'skill', 'index', 'make', 'desk-things', ...packages, '--url-base', BASE, '--seq', '1', '--out', out)).code, 1);
  await lamp('', 'skill', 'keygen');
  const making = await lamp('', 'skill', 'index', 'make', 'desk-things', ...packages, '--url-base', BASE, '--seq', '1', '--out', out);
  assert.ok(making.code === 0 && /published by key .*, not the Store, seq 1, 2 skills/.test(making.said), making.said);
  const no = await lamp('n\n', 'skill', 'index', 'add', out);
  assert.ok(no.code === 1 && no.said.endsWith('not added'), no.said);
  const yes = await lamp('y\n', 'skill', 'index', 'add', out);
  assert.ok(yes.code === 0 && yes.said.endsWith('index desk-things added'), yes.said);
  assert.ok((await lamp('', 'skill', 'index', 'list')).said.startsWith('desk-things\tkey '));
  const browsed = await lamp('', 'skill', 'browse', 'wave');
  assert.ok(browsed.said.startsWith('desk-wave\t1.0.0\tsigned by key ') && browsed.said.endsWith('\tasks light, speak\tindex desk-things'), browsed.said);
  assert.equal((await lamp('', 'skill', 'index', 'remove', 'desk-things')).said, 'index desk-things removed');
  assert.equal((await lamp('', 'skill', 'browse')).said, '');
});

test('a push waits out a short rate, says a long one, and a name kept under another signer is refused before a byte', async () => {
  const m = made();
  const otherDir = join(folder(), 'desk-wave');
  mkdirSync(otherDir);
  writeFileSync(join(otherDir, 'SKILL.md'), '---\nname: desk-wave\ndescription: A test skill.\n---\n');
  writeFileSync(join(otherDir, 'skill.toml'), 'version = "1.0.1"\n[permissions]\nlight = true\nspeak = true\n');
  const impostor = pack(otherDir, { author: new Keyring(folder()).authorKey({ make: true }) }).data;
  const fake = await FakeLamp.start({ grants: ['light', 'speak', 'skills'] });
  const keyring = new Keyring(folder());
  try {
    await Lamp.pair(fake.code, fake.host, { pin: fake.pin, port: fake.port, keyring });
    const lamp = await Lamp.connect(undefined, { keyring });
    try {
      fake.refuseNext('skill.push', 'rate', '1 each 10 s', 50);
      assert.equal((await lamp.installPackage(m.wave)).name, 'desk-wave');
      fake.refuseNext('skill.push', 'rate', '20 an hour', 600_000);
      await assert.rejects(lamp.installPackage(m.wave), (e) => e.code === 'rate' && e.retryMs === 600_000);
      const pushed = fake.asked('skill.push').length;
      await assert.rejects(lamp.installPackage(impostor), (e) => e.code === 'signer_changed' && e.message.includes(`desk-wave is signed by key ${openPackage(m.wave).author} on this lamp: remove it first`));
      assert.equal(fake.asked('skill.push').length, pushed);
    } finally { await lamp.close(); }
  } finally { await fake.stop(); }
});

test('one index that can\'t be read is named, the others go on, and a refused one stays loud', async () => {
  const m = made();
  const kept = new Indexes(folder());
  kept.keep('https://lan.example/a.json', openIndex(m.index));
  kept.keep('https://gone.example/b.json', openIndex(makeIndex('other-things', [m.nod], BASE, 1, m.publisher)));
  const served = new Map([['https://lan.example/a.json', m.index], [`${BASE}desk-wave-1.0.0.tar`, m.wave]]);
  const fetch = async (url) => { if (!served.has(url)) throw new Error(`${url}: 404`); return served.get(url); };
  const { listings, unread } = await refresh(kept, fetch);
  assert.deepEqual(listings.map((l) => l.name), ['desk-things']);
  assert.deepEqual(unread, [['other-things', 'https://gone.example/b.json: 404']]);
  await assert.rejects(find(kept, 'desk-wave', { fetch }), /index other-things could not be read .*: say which with --from/);
  assert.equal((await find(kept, 'desk-wave', { fromIndex: 'desk-things', fetch })).entry.name, 'desk-wave');
  served.set('https://lan.example/a.json', resign({ ...listingOf(m.index), seq: 1 }, m.publisher));
  await assert.rejects(refresh(kept, fetch), /went back/);
});

test('the kept indexes are the person\'s alone, a moved URL is said, and an older version is told apart', () => {
  const m = made();
  const kept = new Indexes(folder());
  const listing = openIndex(m.index);
  assert.equal(kept.keep('https://a.example/i.json', listing), null);
  assert.equal(statSync(kept.path).mode & 0o777, 0o600);
  assert.equal(kept.keep('https://b.example/i.json', listing), 'https://a.example/i.json');
  assert.equal(older('1.2.0', '1.10.0'), true);
  assert.equal(older('1.10.0', '1.2.0'), false);
  assert.equal(older('1.0.0', '1.0.0'), false);
  assert.equal(older('1.0.0-beta', '1.0.1'), true);
  assert.equal(older('one', '1.0.0'), null);
});
