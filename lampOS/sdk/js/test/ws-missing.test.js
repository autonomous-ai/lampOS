// A link install of the SDK cannot find `ws` from the checkout; the error says what to run.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { WS_MISSING } from '../src/ws.js';

test('an SDK that cannot find ws says to install it with --install-links', (t) => {
  const folder = mkdtempSync(join(tmpdir(), 'lampos-ws-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  // A copy of the sources with no node_modules above it: `ws` is not there.
  cpSync(new URL('../src', import.meta.url), join(folder, 'src'), { recursive: true });
  cpSync(new URL('../package.json', import.meta.url), join(folder, 'package.json'));
  const run = spawnSync(process.execPath, ['--input-type=module', '-e', "import('./src/index.js')"], { cwd: folder, encoding: 'utf8' });
  assert.notEqual(run.status, 0);
  assert.ok(run.stderr.includes(WS_MISSING), run.stderr);
});

// Node 22.12 can require() an ES module that has no top-level await: a CommonJS app does.
const [major, minor] = process.versions.node.split('.').map(Number);
test('a CommonJS app can require the SDK', { skip: major < 22 || (major === 22 && minor < 12) }, () => {
  const run = spawnSync(process.execPath, ['-e', "const { Lamp, LampError } = require('./src/index.js'); if (typeof Lamp.connect !== 'function' || !LampError) process.exit(3);"], {
    cwd: new URL('..', import.meta.url).pathname,
    encoding: 'utf8',
  });
  assert.equal(run.status, 0, run.stderr);
});
