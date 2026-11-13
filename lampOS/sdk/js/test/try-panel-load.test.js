// The try-it page loads its buttons (panel.js) by a dynamic import that a browser can abort (seen on a loaded machine); the page
// retries under another URL and, if it never comes, says so. The function is the one in index.html, taken out between its markers.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const html = readFileSync(new URL('../examples/twin/index.html', import.meta.url), 'utf8');
const source = /\/\/ <panel-load>\n([\s\S]*?)\/\/ <\/panel-load>/.exec(html)?.[1];
const { loadPanel, retrying, fetched, refusedText } = new Function(`${source}\nreturn { loadPanel, retrying, fetched, refusedText };`)();

const run = (probes, loads, tries) => {
  const said = [];
  const urls = [];
  return loadPanel({
    probe: async (n) => { urls.push(`probe ${n}`); const answer = probes[Math.min(n, probes.length - 1)]; if (answer instanceof Error) throw answer; return answer; },
    load: async (n) => { urls.push(`load ${n}`); const answer = loads[Math.min(n, loads.length - 1)]; if (answer instanceof Error) throw answer; },
    say: (text) => said.push(text),
    wait: async () => {},
    tries,
  }).then((outcome) => ({ outcome, said, urls }));
};

test('the page markers are in index.html', () => assert.ok(source && source.includes('async function loadPanel')));

test('a first import that fails is tried again, and the panel comes with nothing said', async () => {
  const { outcome, said, urls } = await run([200], [fetchLost(), fetchLost(), 'ok']);
  assert.equal(outcome, 'loaded');
  assert.deepEqual(said, []);
  assert.deepEqual(urls, ['probe 0', 'load 0', 'probe 1', 'load 1', 'probe 2', 'load 2'], 'each try under its own counter');
});

const fetchLost = () => new TypeError('Failed to fetch dynamically imported module: http://x/panel.js');

test('an import whose panel.js RAN and threw is not run again (each run can leave its part on the page), and the page says why', async () => {
  const { outcome, said, urls } = await run([200], [new ReferenceError('banner is not defined')]);
  assert.equal(outcome, 'failed');
  assert.deepEqual(urls, ['probe 0', 'load 0'], 'one run only');
  assert.equal(said.length, 1);
  assert.match(said[0], /started but stopped.*banner is not defined/);
});

test('each browser words a lost fetch its own way, and each is tried again', async () => {
  for (const text of ['Failed to fetch dynamically imported module: u', 'error loading dynamically imported module: u', 'Importing a module script failed.']) {
    const { outcome } = await run([200], [new TypeError(text), 'ok']);
    assert.equal(outcome, 'loaded', text);
  }
});

test('a probe answered 403 (a wrong token) says so plainly and does not try again', async () => {
  const { outcome, said, urls } = await run([403], ['ok']);
  assert.equal(outcome, 'refused');
  assert.deepEqual(urls, ['probe 0']);
  assert.equal(said.length, 1);
  assert.match(said[0], /403/);
  assert.match(said[0], /link|token/);
});

test('a probe answered 500 is tried again, then said with its number', async () => {
  const { outcome, said, urls } = await run([500], ['ok'], 3);
  assert.equal(outcome, 'failed');
  assert.deepEqual(urls, ['probe 0', 'probe 1', 'probe 2']);
  assert.match(said[0], /500/);
});

test('a probe that fails is also tried again', async () => {
  const { outcome } = await run([new Error('Failed to fetch'), 200], ['ok']);
  assert.equal(outcome, 'loaded');
});

test('when every try fails the page says so, plainly, once', async () => {
  const { outcome, said } = await run([200], [fetchLost()], 4);
  assert.equal(outcome, 'failed');
  assert.deepEqual(said, ['the buttons could not load: reload the page']);
});

test('a twin with no panel (the server says no) says nothing and tries once', async () => {
  const { outcome, said, urls } = await run([404], [new Error('never')]);
  assert.equal(outcome, 'none');
  assert.deepEqual(said, []);
  assert.deepEqual(urls, ['probe 0']);
});

test('the model and the lamp are fetched through retrying: a lost first request is tried again under another URL, then said', async () => {
  const urls = [];
  const got = await retrying(async (n) => { urls.push(n ? `again=${n}` : 'first'); if (n < 2) throw new TypeError('Failed to fetch'); return 'model'; }, { wait: async () => {} });
  assert.equal(got, 'model');
  assert.deepEqual(urls, ['first', 'again=1', 'again=2']);
  const said = [];
  await assert.rejects(retrying(async () => { throw new TypeError('Failed to fetch'); }, { wait: async () => {}, said: () => said.push('line') }), /Failed to fetch/);
  assert.deepEqual(said, ['line'], 'a plain line, once, and the error is not swallowed');
  assert.match(html, /retrying\(\(n\) => fetched\(withToken\('\/model\.json'\)/);
  assert.match(html, /retrying\(\(n\) => new GLTFLoader\(\)\.loadAsync\(withToken\('\/lamp\.glb'\)/);
});

test('a 403 on the model (a wrong token) is not tried again, and is said as a refusal with the way out', async () => {
  let tries = 0;
  const said = [];
  const refusedFetch = async () => ({ ok: false, status: 403 });
  const globalFetch = globalThis.fetch;
  globalThis.fetch = refusedFetch;
  try {
    await assert.rejects(retrying(async () => { tries += 1; return fetched('/model.json?t=x', (r) => r.json()); }, { wait: async () => {}, said: (error) => said.push(error) }), /403/);
  } finally { globalThis.fetch = globalFetch; }
  assert.equal(tries, 1, 'one request');
  assert.equal(said.length, 1);
  assert.match(refusedText(said[0]), /403.*link the terminal printed/);
});

test('the lamp model (a loader error that names the 403) is also a refusal, and any other failure is not', () => {
  assert.match(refusedText(new Error('fetch for "http://x/lamp.glb" responded with 403: Forbidden')), /403/);
  assert.equal(refusedText(new TypeError('Failed to fetch')), undefined);
  assert.equal(refusedText(Object.assign(new Error('the server answered 500'), { status: 500 })), undefined);
});

test('a fetch answered 500 throws with its status; one answered 200 gives its body', async () => {
  const globalFetch = globalThis.fetch;
  try {
    globalThis.fetch = async () => ({ ok: false, status: 500 });
    await assert.rejects(fetched('/x', (r) => r), (error) => error.status === 500);
    globalThis.fetch = async () => ({ ok: true, status: 200, json: async () => ({ a: 1 }) });
    assert.deepEqual(await fetched('/x', (r) => r.json()), { a: 1 });
  } finally { globalThis.fetch = globalFetch; }
});

test('loadPanel names its answer `answer`, not the page\'s status element', () => {
  assert.doesNotMatch(source, /let status\b/);
  assert.match(source, /let answer\b/);
});
