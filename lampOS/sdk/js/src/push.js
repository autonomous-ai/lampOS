// Putting a skill of your own on your own paired lamp (docs/control.md,
// "Skills pushed from a computer"): pack a skill folder as `lamp-skills pack`
// does, and the parts `skill.push` sends.
//
// A package is a ustar tar: `lamp-skill.json` first (format, name, version,
// and each file's path, size and SHA-256), then each file as `NAME/path`,
// sorted, mode 0644, mtime 0, owner 0, with no folder entries and no links.
// Hidden files (a name beginning with a dot) are left out, as the lamp's
// loader passes them over. The lamp checks the bytes against `sha256`, then
// each file against the manifest, and checks the skill itself as a load
// does; the Python SDK's `lamp skill check` mirrors that check before a push.
import { createHash } from 'node:crypto';
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { LampError } from './errors.js';

export const PACKAGE_MAX = 1 << 20;
// A part's bytes, so its base64 is at most 48 KiB and a frame stays under 64 KiB.
export const PART_BYTES = 36 * 1024;

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

export const MOST_DEPTH = 4;

// A file's path in a skill as lamp-skills' `path_ok` takes it: 1 to 4 parts
// of letters, digits, -, _ and ., none beginning with a dot. So no path in a
// package reaches outside its folder when written out.
export function pathOk(path) {
  if (typeof path !== 'string') return false;
  const parts = path.split('/');
  return parts.length <= MOST_DEPTH && parts.every((part) => /^[A-Za-z0-9_.-]{1,100}$/.test(part) && !part.startsWith('.'));
}

// Every plain file but hidden ones, by its `/` path, sorted; a link refuses
// it, and so does a path lamp-skills would refuse (Reviewer-lead's P3 on #4029).
function files(folder) {
  const found = new Map();
  const walk = (dir, prefix) => {
    for (const name of readdirSync(dir)) {
      if (name.startsWith('.')) continue;
      const path = prefix ? `${prefix}/${name}` : name;
      const stat = lstatSync(join(dir, name));
      if (stat.isSymbolicLink()) throw new LampError(`${path}: a link: a skill is plain files and folders`, { code: 'bad_request' });
      if (stat.isDirectory()) walk(join(dir, name), path);
      else if (stat.isFile()) {
        if (!pathOk(path)) throw new LampError(`${path}: a skill's paths are at most ${MOST_DEPTH} parts of letters, digits, -, _ and ., none beginning with a dot`, { code: 'bad_request' });
        found.set(path, readFileSync(join(dir, name)));
      }
    }
  };
  walk(folder, '');
  return new Map([...found].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
}

// A top-level string key of skill.toml (before its first table), basic or
// literal (`"1.0.0"` or `'1.0.0'`), or undefined.
function topLevel(toml, key) {
  for (const line of toml.split('\n')) {
    if (/^\s*\[/.test(line)) return undefined;
    const match = line.match(new RegExp(`^\\s*${key}\\s*=\\s*(?:"([^"]*)"|'([^']*)')`));
    if (match) return match[1] ?? match[2];
  }
  return undefined;
}

// One ustar header and its data, padded to 512 bytes, as the tar crate
// writes them: mode 0644, mtime 0, owner 0.
function entry(path, data) {
  const header = Buffer.alloc(512);
  let name = path;
  let prefix = '';
  if (Buffer.byteLength(name) > 100) {
    // The prefix is up to the last `/` within 155 bytes, as the tar crate's
    // ustar `set_path` peels it. Where that leaves a name over 100, or there
    // is no such `/`, the crate writes a GNU long-name entry instead: refused
    // here, so the bytes stay the crate's.
    const cut = path.lastIndexOf('/', 155);
    if (cut < 0 || Buffer.byteLength(path) - cut - 1 > 100) throw new LampError(`${path}: longer than a tar header holds; shorten its folders`, { code: 'bad_request' });
    prefix = path.slice(0, cut);
    name = path.slice(cut + 1);
  }
  const octal = (value, width) => `${value.toString(8).padStart(width - 1, '0')}\0`;
  header.write(name, 0, 100, 'utf8');
  header.write(octal(0o644, 8), 100, 8, 'ascii');
  header.write(octal(0, 8), 108, 8, 'ascii');
  header.write(octal(0, 8), 116, 8, 'ascii');
  header.write(octal(data.length, 12), 124, 12, 'ascii');
  header.write(octal(0, 12), 136, 12, 'ascii');
  header.fill(' ', 148, 156);
  header.write('0', 156, 1, 'ascii');
  header.write('ustar\0', 257, 6, 'ascii');
  header.write('00', 263, 2, 'ascii');
  header.write(prefix, 345, 155, 'utf8');
  let sum = 0;
  for (const byte of header) sum += byte;
  // Seven octal digits and a NUL, as the tar crate's `octal_into`, so the
  // bytes are lamp-skills' (harness-reviewer's P3 on #4029).
  header.write(octal(sum, 8), 148, 8, 'ascii');
  const padding = Buffer.alloc((512 - (data.length % 512)) % 512);
  return Buffer.concat([header, data, padding]);
}

// The skill folder as a package. Its name is the folder's, and skill.toml
// must say a top-level `version`; a core or add-on skill is never pushed.
export function pack(folder) {
  const dir = resolve(folder);
  const name = basename(dir);
  let toml;
  try {
    toml = readFileSync(join(dir, 'skill.toml'), 'utf8');
  } catch (error) {
    throw new LampError(`skill.toml: ${error.message}`, { code: 'bad_request' });
  }
  const version = topLevel(toml, 'version');
  if (!version) throw new LampError('skill.toml says no version: a package carries one, as version = "1.0.0"', { code: 'bad_request' });
  const kind = topLevel(toml, 'kind') ?? 'installable';
  if (kind !== 'installable') throw new LampError(`kind = "${kind}": a pushed skill is installable`, { code: 'refused' });
  const all = files(dir);
  const manifest = {
    format: 1,
    name,
    version,
    files: [...all].map(([path, data]) => ({ path, size: data.length, sha256: sha256(data) })),
  };
  const data = Buffer.concat([
    entry('lamp-skill.json', Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`)),
    ...[...all].map(([path, bytes]) => entry(`${name}/${path}`, bytes)),
    Buffer.alloc(1024),
  ]);
  if (data.length > PACKAGE_MAX) throw new LampError(`the package is ${data.length} bytes: a pushed skill is 1 MiB at most`, { code: 'bad_request' });
  const parts = [];
  for (let at = 0; at < data.length; at += PART_BYTES) parts.push(data.subarray(at, at + PART_BYTES).toString('base64'));
  return { name, version, data, sha256: sha256(data), parts };
}
