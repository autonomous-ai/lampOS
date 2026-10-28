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
import { createHash, createPublicKey, sign, verify } from 'node:crypto';
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { LampError } from './errors.js';

export const MANIFEST = 'lamp-skill.json';
// The author's signature, the package's second entry (lamp-skills'
// package.rs): {"key": FPR16, "public": B64, "signature": B64} over the
// manifest's exact bytes.
export const SIGNATURE = 'lamp-skill.sig';
const SIGNATURE_MAX = 1024;
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

// A key's fingerprint, as the lamp shows it: the first 16 hex of the SHA-256
// of its 32 bytes (lamp-update's `manifest::fingerprint`).
export const fingerprint = (publicBytes) => sha256(publicBytes).slice(0, 16);

export const rawPublic = (key) => {
  const der = createPublicKey(key).export({ type: 'spki', format: 'der' });
  return der.subarray(der.length - 32);
};

// `lamp-skill.sig` for these manifest bytes, compact JSON and a newline, as
// lamp-skills writes it, with the author's `public` beside `key`.
export function signatureFor(manifest, author) {
  const publicBytes = rawPublic(author);
  const signed = {
    key: fingerprint(publicBytes),
    public: publicBytes.toString('base64'),
    signature: sign(null, manifest, author).toString('base64'),
  };
  return Buffer.from(`${JSON.stringify(signed)}\n`);
}

const bad = (why) => new LampError(`not a skill package: ${why}`, { code: 'bad_request' });

// The entries of a ustar tar, in order: { name, type, data }.
function entries(data) {
  const found = [];
  for (let at = 0; at + 512 <= data.length;) {
    const header = data.subarray(at, at + 512);
    if (header.every((byte) => byte === 0)) break;
    const text = (from, length) => header.subarray(from, from + length).toString('utf8').replace(/\0.*$/s, '');
    const size = parseInt(text(124, 12).trim() || '0', 8);
    const prefix = text(345, 155);
    const name = prefix ? `${prefix}/${text(0, 100)}` : text(0, 100);
    const type = text(156, 1) || '0';
    if (Number.isNaN(size) || at + 512 + size > data.length) throw bad('a tar cut short');
    found.push({ name, type, data: data.subarray(at + 512, at + 512 + size) });
    at += 512 + Math.ceil(size / 512) * 512;
  }
  return found;
}

// Check a package as the lamp does before anything is written: the manifest
// first, the signature (if any) over its exact bytes by the key it names, then
// every file against the manifest, no more and no fewer. Returns
// { name, version, author, files } with `author` the key's fingerprint, or
// null when it isn't signed.
export function openPackage(data) {
  const all = entries(data);
  if (all[0]?.name !== MANIFEST) throw bad(`${MANIFEST} must come first`);
  const read = new Map();
  for (const entry of all) {
    if (entry.type !== '0') throw bad(`${entry.name}: a package holds only plain files`);
    if (read.has(entry.name)) throw bad(`${entry.name} twice`);
    read.set(entry.name, entry.data);
  }
  const manifestBytes = read.get(MANIFEST);
  read.delete(MANIFEST);
  let manifest;
  try {
    manifest = JSON.parse(manifestBytes.toString('utf8'));
  } catch (error) {
    throw bad(`${MANIFEST}: ${error.message}`);
  }
  const { name, version, files: listed } = manifest;
  if (typeof name !== 'string' || typeof version !== 'string' || !Array.isArray(listed)) throw bad(`${MANIFEST}: name, version and files`);
  if (!/^[a-z0-9-]{1,64}$/.test(name)) throw bad(`${MANIFEST}: name ${JSON.stringify(name)} is not a skill's name (1 to 64 of a-z, 0-9 and -)`);
  for (const entry of listed) {
    if (!pathOk(entry?.path)) throw bad(`${JSON.stringify(entry?.path)}: a skill's paths are at most ${MOST_DEPTH} parts of letters, digits, -, _ and ., none beginning with a dot`);
  }
  let author = null;
  let publicKey = null;
  const signed = read.get(SIGNATURE);
  if (signed) {
    read.delete(SIGNATURE);
    if (all[1].name !== SIGNATURE) throw bad(`${SIGNATURE} must come second`);
    if (signed.length > SIGNATURE_MAX) throw bad(`${SIGNATURE} is too large to be a signature`);
    let fields;
    try {
      fields = JSON.parse(signed.toString('utf8'));
    } catch {
      throw bad(`${SIGNATURE}: an author's signature is {"key", "public", "signature"}`);
    }
    const publicBytes = Buffer.from(fields.public ?? '', 'base64');
    if (publicBytes.length !== 32 || fingerprint(publicBytes) !== fields.key) throw bad(`${SIGNATURE}: key is not the fingerprint of public`);
    // An Ed25519 public key as SPKI DER: a fixed 12-byte prefix, then its 32 bytes.
    const key = createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), publicBytes]), format: 'der', type: 'spki' });
    if (!verify(null, manifestBytes, key, Buffer.from(fields.signature ?? '', 'base64'))) {
      throw bad(`${SIGNATURE} does not match ${MANIFEST}: the package was changed after it was signed`);
    }
    author = fields.key;
    publicKey = publicBytes.toString('base64');
  }
  const files = new Map();
  for (const entry of listed) {
    const body = read.get(`${name}/${entry.path}`);
    if (!body) throw bad(`${entry.path} is in ${MANIFEST} but not in the package`);
    if (body.length !== entry.size || sha256(body) !== entry.sha256) throw bad(`${entry.path} does not match ${MANIFEST}`);
    files.set(entry.path, body);
    read.delete(`${name}/${entry.path}`);
  }
  if (read.size) throw bad(`${[...read.keys()].sort()[0]} is in the package but not in ${MANIFEST}`);
  // `public`: the author's whole key, base64 of its 32 bytes (null when not signed).
  return { name, version, author, public: publicKey, files };
}

// At most `cap` bytes from a web stream, refused at the first byte past it:
// reading stops there rather than after the whole download.
export async function readCapped(body, cap = PACKAGE_MAX) {
  const chunks = [];
  let size = 0;
  const reader = body.getReader();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) return Buffer.concat(chunks);
    size += value.length;
    if (size > cap) {
      await reader.cancel();
      throw new LampError(`more than ${cap} bytes: a shared skill is 1 MiB at most`, { code: 'bad_request' });
    }
    chunks.push(Buffer.from(value));
  }
}

// A shared package from an https URL, on this computer; the lamp never goes
// to a URL for it. https only, no redirect to another scheme, and at most
// 1 MiB read (Fourteen's point 4 on #3504). Nothing in it is run or unpacked
// here: `openPackage` checks it in memory.
export async function fetchPackage(url, options = {}) {
  return (await fetchFrom(url, options)).data;
}

// fetchPackage, and the URL the bytes came from after any redirect, so the
// person can be shown the host (Fourteen on #3997). A URL carrying a name or
// password is refused: nothing of the person's goes to a host.
export async function fetchFrom(url, { redirects = 5 } = {}) {
  let at = new URL(url);
  if (at.username || at.password) throw new LampError(`https://${at.hostname}/…: a URL with a name or password in it is refused`, { code: 'bad_request' });
  for (let hop = 0; ; hop += 1) {
    if (at.protocol !== 'https:') {
      const why = hop ? 'a redirect away from https is refused' : 'a shared skill is fetched over https only';
      throw new LampError(`${at}: ${why}`, { code: 'bad_request' });
    }
    let response;
    try {
      response = await fetch(at, { redirect: 'manual', signal: AbortSignal.timeout(30_000) });
    } catch (error) {
      throw new LampError(`${at}: ${error.message}`, { code: 'bad_request' });
    }
    const next = response.headers.get('location');
    if (response.status >= 300 && response.status < 400 && next && hop < redirects) {
      at = new URL(next, at);
      continue;
    }
    if (!response.ok) throw new LampError(`${at}: ${response.status}`, { code: 'bad_request' });
    return { data: await readCapped(response.body), url: at.toString() };
  }
}

// The skill format's [permissions] a push may carry, onto the pairing's
// grants, as the Python SDK's push.py has them (docs/control.md).
const PERMISSION_GRANTS = { body: 'body', light: 'light', speak: 'speak' };
const PRESENCE_TOPICS = ['humans/faces/tracked', 'room/stood_up'];
// The topics a push may read, as lamp-skills' `PUSHED_TOPICS` (#3879): presence,
// and those the protocol's events take with no grant (`time`, `lamp.changed`,
// `touch`).
const PUSHED_TOPICS = [...PRESENCE_TOPICS, 'clock/local', 'voice/state', 'lamp/pause', 'controls/touch'];
const HEARD_TOPICS = ['voice/heard', 'name/heard', 'ears/classes'];
const OUTSIDE = ['actions', 'messages', 'known_faces', 'who', 'output_limits', 'propose', 'core'];

const permissionsOf = (rules) => (rules.permissions && typeof rules.permissions === 'object' && !Array.isArray(rules.permissions) ? rules.permissions : {});

// The protocol's grants a push asks, in the protocol's order, each with the
// words the owner's app shows for it: lamp-skills' `package::grants_asked`,
// the one definition the lamp checks `asks` against, in order (Eighth's
// piece B, for Ten and blue on #3931).
export const GRANTS_ASKED = {
  body: "moves the lamp's body",
  light: 'changes the light',
  speak: 'speaks and plays sounds',
  look: 'knows when someone is there, never who',
  camera: 'uses the camera when it runs',
};

// What a skill's rules ask, as grants_asked names them, in its order: body,
// light and speak as the rules say them; look for the presence topics, or
// format 2's look; camera for the picture (camera, or format 1's look:
// lamp-skills' sees_picture).
export function asksOf(rules) {
  const permissions = permissionsOf(rules);
  const format2 = (rules.format ?? 1) === 2;
  const topics = Array.isArray(permissions.topics) ? permissions.topics : [];
  const asked = {
    body: permissions.body === true,
    light: permissions.light === true,
    speak: permissions.speak === true,
    look: topics.some((topic) => PRESENCE_TOPICS.includes(topic)) || (format2 && permissions.look === true),
    camera: permissions.camera === true || (!format2 && permissions.look === true),
  };
  return Object.keys(GRANTS_ASKED).filter((grant) => asked[grant]);
}

// The code and why the lamp would refuse these rules as a push for this
// pairing's grants, or null: no_grant for a grant it lacks, refused for what
// no grant allows. The lamp checks it all again; a picture skill isn't
// refused here, the lamp holds it for the owner's yes.
export function rulesBeyond(rules, grants) {
  const kind = rules.kind ?? 'installable';
  if (kind !== 'installable') return ['refused', `kind = "${kind}": a pushed skill is installable`];
  const permissions = permissionsOf(rules);
  for (const key of OUTSIDE) {
    const given = permissions[key];
    if (given && !(Array.isArray(given) && given.length === 0)) return ['refused', `[permissions] ${key} is outside the protocol's grants: a push refuses it`];
  }
  for (const topic of Array.isArray(permissions.topics) ? permissions.topics : []) {
    if (HEARD_TOPICS.includes(topic)) return ['no_grant', `topic ${topic}: hear and sound wait for format 2's phrase and class lists`];
    if (!PUSHED_TOPICS.includes(topic)) return ['refused', `topic ${topic}: a raw topic is outside the protocol's grants`];
    if (PRESENCE_TOPICS.includes(topic) && !grants.includes('look')) return ['no_grant', `topic ${topic} needs the look grant, which this computer doesn't hold`];
  }
  for (const [key, grant] of Object.entries(PERMISSION_GRANTS)) {
    if (permissions[key] && !grants.includes(grant)) return ['no_grant', `[permissions] ${key} needs the ${grant} grant, which this computer doesn't hold`];
  }
  return null;
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
export function ustarEntry(path, data) {
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
// With `author` (a private key), the package carries that author's
// signature, to share it.
export function pack(folder, { author } = {}) {
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
  const manifestBytes = Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`);
  const data = Buffer.concat([
    ustarEntry(MANIFEST, manifestBytes),
    ...(author ? [ustarEntry(SIGNATURE, signatureFor(manifestBytes, author))] : []),
    ...[...all].map(([path, bytes]) => ustarEntry(`${name}/${path}`, bytes)),
    Buffer.alloc(1024),
  ]);
  if (data.length > PACKAGE_MAX) throw new LampError(`the package is ${data.length} bytes: a pushed skill is 1 MiB at most`, { code: 'bad_request' });
  const parts = [];
  for (let at = 0; at < data.length; at += PART_BYTES) parts.push(data.subarray(at, at + PART_BYTES).toString('base64'));
  return { name, version, data, sha256: sha256(data), parts };
}
