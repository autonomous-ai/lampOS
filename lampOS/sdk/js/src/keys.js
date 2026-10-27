// A computer's pairing with its lamps: its Ed25519 key (0600) and, per lamp,
// the client id, grants and the pin of its certificate (its full SHA-256), in
// ~/.config/lampos/ or $LAMPOS_HOME. The code itself is never kept.
import { createHash, createHmac, generateKeyPairSync, createPrivateKey, createPublicKey, sign } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const b64 = (bytes) => Buffer.from(bytes).toString('base64');
// Strict base64; with `length`, exactly that many bytes.
export function unb64(text, length) {
  if (typeof text !== 'string' || !/^[A-Za-z0-9+/]*={0,2}$/.test(text) || text.length % 4) throw new Error('not base64');
  const bytes = Buffer.from(text, 'base64');
  if (length !== undefined && bytes.length !== length) throw new Error(`${bytes.length} bytes, not ${length}`);
  return bytes;
}

// Each proof names what it is for, so one made for pairing can never stand as
// a hello's (docs/control.md).
export const PAIR = Buffer.from('lampos/v1/pair\0', 'ascii');
export const HELLO = Buffer.from('lampos/v1/hello\0', 'ascii');

// Pairing's proof: HMAC-SHA256 keyed with the code's ASCII digits, over PAIR,
// the pin's 32 bytes, the 32 key bytes, then the welcome's 32 nonce bytes
// (docs/control.md). With the pin inside, a proof relayed through a host in
// the middle fails at the real lamp.
export const proof = (code, pin, key, nonce) =>
  createHmac('sha256', Buffer.from(code, 'ascii')).update(Buffer.concat([PAIR, pin, key, nonce])).digest();

// The certificate's pin: its full SHA-256, 64 lowercase hex, as the owner's
// app shows it with the pairing code.
export const pinOf = (der) => createHash('sha256').update(der).digest('hex');
export const isPin = (text) => typeof text === 'string' && /^[0-9a-f]{64}$/.test(text);

export function home() {
  return process.env.LAMPOS_HOME || join(homedir(), '.config', 'lampos');
}

export class Keyring {
  constructor(folder = home()) { this.folder = folder; }

  // The folder, this user's alone (0700) whatever the umask: anyone who could
  // write in it could swap a lamp's host and pin, or the key (harness-reviewer's
  // P2 on #3761).
  ownFolder() {
    mkdirSync(this.folder, { recursive: true, mode: 0o700 });
    chmodSync(this.folder, 0o700);
  }

  key() { return this.keyFile('key', true); }

  // The key this person signs their skills with (`lamp skill keygen`), kept
  // apart from the pairing key: revoking a pairing never changes who wrote a
  // skill. Undefined if it hasn't been made.
  authorKey({ make = false } = {}) { return this.keyFile('author.pem', make); }

  keyFile(name, make) {
    const path = join(this.folder, name);
    if (existsSync(path)) return createPrivateKey(readFileSync(path));
    if (!make) return undefined;
    const { privateKey } = generateKeyPairSync('ed25519');
    this.ownFolder();
    writeFileSync(path, privateKey.export({ type: 'pkcs8', format: 'pem' }), { mode: 0o600, flag: 'wx' });
    return privateKey;
  }

  // The raw 32 bytes of the public key.
  public() {
    const der = createPublicKey(this.key()).export({ type: 'spki', format: 'der' });
    return der.subarray(der.length - 32);
  }

  // Hello's signature: over HELLO, then the welcome's nonce.
  hello(nonce) { return sign(null, Buffer.concat([HELLO, nonce]), this.key()); }

  pairings() {
    const path = join(this.folder, 'lamps.json');
    return existsSync(path) ? JSON.parse(readFileSync(path, 'utf8')) : {};
  }

  remember(pairing) {
    const pairings = { ...this.pairings(), [pairing.lamp]: pairing };
    this.ownFolder();
    const path = join(this.folder, 'lamps.json');
    rmSync(`${path}.new`, { force: true });
    writeFileSync(`${path}.new`, JSON.stringify(pairings, null, 2), { mode: 0o600, flag: 'wx' });
    renameSync(`${path}.new`, path);
  }

  // The pairing with `lamp`, or the only one there is.
  pairing(lamp) {
    const pairings = this.pairings();
    if (lamp) return pairings[lamp] ?? null;
    const all = Object.values(pairings);
    return all.length === 1 ? all[0] : null;
  }
}
