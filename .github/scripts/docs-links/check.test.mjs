import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { checkDocs, parseDocument } from './check.mjs';

function tree(t, files) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'lamp-doc-links-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const [name, content] of Object.entries(files)) {
    const file = path.join(root, name);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, content);
  }
  return root;
}

test('GitHub headings include formatted text, duplicate suffixes and Unicode', () => {
  const doc = parseDocument('# A **bold** `code` [link](https://example.test) &amp; café!\n\n'
    + '## Again\n\n## Again\n\nA setext heading\n---\n');
  assert.deepEqual([...doc.anchors], ['a-bold-code-link--café', 'again', 'again-1', 'a-setext-heading']);
});

test('explicit HTML anchors survive, but examples and comments do not', () => {
  const doc = parseDocument('<a id="kept"></a>\n\nA <span id=inline>span</span>.\n\n'
    + "<a name='old-anchor'></a>\n\n<!-- <a id='comment'></a> -->\n\n"
    + '```md\n<a id="code"></a>\n[example](missing.md)\n```\n\n'
    + '`[example](missing.md)`\n\n    [indented example](missing.md)\n');
  assert.deepEqual([...doc.anchors], ['kept', 'inline', 'old-anchor']);
  assert.deepEqual(doc.links, []);
});

test('inline, reference, image and fragment links resolve with escaped destinations', t => {
  const root = tree(t, {
    'docs/index.md': '# Here\n\n[local](#here) [empty](#) [ref][read]\n\n'
      + '[read]: <target file.md#an-anchor> "a title"\n\n'
      + '[percent](target%20file.md#an-anchor) ![picture](../image.svg)\n'
      + '[code](../source.rs#L1) [folder](../folder/)\n',
    'docs/target file.md': '<a id="an-anchor"></a>\n',
    'image.svg': '<svg/>',
    'source.rs': '// source',
    'folder/README.md': '# Folder',
  });
  assert.deepEqual(checkDocs(root), { files: 2, checked: 7, errors: [] });
});

test('missing file and wrong fragment are each reported and produce exit 1', t => {
  const root = tree(t, {
    'docs/a.md': '[file](absent.md)\n\n[fragment](b.md#wrong)\n',
    'docs/b.md': '# Present\n',
  });
  const result = checkDocs(root);
  assert.equal(result.errors.length, 2);
  assert.match(result.errors[0], /a\.md:1: absent\.md: target does not exist/);
  assert.match(result.errors[1], /a\.md:3: b\.md#wrong: heading or HTML anchor does not exist/);
  const cli = spawnSync(process.execPath, [fileURLToPath(new URL('./check.mjs', import.meta.url)), root], { encoding: 'utf8' });
  assert.equal(cli.status, 1);
  assert.match(cli.stdout, /2 broken/);
});

test('percent-encoded Unicode fragments and root-relative files resolve', t => {
  const root = tree(t, { 'docs/a.md': '# Café\n\n[yes](/docs/a.md#caf%C3%A9)\n' });
  assert.equal(checkDocs(root).errors.length, 0);
});

test('network destinations are skipped and historical docs are not scanned', t => {
  const root = tree(t, {
    'docs/a.md': '[https](https://invalid.example/#missing) [mail](mailto:nobody@example.test) '
      + '[network](//invalid.example/) [record](verification/old.md#receipt)\n',
    'docs/verification/old.md': '# Receipt\n\n[historical](no-longer-present.md)\n',
  });
  assert.deepEqual(checkDocs(root), { files: 1, checked: 1, errors: [] });
});

test('a sibling file or escaping symlink cannot conceal a standalone broken link', t => {
  const root = tree(t, { 'docs/a.md': '[outside](../outside.md)\n' });
  const sibling = tree(t, { 'external.md': '# External\n' });
  fs.symlinkSync(path.join(sibling, 'external.md'), path.join(root, 'outside.md'));
  assert.match(checkDocs(root).errors[0], /outside the lampOS source tree/);
});

test('deleted docs directory or an empty scan fails rather than passing', t => {
  const root = tree(t, { 'docs/not-markdown.txt': '' });
  assert.throws(() => checkDocs(root), /no top-level Markdown docs/);
  assert.throws(() => checkDocs(path.join(root, 'missing')), /ENOENT/);
});


test('HTML anchor values handle entities and quoted greater-than without fake attributes', () => {
  const doc = parseDocument(`<span title="fake id='absent'" id="a&gt;b">text</span>\n`);
  assert.deepEqual([...doc.anchors], ['a>b']);
});
