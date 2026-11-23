// The Skills Store's developer API, as the Python SDK's lampos/store.py
// calls it: sign in with an emailed code, upload an app (format 2, docs/apps.md),
// read its review, ask for it to go public (#7415; Backend3's spec of 20 Nov).
// Every host, path and error code is in this file's constants.
//
//   await sendLoginCode('dev@example.com');
//   const token = await signIn('dev@example.com', '123456'); // kept, 0600
//   const app = await submitApp(readFileSync('gym-coach-1.0.0.tar'), { title: 'Gym coach' });
//   (await submissions(app.id)).review_status;                // pending, approved or rejected
//
// The token is kept in ~/.config/lampos/store.json ($LAMPOS_HOME), the same
// file `lamp login` writes, one per account host; LAMP_STORE_TOKEN wins. No
// password is ever asked or sent. Check and pack an app with the Python
// SDK's `lamp app check` and `lamp app pack`; `submitApp` refuses here only
// what is plainly not an app's package, and the Store checks the rest.
import { chmodSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { LampError } from './errors.js';
import { home } from './keys.js';

export const PRODUCTION = Object.freeze({
  account: 'https://apiv2.autonomous.ai/api/v1/customers',
  store: 'https://campaign-api.autonomous.ai/api/v1/skills-store/developer',
});
export const STAGING = Object.freeze({
  account: 'https://apiv2.staging.autonomousdev.xyz/api/v1/customers',
  store: 'https://campaign-api.staging.autonomousdev.xyz/api/v1/skills-store/developer',
});
export const PATHS = Object.freeze({
  sendCode: '/send-login-verification',
  signIn: '/sign-in',
  apps: '/apps',
  app: (id) => `/apps/${encodeURIComponent(id)}`,
  requestPublic: (id) => `/apps/${encodeURIComponent(id)}/request-public`,
});
export const LOCATION = 'en-US';
export const MOST_PACKAGE_BYTES = 18 * 1024 * 1024;
export const MOST_ART_BYTES = 2 * 1024 * 1024;
const TOKEN_FILE = 'store.json';

// Every error code the Store's spec lists, with what the developer can do (the Python SDK's words).
export const STORE_ERRORS = Object.freeze({
  authentication_error: 'not signed in, or the sign-in ran out: run `lamp login` again',
  device_key_refused: "that is a lamp's key, not a developer's sign-in: run `lamp login` with your Autonomous account",
  developer_app_only: "this is a skill (format 1): a skill goes on through the lamp (`lamp skill push`), not the Store's app upload",
  lampos_package_invalid: "the Store's check refused the package",
  skill_name_exists: 'that app name belongs to another account: change `name` in app.json',
  app_not_found: 'no app of yours has that id (`lamp app status` lists yours)',
  app_not_approved: 'only an approved app can ask to go public: wait for the review',
  app_already_public: 'this app is public already',
  developer_daily_limit: "today's 10 accepted uploads for this account are used: try again tomorrow",
  developer_attempt_limit: "today's 30 upload attempts for this account are used: try again tomorrow",
  developer_public_limit: "today's 3 asks to go public for this account are used: try again tomorrow",
});
export const NOT_SIGNED_IN = 'not signed in to the Store: run `lamp login` (or set LAMP_STORE_TOKEN)';

export class StoreError extends LampError {
  constructor(why, { code = 'store', status } = {}) {
    super(why, { code });
    this.name = 'StoreError';
    this.status = status;
  }
}

// Production by default; `staging`, then the URLs given or in LAMP_STORE_URL and LAMP_ACCOUNT_URL, change it.
export function endpoints({ staging = false, store, account } = {}) {
  const base = staging ? STAGING : PRODUCTION;
  const trim = (url) => url.replace(/\/+$/, '');
  return { account: trim(account || process.env.LAMP_ACCOUNT_URL || base.account), store: trim(store || process.env.LAMP_STORE_URL || base.store) };
}

const tokenPath = () => join(home(), TOKEN_FILE);
function tokens() {
  try {
    const kept = JSON.parse(readFileSync(tokenPath(), 'utf8'));
    return kept && typeof kept === 'object' ? kept : {};
  } catch {
    return {};
  }
}

// The kept tokens, written whole to a 0600 file in a 0700 folder.
function writeKept(kept) {
  mkdirSync(home(), { recursive: true, mode: 0o700 });
  const temporary = `${tokenPath()}.tmp`;
  writeFileSync(temporary, JSON.stringify(kept, null, 2), { mode: 0o600 });
  chmodSync(temporary, 0o600);
  renameSync(temporary, tokenPath());
  return tokenPath();
}

// Keep `token` for `where`'s account host.
export function saveToken(token, where = endpoints(), email = null) {
  return writeKept({ ...tokens(), [where.account]: { token, email, store: where.store } });
}

// Drop the token kept for `where`; whether there was one.
export function forgetToken(where = endpoints()) {
  const kept = tokens();
  if (!(where.account in kept)) return false;
  delete kept[where.account];
  if (Object.keys(kept).length) writeKept(kept);
  else rmSync(tokenPath(), { force: true });
  return true;
}

// The token for `where`: LAMP_STORE_TOKEN, or the one kept for that account host.
export function keptToken(where = endpoints()) {
  if (process.env.LAMP_STORE_TOKEN) return process.env.LAMP_STORE_TOKEN;
  const entry = tokens()[where.account];
  return entry && typeof entry.token === 'string' ? entry.token : null;
}

// A PNG's [width, height] from its IHDR, or null.
export function pngSize(bytes) {
  const b = Buffer.from(bytes);
  if (b.length < 24 || !b.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])) || b.toString('ascii', 12, 16) !== 'IHDR') return null;
  return [b.readUInt32BE(16), b.readUInt32BE(20)];
}

function artError(what, bytes, [width, height]) {
  const size = pngSize(bytes);
  if (!size) return `${what}: not a PNG`;
  if (size[0] !== width || size[1] !== height) return `${what}: ${size[0]}x${size[1]}; the Store takes exactly ${width}x${height}`;
  if (bytes.length > MOST_ART_BYTES) return `${what}: ${bytes.length} bytes; the Store takes 2 MB at most`;
  return null;
}

// Why these bytes are plainly not an app's unsigned package, or null: the
// first tar entry is `lamp-skill.json` with format 2 and kind "app".
export function packageError(bytes) {
  const b = Buffer.from(bytes);
  if (b.length > MOST_PACKAGE_BYTES) return `the package is ${b.length} bytes; the Store takes 18 MiB at most`;
  if (b[0] === 0x1f && b[1] === 0x8b) return 'a gzip: a package is a plain tar';
  if (b.length < 1024 || b.toString('ascii', 257, 262) !== 'ustar') return 'not a tar';
  if (b.toString('utf8', 0, 100).replace(/\0.*$/s, '') !== 'lamp-skill.json') return 'no lamp-skill.json first: not a packed app (`lamp app pack`)';
  const size = parseInt(b.toString('ascii', 124, 136).replace(/\0.*$/s, '').trim() || '0', 8);
  let manifest;
  try {
    manifest = JSON.parse(b.toString('utf8', 512, 512 + size));
  } catch (error) {
    return `lamp-skill.json: ${error.message}`;
  }
  if (manifest.format === 1) return "lamp-skill.json: format 1 is a skill: a skill goes on through the lamp (`lamp skill push`), not the Store's app upload";
  if (manifest.format !== 2 || manifest.kind !== 'app') return 'lamp-skill.json: not an app (format 2, kind "app")';
  return null;
}

async function call(method, url, { body, token, auth = true, json } = {}) {
  const headers = { Accept: 'application/json', Location: process.env.LAMP_STORE_LOCATION || LOCATION };
  if (auth) {
    if (!token) throw new StoreError(NOT_SIGNED_IN, { code: 'authentication_error' });
    headers.Authorization = `Bearer ${token}`;
  }
  if (json !== undefined) {
    headers['Content-Type'] = 'application/json';
    body = JSON.stringify(json);
  }
  let answer;
  try {
    answer = await fetch(url, { method, headers, body });
  } catch (error) {
    throw new StoreError(`no answer from ${new URL(url).host} (${error.cause?.code ?? error.message})`, { code: 'unreachable' });
  }
  let said;
  try {
    said = await answer.json();
  } catch {
    throw new StoreError(`the Store answered HTTP ${answer.status} with no JSON`, { status: answer.status });
  }
  if (said && said.status === 1) return said.data;
  const code = said?.error_code || (answer.status === 401 ? 'authentication_error' : 'store');
  const message = said?.error_message || said?.message || `HTTP ${answer.status}`;
  const known = STORE_ERRORS[code];
  const why = code === 'lampos_package_invalid' ? `${message} (${known})` : known ? `${known} (the Store: ${message})` : String(message);
  throw new StoreError(why, { code, status: answer.status });
}

const options = (opts = {}) => {
  const where = opts.where ?? endpoints(opts);
  return { where, token: opts.token ?? keptToken(where) };
};

// Have the account email a sign-in code.
export async function sendLoginCode(email, opts = {}) {
  const { where } = options(opts);
  await call('POST', where.account + PATHS.sendCode, { json: { email }, auth: false });
}

// The account's token for the emailed code; kept for `lamp` unless `{ keep: false }`.
export async function signIn(email, otp, opts = {}) {
  const { where } = options(opts);
  const data = await call('POST', where.account + PATHS.signIn, { json: { grant_type: 'otp', email, otp }, auth: false });
  const token = data?.access_token;
  if (typeof token !== 'string' || !token) throw new StoreError('the sign-in answered no access_token', { code: 'authentication_error' });
  if (opts.keep !== false) saveToken(token, where, email);
  return token;
}

// Upload an unsigned format-2 tar (bytes) for review: the Store's app,
// `review_status` "pending", hidden. `icon` (PNG 1024x1024) and `cover`
// (PNG 1600x900), each 2 MB at most, and the listing are checked here first.
export async function submitApp(tar, { icon, cover, title, description, category, filename = 'app.tar', ...opts } = {}) {
  const { where, token } = options(opts);
  if (!token) throw new StoreError(NOT_SIGNED_IN, { code: 'authentication_error' });
  const refused = packageError(tar)
    ?? (icon && artError('icon', icon, [1024, 1024]))
    ?? (cover && artError('cover', cover, [1600, 900]))
    ?? (title !== undefined && !(title.length > 0 && title.length <= 100) ? 'title: 1 to 100 characters' : null)
    ?? (description !== undefined && description.length > 1000 ? 'description: at most 1000 characters of plain text' : null);
  if (refused) throw new StoreError(refused, { code: 'bad_request' });
  const form = new FormData();
  form.append('file', new Blob([tar], { type: 'application/x-tar' }), filename);
  if (icon) form.append('icon', new Blob([icon], { type: 'image/png' }), 'icon.png');
  if (cover) form.append('cover', new Blob([cover], { type: 'image/png' }), 'cover.png');
  for (const [key, value] of Object.entries({ title, description, category })) if (value !== undefined) form.append(key, value);
  return call('POST', where.store + PATHS.apps, { body: form, token });
}

// With an id, that app of yours; without, `{ data: [...], total }`, a page of your apps.
export async function submissions(id, { page = 1, limit = 50, ...opts } = {}) {
  const { where, token } = options(opts);
  if (id !== undefined && id !== null) return call('GET', where.store + PATHS.app(id), { token });
  const data = await call('GET', `${where.store}${PATHS.apps}?page=${page}&limit=${limit}`, { token });
  return { data: data?.data ?? [], total: data?.total ?? 0 };
}

// Ask the reviewers to make an approved app public; asking again changes nothing.
export async function requestPublic(id, opts = {}) {
  const { where, token } = options(opts);
  return call('POST', where.store + PATHS.requestPublic(id), { token });
}
