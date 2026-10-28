// Shared-skill indexes, on this computer only (decision 4, #3997): a signed
// list of signed packages to fetch from, as the Python SDK's lampos.index has
// them. An index grants nothing and nothing of it runs on the lamp; each
// package it lists is checked against its entry, then installed as any
// shared package is (`lamp.installPackage`), and the lamp checks it again.
//
// lamp-index.json, format 1, at most 1 MiB:
//   {"format": 1, "index": "<the listing, a JSON string>", "key": FPR16,
//    "public": "<base64, 32 bytes>", "signature": "<base64 Ed25519 over the UTF-8 bytes of index>"}
//   listing = {"format": 1, "name", "seq", "skills": [{"name", "version",
//              "author": FPR16, "public": "<base64, 32 bytes>", "sha256", "size", "url", "asks"}]}
// The publisher's key is pinned at `index add`, and a listing never goes back
// to a lower seq. "published by key FPR16, not the Store", never "verified".
// What the person is shown at install comes from the package itself, never
// from its entry. Nothing here runs in the background: an index is fetched
// when the person runs browse or install, never polled.
import { createHash, createPublicKey, sign, verify } from 'node:crypto';
import { chmodSync, mkdirSync, readFileSync, renameSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { parse as parseToml } from 'smol-toml';
import { LampError } from './errors.js';
import { PACKAGE_MAX, asksOf, fetchPackage, fingerprint, openPackage, rawPublic } from './push.js';

export const FORMAT = 1;
const NAME = /^[a-z0-9-]{1,64}$/;
export const MOST_SKILLS = 500;
const ENTRY_KEYS = new Set(['name', 'version', 'author', 'public', 'sha256', 'size', 'url', 'asks']);
const LISTING_KEYS = new Set(['format', 'name', 'seq', 'skills']);

// An https URL with a host and no name or password in it.
export function urlOk(url) {
  if (typeof url !== 'string' || url.length > 2048) return false;
  try {
    const at = new URL(url);
    return at.protocol === 'https:' && !!at.hostname && !at.username && !at.password;
  } catch {
    return false;
  }
}

export const hostOf = (url) => { try { return new URL(url).hostname; } catch { return url; } };
const bad = (why) => new LampError(`not a skill index: ${why}`, { code: 'bad_request' });
const refused = (why) => new LampError(why, { code: 'refused' });
const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const asksOfPackage = (opened) => asksOf(parseToml((opened.files.get('skill.toml') ?? Buffer.alloc(0)).toString('utf8')));

function entryOf(raw) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw bad('an entry is an object');
  const { name, version, author, public: publicKey, sha256: sha, size, url, asks } = raw;
  if (typeof name !== 'string' || !NAME.test(name)) throw bad(`${JSON.stringify(name)} is not a skill's name (1 to 64 of a-z, 0-9 and -)`);
  // A later format's field is never silently passed over by this one (Fourteen on #3997).
  const unknown = Object.keys(raw).filter((key) => !ENTRY_KEYS.has(key)).sort();
  if (unknown.length) throw bad(`${name}: ${unknown[0]} is not a field of a format ${FORMAT} entry`);
  if (!urlOk(url)) throw bad(`${name}: its url is fetched over https only, with a host and no name or password in it`);
  if (typeof author !== 'string' || !/^[0-9a-f]{16}$/.test(author)) throw bad(`${name}: author is a key's fingerprint, 16 hex: an index lists signed packages only`);
  const publicBytes = typeof publicKey === 'string' ? Buffer.from(publicKey, 'base64') : Buffer.alloc(0);
  if (publicBytes.length !== 32 || publicBytes.toString('base64') !== publicKey || fingerprint(publicBytes) !== author) {
    throw bad(`${name}: public is the author's whole key, whose fingerprint is author`);
  }
  if (typeof sha !== 'string' || !/^[0-9a-f]{64}$/.test(sha)) throw bad(`${name}: sha256 is 64 lowercase hex`);
  if (!Number.isInteger(size) || size < 1 || size > PACKAGE_MAX) throw bad(`${name}: size is 1 to ${PACKAGE_MAX} bytes`);
  if (typeof version !== 'string' || !version || version.length > 64) throw bad(`${name}: version is 1 to 64 characters`);
  if (!Array.isArray(asks) || asks.length > 8 || !asks.every((ask) => typeof ask === 'string' && ask.length <= 16)) throw bad(`${name}: asks is a list of grant names`);
  return { name, version, author, public: publicKey, sha256: sha, size, url, asks };
}

// Check an index: its signature over the listing's exact bytes, by the key
// it carries, and that key the `pinned` one (base64) when there is one; a seq
// no lower than `lastSeq`; and each entry's shape.
export function openIndex(data, { pinned, lastSeq } = {}) {
  if (data.length > PACKAGE_MAX) throw bad(`more than ${PACKAGE_MAX} bytes`);
  let outer;
  let publicBytes;
  let signature;
  try {
    outer = JSON.parse(Buffer.from(data).toString('utf8'));
    publicBytes = Buffer.from(outer.public, 'base64');
    signature = Buffer.from(outer.signature, 'base64');
    if (typeof outer.index !== 'string' || typeof outer.key !== 'string') throw new Error();
  } catch {
    throw bad('it is {"format", "index", "key", "public", "signature"}, signed by its publisher');
  }
  if (outer.format !== FORMAT) throw bad(`format ${JSON.stringify(outer.format)} is not one this SDK reads (${FORMAT})`);
  if (publicBytes.length !== 32 || fingerprint(publicBytes) !== outer.key) throw bad('key is not the fingerprint of public');
  const key = createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), publicBytes]), format: 'der', type: 'spki' });
  if (!verify(null, Buffer.from(outer.index, 'utf8'), key, signature)) throw bad('the signature does not match: the index was changed after it was signed');
  let listing;
  try {
    listing = JSON.parse(outer.index);
  } catch (error) {
    throw bad(`its listing: ${error.message}`);
  }
  const { name, seq, skills } = listing ?? {};
  if (listing?.format !== FORMAT || !Array.isArray(skills)) throw bad(`its listing is format ${FORMAT}, with a list of skills`);
  const unknownKeys = Object.keys(listing).filter((key) => !LISTING_KEYS.has(key)).sort();
  if (unknownKeys.length) throw bad(`${unknownKeys[0]} is not a field of a format ${FORMAT} listing`);
  if (skills.length > MOST_SKILLS) throw bad(`more than ${MOST_SKILLS} skills`);
  if (typeof name !== 'string' || !NAME.test(name)) throw bad(`${JSON.stringify(name)} is not an index's name (1 to 64 of a-z, 0-9 and -)`);
  const publicB64 = publicBytes.toString('base64');
  if (pinned !== undefined && publicB64 !== pinned) {
    throw refused(`index ${name} is published by key ${fingerprint(Buffer.from(pinned, 'base64'))} here, and this one by key ${outer.key}: remove it and add it again to trust another key`);
  }
  if (!Number.isInteger(seq) || seq < 0) throw bad('seq is a whole number');
  if (lastSeq !== undefined && seq < lastSeq) throw refused(`index ${name} went back from seq ${lastSeq} to ${seq}: an older copy is never taken`);
  const entries = skills.map(entryOf);
  if (new Set(entries.map((entry) => entry.name)).size !== entries.length) throw bad("a skill's name is listed twice");
  return { name, seq, public: publicB64, publisher: outer.key, skills: entries };
}

// An index of these signed packages, each at urlBase + NAME-VERSION.tar, its
// entries taken from the packages themselves, signed by `publisher`.
export function makeIndex(name, packages, urlBase, seq, publisher) {
  if (!urlOk(urlBase)) throw new LampError("an index's packages are fetched over https only: --url-base https://…", { code: 'bad_request' });
  const base = urlBase.endsWith('/') ? urlBase : `${urlBase}/`;
  const skills = packages.map((data) => {
    const opened = openPackage(data);
    if (!opened.author) throw new LampError(`${opened.name}: an index lists signed packages only (lamp skill pack --sign)`, { code: 'bad_request' });
    return {
      name: opened.name, version: opened.version, author: opened.author, public: opened.public, sha256: sha256(data), size: data.length,
      url: `${base}${opened.name}-${opened.version}.tar`, asks: asksOfPackage(opened),
    };
  });
  const listing = JSON.stringify({ format: FORMAT, name, seq, skills });
  const publicBytes = rawPublic(publisher);
  const data = Buffer.from(`${JSON.stringify({
    format: FORMAT, index: listing, key: fingerprint(publicBytes), public: publicBytes.toString('base64'),
    signature: sign(null, Buffer.from(listing, 'utf8'), publisher).toString('base64'),
  })}\n`);
  openIndex(data); // what it writes, it reads
  return data;
}

// The fetched package against its entry: the same bytes (SHA-256 and size),
// signed by the entry's author, asking what the entry says.
export function checkPackage(entry, data) {
  if (data.length !== entry.size || sha256(data) !== entry.sha256) throw refused(`${entry.name}: the package fetched is not the one the index lists (SHA-256)`);
  const opened = openPackage(data);
  if (opened.name !== entry.name || opened.version !== entry.version) throw refused(`${entry.name}: the package is ${opened.name} ${opened.version}, not the index's`);
  // The whole key, not the 16-hex fingerprint: a look-alike key is caught on the first install (Ten on #3997).
  if (opened.public !== entry.public) throw refused(`${entry.name}: signed by ${opened.author ? `key ${opened.author}` : 'no key'}, and the index says key ${entry.author}`);
  const asks = asksOfPackage(opened);
  if (asks.join(',') !== entry.asks.join(',')) throw refused(`${entry.name}: it asks [${asks.join(', ')}], and the index says [${entry.asks.join(', ')}]`);
  return opened;
}

// The indexes this computer trusts to list skills, in `folder`'s
// indexes.json (the Python SDK's file): each one's URL, its publisher's
// pinned key and the last seq seen.
export class Indexes {
  constructor(folder) { this.path = join(folder, 'indexes.json'); this.folder = folder; }

  all() {
    try {
      return JSON.parse(readFileSync(this.path, 'utf8'));
    } catch (error) {
      if (error.code === 'ENOENT') return {};
      throw error;
    }
  }

  write(kept) {
    mkdirSync(this.folder, { recursive: true, mode: 0o700 });
    // 0600 like author.pem, written aside and renamed over it.
    const part = `${this.path}.part`;
    writeFileSync(part, `${JSON.stringify(kept, null, 2)}\n`, { mode: 0o600 });
    chmodSync(part, 0o600);
    renameSync(part, this.path);
  }

  // Keep `listing` from `url`; returns the URL it was kept from before, when
  // that was another one.
  keep(url, listing) {
    const kept = this.all();
    const held = kept[listing.name];
    if (held && held.public !== listing.public) throw refused(`an index named ${listing.name} is kept already, by another key: remove it first`);
    kept[listing.name] = { url, public: listing.public, key: listing.publisher, seq: listing.seq };
    this.write(kept);
    return held && held.url !== url ? held.url : null;
  }

  remove(name) {
    const kept = this.all();
    if (!kept[name]) throw new LampError(`no index named ${name} here`, { code: 'bad_request' });
    delete kept[name];
    this.write(kept);
  }
}

// An index or package from an https URL (the SDK's capped fetch) or a file
// here, 1 MiB at most either way.
export async function readSource(source, fetch = fetchPackage) {
  if (source.includes('://')) return Buffer.from(await fetch(source));
  if (statSync(source).size > PACKAGE_MAX) throw new LampError(`${source}: more than ${PACKAGE_MAX} bytes: a shared skill is 1 MiB at most`, { code: 'bad_request' });
  return readFileSync(source);
}

// A kept index fetched again: held to its pinned key and last seq, which is
// moved on.
export async function readKept(indexes, name, fetch) {
  return readKeptBytes(indexes, name, await readSource(indexes.all()[name].url, fetch));
}

function readKeptBytes(indexes, name, data) {
  const held = indexes.all()[name];
  const listing = openIndex(data, { pinned: held.public, lastSeq: held.seq });
  if (listing.name !== name) throw refused(`the index at ${held.url} is now named ${listing.name}, not ${name}`);
  indexes.keep(held.url, listing);
  return listing;
}

// A kept index's bytes, or why they couldn't be fetched (a host that's off, a
// gist that's gone): that index is unread, the others go on.
async function fetched(indexes, name, fetch) {
  try {
    return await readSource(indexes.all()[name].url, fetch);
  } catch (error) {
    return error.message;
  }
}

// Every kept index fetched again, each held to its pinned key and last seq,
// and those that couldn't be fetched, with why (Ten on #3997). One that is
// refused (another key, an older seq, changed bytes) throws: that is the
// warning the pin is for, never skipped.
export async function refresh(indexes, fetch) {
  const listings = [];
  const unread = [];
  for (const name of Object.keys(indexes.all()).sort()) {
    const data = await fetched(indexes, name, fetch);
    if (typeof data === 'string') unread.push([name, data]);
    else listings.push(await readKeptBytes(indexes, name, data));
  }
  return { listings, unread };
}

// `name` looked up in the kept indexes (or `fromIndex` alone), its package
// fetched from the entry's URL and checked against the entry.
export async function find(indexes, name, { fromIndex, fetch } = {}) {
  const kept = indexes.all();
  if (fromIndex !== undefined && !kept[fromIndex]) throw new LampError(`no index named ${fromIndex} here: lamp skill index add URL`, { code: 'bad_request' });
  const found = [];
  for (const indexName of fromIndex !== undefined ? [fromIndex] : Object.keys(kept).sort()) {
    const data = await fetched(indexes, indexName, fetch);
    // Without it, a second listing of the name can't be ruled out.
    if (typeof data === 'string') throw new LampError(`index ${indexName} could not be read (${data}): say which with --from`, { code: 'bad_request' });
    const listing = readKeptBytes(indexes, indexName, data);
    for (const entry of listing.skills) if (entry.name === name) found.push({ listing, entry });
  }
  if (!found.length) throw new LampError(`${name} is in no index kept here`, { code: 'bad_request' });
  if (found.length > 1) throw new LampError(`${name} is listed by ${found.map(({ listing }) => listing.name).join(', ')}: say which with --from`, { code: 'bad_request' });
  const { listing, entry } = found[0];
  const data = await readSource(entry.url, fetch);
  checkPackage(entry, data);
  return { listing, entry, data };
}

// Whether dotted-number `version` is older than `than`; null when either isn't one.
export function older(version, than) {
  const parts = (text) => {
    const core = text.split('-')[0].split('+')[0];
    return /^\d+(\.\d+)*$/.test(core) ? core.split('.').map(Number) : null;
  };
  const a = parts(version);
  const b = parts(than);
  if (!a || !b) return null;
  for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
    if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) < (b[i] ?? 0);
  }
  return false;
}
