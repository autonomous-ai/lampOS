import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const CLI = new URL('../src/cli.js', import.meta.url).pathname;
const run = (...args) => spawnSync(process.execPath, [CLI, ...args], { encoding: 'utf8' });

test('lamp --help, -h and help print the usage and exit 0', () => {
  for (const args of [['--help'], ['-h'], ['help']]) {
    const done = run(...args);
    assert.equal(done.status, 0, args.join(' '));
    assert.match(done.stdout, /^usage: lamp find/);
    assert.match(done.stdout, /lamp skill push FOLDER/);
    assert.equal(done.stderr, '');
  }
});

test('a misspelt option is one line and the usage, exit 2, never a stack trace', () => {
  const done = run('--bogus');
  assert.equal(done.status, 2);
  assert.match(done.stderr, /^lamp: Unknown option '--bogus'\nusage: lamp find/);
  assert.doesNotMatch(done.stderr, /node:internal|at checkOptionUsage|ERR_PARSE_ARGS/);
});

test('lamp with no command prints the usage on stderr and exits 2', () => {
  const done = run();
  assert.equal(done.status, 2);
  assert.match(done.stderr, /^usage: lamp find/);
});
