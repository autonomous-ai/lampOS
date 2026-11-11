// The virtual desktop lamp in one command: starts `lamp-sim` (the real
// lamp-control and lamp-body on a simulated body, docs/simulator-lamp.md),
// pairs this computer with it in a keyring of its own, and opens the twin
// on it. The twin, the SDK and lamp-mcp then see a lamp like any other.
//
//   node examples/twin/virtual-lamp.js [--lamp-sim PATH] [--bin-dir DIR] [--dir DIR]
//        [--port N] [--twin-port N] [--glb PATH] [--camera|--camera=always] [--microphone] [--no-open] [--browser] [--try]
//
// --lamp-sim PATH   the lamp-sim binary (default: `lamp-sim` on the PATH)
// --bin-dir DIR     where lamp-body and lamp-control are (default: beside lamp-sim)
// --dir DIR         the lamp's folder, short and private (default ~/.lampsim/virtual)
// --port N          the lamp's door, on 127.0.0.1 (default 7731)
// --twin-port N     the twin's page (default 8737)
// --glb PATH        the rigged CAD for the twin (default: the repository's, which needs `git lfs pull`)
// --try             the try-it app (see try.js): the page on the LAN too, with a control panel, stimuli and
//                   an event log; no camera or microphone unless --camera / --microphone is given
// --browser         open the default browser's tab, not a window of its own (the default is a
//                   chromeless Chrome/Chromium/Edge window, the browser's own tab when none is installed)
// It pairs a fresh keyring under DIR/sdk each start (the lamp makes a new
// certificate then); to drive the lamp from your own program too, run it with
// LAMPOS_HOME=DIR/sdk. Ctrl-C stops the twin and the lamp.
import { spawn } from 'node:child_process';
import { chmodSync, closeSync, existsSync, mkdirSync, openSync, readSync, rmSync } from 'node:fs';
import { createConnection, createServer } from 'node:net';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Keyring, Lamp } from '../../src/index.js';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const GRANTS = 'body,light,speak,look,hear,sound,air,device,camera,microphone';
// The try-it app's lamp has no camera or microphone grant unless that is asked for (`--camera`, `--microphone`):
// a picture of the room would sit on the LAN page.
const TRY_GRANTS = 'body,light,speak,look,hear,sound,air,device';
export function grantsFor(flags, tryApp) {
  if (!tryApp) return GRANTS;
  const camera = flags.some((f) => f === '--camera' || f.startsWith('--camera='));
  return [TRY_GRANTS, ...(camera ? ['camera'] : []), ...(flags.includes('--microphone') ? ['microphone'] : [])].join(',');
}

export function parseArguments(args) {
  const out = { flags: [] };
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--no-open' || arg === '--browser' || arg === '--try' || arg === '--microphone' || arg === '--camera' || arg.startsWith('--camera=')) out.flags.push(arg);
    else if (arg.startsWith('--') && args[i + 1] !== undefined) out[arg.slice(2)] = args[++i];
    else throw new Error(`unknown argument ${arg}`);
  }
  return out;
}

// The pairing code and pin, from what `lamp-sim code` printed.
export function pairingFrom(text) {
  const given = JSON.parse(text.trim().split('\n').pop());
  if (typeof given.code !== 'string' || typeof given.pin !== 'string') throw new Error('no pairing code in what lamp-sim printed');
  return { code: given.code, pin: given.pin };
}

// Whether nothing listens on `port` of 127.0.0.1 now.
export const portFree = (port) => new Promise((resolve) => {
  const probe = createServer();
  probe.once('error', () => resolve(false));
  probe.listen(port, '127.0.0.1', () => probe.close(() => resolve(true)));
});

async function listening(port, timeoutMs) {
  const until = Date.now() + timeoutMs;
  while (Date.now() < until) {
    const open = await new Promise((resolve) => {
      const socket = createConnection({ port, host: '127.0.0.1' }, () => { socket.destroy(); resolve(true); });
      socket.on('error', () => resolve(false));
    });
    if (open) return;
    await sleep(250);
  }
  throw new Error(`the lamp did not open its door on 127.0.0.1:${port} in ${timeoutMs / 1000} s`);
}

function outputOf(program, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(program, args, { stdio: ['ignore', 'pipe', 'inherit'] });
    let out = '';
    child.stdout.on('data', (chunk) => { out += chunk; });
    child.on('error', reject);
    child.on('close', (code) => (code === 0 ? resolve(out) : reject(new Error(`${program} ${args[0]} exited ${code}`))));
  });
}

// A chromeless window (`--app`) when a Chromium-based browser is installed, else the default browser.
const APP_BROWSERS = {
  linux: ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'microsoft-edge'],
  darwin: ['Google Chrome', 'Chromium', 'Microsoft Edge'],
  win32: ['chrome', 'msedge'],
};

export function appCommands(url, platform = process.platform) {
  const flags = [`--app=${url}`, '--window-size=1100,720'];
  const browsers = APP_BROWSERS[platform] ?? [];
  if (platform === 'darwin') return browsers.map((name) => ['open', ['-na', name, '--args', ...flags]]);
  if (platform === 'win32') return browsers.map((name) => ['cmd', ['/c', 'start', '', name, ...flags]]);
  return browsers.map((name) => [name, flags]);
}

function spawned(program, args) {
  return new Promise((resolve) => {
    const child = spawn(program, args, { stdio: 'ignore', detached: true });
    child.on('error', () => resolve(false));
    // A program that is not there fails at once; one that is keeps running (or hands over and exits 0).
    child.on('exit', (code) => resolve(code === 0));
    setTimeout(() => { child.unref(); resolve(true); }, 800);
  });
}

async function openBrowser(url, { window = true } = {}) {
  if (window) for (const [program, args] of appCommands(url)) if (await spawned(program, args)) return;
  const [program, ...rest] = process.platform === 'darwin' ? ['open'] : process.platform === 'win32' ? ['cmd', '/c', 'start', ''] : ['xdg-open'];
  await spawned(program, [...rest, url]);
}

// Whether `path` is a glTF file (a clone without `git lfs pull` has a pointer text in its place).
export function riggedCad(path) {
  if (!existsSync(path)) return false;
  const fd = openSync(path, 'r');
  try { const head = Buffer.alloc(4); readSync(fd, head, 0, 4, 0); return head.toString('latin1') === 'glTF'; } finally { closeSync(fd); }
}

export async function start(options, log = console.log) {
  // The twin needs the rigged CAD: say so before anything starts, not as the twin's exit code.
  const cad = options.glb ?? fileURLToPath(new URL('../../../../../robots/lamp/hardware/cad/glb/lamp.glb', import.meta.url));
  if (!riggedCad(cad)) throw new Error(`${cad}: no rigged CAD here (a clone needs \`git lfs pull\`); give --glb PATH`);
  const lampSim = options['lamp-sim'] ?? 'lamp-sim';
  const dir = options.dir ?? join(homedir(), '.lampsim', 'virtual');
  const port = Number(options.port ?? 7731);
  const binDir = options['bin-dir'] ? ['--bin-dir', options['bin-dir']] : [];
  // Both ports are taken before anything starts, so a busy one says so instead of ending in a stack.
  for (const [what, number] of [['the lamp\'s door', port], ['the twin\'s page', Number(options['twin-port'] ?? 8737)]]) {
    if (!(await portFree(number))) throw new Error(`${what} port ${number} is in use on 127.0.0.1: give another ${what.includes('door') ? '--port' : '--twin-port'}`);
  }
  mkdirSync(dir, { recursive: true, mode: 0o700 });
  chmodSync(dir, 0o700);
  const tryApp = options.flags.includes('--try');
  const children = [];
  const stop = () => { for (const child of children) child.kill('SIGTERM'); };
  try {
    const lamp = spawn(lampSim, ['run', dir, ...binDir, '--listen', `127.0.0.1:${port}`, ...options.flags.filter((f) => f !== '--no-open' && f !== '--browser' && f !== '--try')], { stdio: ['ignore', 'inherit', 'inherit'] });
    children.push(lamp);
    const ended = new Promise((_, reject) => lamp.on('close', (code) => reject(new Error(`lamp-sim ended (${code})`))));
    ended.catch(() => {});
    await Promise.race([listening(port, 60_000), ended]);
    // lamp-control answers on its admin socket a moment after its door opens.
    const given = pairingFrom(await outputOf(lampSim, ['code', dir, ...binDir, '--grants', grantsFor(options.flags, tryApp)]));
    const home = join(dir, 'sdk');
    rmSync(home, { recursive: true, force: true });
    const pairing = await Lamp.pair(given.code, '127.0.0.1', { pin: given.pin, port, name: 'virtual lamp viewer', keyring: new Keyring(home) });
    log(`paired with ${pairing.name} as ${pairing.client}; your own programs: LAMPOS_HOME=${home}`);
    const twin = spawn(process.execPath, [fileURLToPath(new URL('server.js', import.meta.url)), '--port', String(options['twin-port'] ?? 8737), ...(options.glb ? ['--glb', options.glb] : []),
      ...(tryApp ? ['--lan', ...(options['lan-address'] ? ['--lan-address', options['lan-address']] : []), '--stim-dir', dir, '--lamp-sim', lampSim] : [])], {
      env: { ...process.env, LAMPOS_HOME: home }, stdio: ['ignore', 'pipe', 'inherit'],
    });
    children.push(twin);
    // The twin prints its local URL, and with --lan the one on this computer's network too.
    const urls = await new Promise((resolve, reject) => {
      let text = '';
      let timer;
      twin.stdout.on('data', (chunk) => {
        text += chunk;
        const local = /http:\/\/localhost:\d+\/\?t=\S+/.exec(text)?.[0];
        const network = /on your network: (http:\/\/[\d.]+:\d+\/\?t=\S+)/.exec(text)?.[1];
        if (local && (network || !tryApp)) resolve({ local, network });
        else if (local) timer ??= setTimeout(() => resolve({ local }), 1500);
      });
      twin.on('close', (code) => reject(new Error(`the twin ended (${code})`)));
    });
    const url = urls.local;
    log(`the virtual lamp: ${url}`);
    if (!options.flags.includes('--no-open')) openBrowser(url, { window: !options.flags.includes('--browser') }).catch(() => {});
    // Rejects when the lamp or the twin ends; a caller that stops them itself need not look.
    const closed = Promise.race([ended, new Promise((_, reject) => twin.on('close', () => reject(new Error('the twin ended'))))]);
    closed.catch(() => {});
    return { url, network: urls.network, home, stop, closed };
  } catch (error) {
    stop();
    throw error;
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  let options;
  try { options = parseArguments(process.argv.slice(2)); } catch (error) { console.error(`${error.message}\nusage: see the top of this file`); process.exit(2); }
  try {
    const running = await start(options);
    process.on('SIGINT', () => { running.stop(); process.exit(0); });
    process.on('SIGTERM', () => { running.stop(); process.exit(0); });
    await running.closed;
  } catch (error) {
    console.error(`virtual-lamp: ${error.message}`);
    process.exit(1);
  }
}
