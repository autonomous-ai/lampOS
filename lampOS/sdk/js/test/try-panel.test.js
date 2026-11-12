// The page's script has no browser test, so this keeps it whole: it parses as a module, and the parts that make the page
// (the panel, the log, the picture, the listener for the server's messages) are all still there, the listener last.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

const source = readFileSync(new URL('../examples/twin/panel.js', import.meta.url), 'utf8');

test('panel.js parses as a module', (t) => {
  const folder = mkdtempSync(join(tmpdir(), 'try-panel-'));
  t.after(() => rmSync(folder, { recursive: true, force: true }));
  const copy = join(folder, 'panel.mjs');
  writeFileSync(copy, source);
  const checked = spawnSync(process.execPath, ['--check', copy], { encoding: 'utf8' });
  assert.equal(checked.status, 0, checked.stderr);
});

test('panel.js still builds the panel, shows a picture and listens for the server, and ends with that listener', () => {
  for (const part of ['function row(', 'function section(', 'function showPicture(', "id: 'panel'", "addEventListener('twin-message'"]) {
    assert.ok(source.includes(part), `panel.js lost ${part}`);
  }
  assert.match(source.trimEnd(), /addEventListener\('twin-message'[\s\S]*\}\);$/, 'the twin-message listener is the last statement');
});
