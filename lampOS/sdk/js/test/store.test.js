// submitApp, submissions, requestPublic and the sign-in, against a fake Store
// on this computer that answers as Backend3's spec says (#7415, 20 Nov).
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, statSync, readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { StoreError, endpoints, forgetToken, keptToken, requestPublic, sendLoginCode, signIn, submissions, submitApp } from '../src/index.js';

const OTP = '123456';
const TOKEN = 'tok-dev-1';

// A minimal ustar: each [path, bytes] as a 512-byte header and its padded data.
function tar(entries) {
  const blocks = [];
  for (const [path, data] of entries) {
    const header = Buffer.alloc(512);
    header.write(path, 0);
    header.write('0000644\0', 100); header.write('0000000\0', 108); header.write('0000000\0', 116);
    header.write(`${data.length.toString(8).padStart(11, '0')}\0`, 124); header.write('00000000000\0', 136);
    header.fill(' ', 148, 156); header.write('0', 156); header.write('ustar\0', 257); header.write('00', 263);
    let sum = 0;
    for (const byte of header) sum += byte;
    header.write(`${sum.toString(8).padStart(6, '0')}\0 `, 148);
    blocks.push(header, data, Buffer.alloc((512 - (data.length % 512)) % 512));
  }
  return Buffer.concat([...blocks, Buffer.alloc(1024)]);
}
const manifest = (fields) => Buffer.from(JSON.stringify({ format: 2, kind: 'app', name: 'gym-coach', title: 'Gym coach', version: '1.0.0', ...fields }));
const APP = tar([['lamp-skill.json', manifest({})], ['gym-coach/src/gym_coach/app.py', Buffer.from("print('hi')\n")]]);

function png(width, height) {
  const b = Buffer.alloc(33);
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).copy(b);
  b.writeUInt32BE(13, 8); b.write('IHDR', 12); b.writeUInt32BE(width, 16); b.writeUInt32BE(height, 20);
  return b;
}

async function fakeStore(t) {
  const fake = { requests: [], apps: new Map(), refuseWith: null };
  const server = createServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const body = Buffer.concat(chunks);
    fake.requests.push({ method: req.method, url: req.url, headers: req.headers, body });
    const send = (status, value) => { res.writeHead(status, { 'Content-Type': 'application/json' }); res.end(JSON.stringify(value)); };
    const fail = (status, code, message) => send(status, { status: -1, error_code: code, error_message: message });
    const path = req.url.split('?')[0];
    if (path === '/account/send-login-verification') return send(200, { status: 1, data: null });
    if (path === '/account/sign-in') {
      const asked = JSON.parse(body);
      return asked.grant_type === 'otp' && asked.otp === OTP ? send(200, { status: 1, data: { access_token: TOKEN } }) : fail(400, 'authentication_error', 'invalid otp');
    }
    if (req.headers.authorization !== `Bearer ${TOKEN}`) return fail(401, 'authentication_error', 'unauthorized');
    if (path === '/store/apps' && req.method === 'POST') {
      if (fake.refuseWith) return fail(400, ...fake.refuseWith);
      const form = await new Response(body, { headers: { 'content-type': req.headers['content-type'] } }).formData();
      const app = { id: String(fake.apps.size + 1), name: 'gym-coach', version: '1.0.0', title: form.get('title'), review_status: 'pending', visibility: 'unlisted', public_requested: false, fields: [...form.keys()].sort(), size: (await form.get('file').arrayBuffer()).byteLength };
      fake.apps.set(app.id, app);
      return send(200, { status: 1, data: app });
    }
    if (path === '/store/apps') return send(200, { status: 1, data: { data: [...fake.apps.values()], total: fake.apps.size } });
    const [, , , id, what] = path.split('/');
    const app = fake.apps.get(id);
    if (!app) return fail(404, 'app_not_found', 'not found');
    if (!what) return send(200, { status: 1, data: app });
    if (app.review_status !== 'approved') return fail(400, 'app_not_approved', 'not approved');
    app.public_requested = true;
    return send(200, { status: 1, data: app });
  });
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  const base = `http://127.0.0.1:${server.address().port}`;
  fake.where = { account: `${base}/account`, store: `${base}/store` };
  const homeBefore = process.env.LAMPOS_HOME;
  const tokenBefore = process.env.LAMP_STORE_TOKEN;
  fake.home = mkdtempSync(join(tmpdir(), 'lampos-store-'));
  process.env.LAMPOS_HOME = fake.home;
  delete process.env.LAMP_STORE_TOKEN;
  t.after(() => {
    server.close();
    rmSync(fake.home, { recursive: true, force: true });
    if (homeBefore === undefined) delete process.env.LAMPOS_HOME; else process.env.LAMPOS_HOME = homeBefore;
    if (tokenBefore !== undefined) process.env.LAMP_STORE_TOKEN = tokenBefore;
  });
  return fake;
}

test('sign in with the emailed code, submit, follow the review and ask to go public', async (t) => {
  const fake = await fakeStore(t);
  const where = fake.where;
  await sendLoginCode('dev@example.com', { where });
  assert.equal(await signIn('dev@example.com', OTP, { where }), TOKEN);
  const kept = join(fake.home, 'store.json');
  assert.equal(statSync(kept).mode & 0o777, 0o600);
  assert.equal(JSON.parse(readFileSync(kept, 'utf8'))[where.account].token, TOKEN);
  assert.equal(keptToken(where), TOKEN);
  assert.ok(fake.requests.every((r) => r.headers.location === 'en-US'));
  assert.ok(fake.requests.every((r) => !r.body.toString().includes('password')));

  const app = await submitApp(APP, { where, icon: png(1024, 1024), cover: png(1600, 900), title: 'Gym coach', category: 'health', filename: 'gym-coach-1.0.0.tar' });
  assert.equal(app.review_status, 'pending');
  assert.deepEqual(app.fields, ['category', 'cover', 'file', 'icon', 'title']);
  assert.equal(app.size, APP.length);
  assert.equal((await submissions(undefined, { where })).total, 1);
  assert.equal((await submissions('1', { where })).visibility, 'unlisted');
  await assert.rejects(requestPublic('1', { where }), (e) => e instanceof StoreError && e.code === 'app_not_approved' && /only an approved app/.test(e.why));
  fake.apps.get('1').review_status = 'approved';
  assert.equal((await requestPublic('1', { where })).public_requested, true);
  await assert.rejects(submissions('9', { where }), (e) => e.code === 'app_not_found');
  assert.equal(forgetToken(where), true);
  assert.equal(keptToken(where), null);
});

test("the Store's refusal is said with its reason and what to do", async (t) => {
  const fake = await fakeStore(t);
  const opts = { where: fake.where, token: TOKEN };
  fake.refuseWith = ['lampos_package_invalid', 'lamp-skill.json: help.en: no quotes, markup or links (found "<")'];
  await assert.rejects(submitApp(APP, opts), (e) => e.code === 'lampos_package_invalid' && e.why.startsWith('lamp-skill.json: help.en: no quotes'));
  fake.refuseWith = ['developer_attempt_limit', 'limit'];
  await assert.rejects(submitApp(APP, opts), (e) => /today's 30 upload attempts/.test(e.why));
  await assert.rejects(signIn('dev@example.com', '000000', { where: fake.where }), (e) => e.code === 'authentication_error');
  await assert.rejects(submissions(undefined, { where: fake.where, token: 'stale' }), (e) => e.code === 'authentication_error' && /run `lamp login` again/.test(e.why));
});

// Each refusal the READMEs list, with the words they promise.
const REFUSALS = {
  skill_version_exists: 'raise `version` in app.json',
  skill_name_exists: 'change `name` in app.json',
  skill_name_reserved: "a lampOS built-in's: change `name` in app.json",
  developer_app_only: '`lamp skill push`',
  listing_invalid: 'the title or description is too long, or holds markup or a control character',
  skill_media_invalid: 'icon 1024x1024, cover 1600x900',
  developer_upload_in_progress: 'already sending an upload: wait for it, then try again',
  developer_upload_disabled: 'uploads are off for now: try again later',
  developer_upload_unavailable: 'upload is unavailable for now: try again later',
};

for (const [code, words] of Object.entries(REFUSALS)) {
  test(`the Store's ${code} says what to do`, async (t) => {
    const fake = await fakeStore(t);
    fake.refuseWith = [code, 'raw words'];
    await assert.rejects(submitApp(APP, { where: fake.where, token: TOKEN }), (e) => e instanceof StoreError && e.code === code && e.why.includes(words) && e.why.endsWith('(the Store: raw words)'));
  });
}

test('with no sign-in, or a package that is plainly not an app, nothing is sent', async (t) => {
  const fake = await fakeStore(t);
  await assert.rejects(submitApp(APP, { where: fake.where }), (e) => e.code === 'authentication_error' && /run `lamp login`/.test(e.why));
  const opts = { where: fake.where, token: TOKEN };
  const skill = tar([['lamp-skill.json', Buffer.from(JSON.stringify({ format: 1, name: 'hello', version: '1.0.0', files: [] }))]]);
  await assert.rejects(submitApp(skill, opts), (e) => e.code === 'bad_request' && /lamp skill push/.test(e.why));
  await assert.rejects(submitApp(Buffer.from([0x1f, 0x8b, 0, 0]), opts), (e) => /gzip/.test(e.why));
  await assert.rejects(submitApp(APP, { ...opts, icon: png(512, 512) }), (e) => /512x512; the Store takes exactly 1024x1024/.test(e.why));
  await assert.rejects(submitApp(APP, { ...opts, title: 'x'.repeat(101) }), (e) => /title/.test(e.why));
  assert.equal(fake.requests.length, 0);
});

// Redirects and plain-http URLs (Migrate's HOLD on #8358).
async function listen(t, handler) {
  const seen = [];
  const server = createServer((req, res) => { seen.push(req.headers); handler(req, res); });
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  t.after(() => server.close());
  return { seen, base: `http://127.0.0.1:${server.address().port}` };
}

for (const status of [301, 302, 307, 308]) {
  test(`a ${status} is refused and the token goes nowhere else`, async (t) => {
    const elsewhere = await listen(t, (req, res) => { res.writeHead(200, { 'Content-Type': 'application/json' }); res.end('{"status":1,"data":{"data":[],"total":0}}'); });
    const store = await listen(t, (req, res) => { res.writeHead(status, { Location: `${elsewhere.base}/store/apps` }); res.end(); });
    const where = { account: `${store.base}/account`, store: `${store.base}/store` };
    await assert.rejects(submissions(undefined, { where, token: TOKEN }), (e) => e instanceof StoreError && e.code === 'store' && e.why === 'the Store answered HTTP 3xx');
    await assert.rejects(submitApp(APP, { where, token: TOKEN }), (e) => e instanceof StoreError && e.code === 'store');
    assert.equal(store.seen.length, 2);
    assert.equal(store.seen[0].authorization, `Bearer ${TOKEN}`);
    assert.deepEqual(elsewhere.seen, [], "the redirect's target got no request");
  });
}

test('https://, or http:// on this computer, is taken', (t) => {
  const before = [process.env.LAMP_STORE_URL, process.env.LAMP_ACCOUNT_URL];
  t.after(() => {
    for (const [key, value] of [['LAMP_STORE_URL', before[0]], ['LAMP_ACCOUNT_URL', before[1]]]) if (value === undefined) delete process.env[key]; else process.env[key] = value;
  });
  for (const url of ['https://store.example.com/api', 'http://localhost:8080/store', 'http://127.0.0.1:9/store', 'http://[::1]:9/store']) {
    process.env.LAMP_STORE_URL = `${url}/`;
    process.env.LAMP_ACCOUNT_URL = url;
    assert.deepEqual(endpoints(), { account: url, store: url });
  }
});

test('plain http:// elsewhere is refused before any call', async (t) => {
  const fake = await fakeStore(t);
  const before = [process.env.LAMP_STORE_URL, process.env.LAMP_ACCOUNT_URL];
  t.after(() => {
    for (const [key, value] of [['LAMP_STORE_URL', before[0]], ['LAMP_ACCOUNT_URL', before[1]]]) if (value === undefined) delete process.env[key]; else process.env[key] = value;
  });
  for (const env of ['LAMP_STORE_URL', 'LAMP_ACCOUNT_URL']) {
    for (const url of ['http://store.example.com/api', 'http://10.0.0.5/store', 'ftp://store.example.com', 'store.example.com']) {
      delete process.env.LAMP_STORE_URL;
      delete process.env.LAMP_ACCOUNT_URL;
      process.env[env] = url;
      assert.throws(() => endpoints(), (e) => e instanceof StoreError && e.code === 'bad_url' && e.why.includes(env) && e.why.includes('https://'));
      await assert.rejects(submissions(undefined, { token: TOKEN }), (e) => e.code === 'bad_url');
      await assert.rejects(submissions(undefined, { where: { account: url, store: url }, token: TOKEN }), (e) => e.code === 'bad_url');
    }
  }
  assert.equal(fake.requests.length, 0);
});
