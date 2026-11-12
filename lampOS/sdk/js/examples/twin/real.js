// The try-it app on a REAL lamp (`try.js --real`): pairs once, or reuses a keyring, and serves the same panel and
// checklist with a banner, no stimuli, and every row that cannot run drawn off with its reason. Nothing here ever prints
// or logs the code, the pin, the token's key or the keyring's path.
//
//   LAMP_TRY_CODE=12345678 LAMP_TRY_PIN=<the pin> node examples/twin/try.js --real --host H [--port N]
//   node examples/twin/try.js --real --keyring DIR            (a pairing made before; nothing is removed)
//
// The code (8 digits) and the pin (the lamp's certificate fingerprint, from the owner's app) come from the environment
// or a prompt that does not echo: never from the command line, where `ps` shows them. A code run pairs as
// `try-controller` into a private folder (0700) that is removed on exit, SIGINT and SIGTERM too, and prints the client id to
// revoke when it is gone (the app, or `lamp-control revoke ID`). That line is said only on a clean end: a crash (SIGKILL)
// leaves the private folder here and that one paired client on the lamp, and the twin, finding its parent gone, takes this
// run's scratch off the lamp and ends by itself.
// The camera and the microphone rows work only with `--camera` / `--microphone` (and the lamp's grant): the page is on
// the network, over plain HTTP.
import { spawn } from 'node:child_process';
import { chmodSync, mkdirSync, rmSync } from 'node:fs';
import { createServer } from 'node:net';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Keyring, Lamp } from '../../src/index.js';
import { portFree } from './virtual-lamp.js';

export const CLIENT_NAME = 'try-controller';
const ONE_WORD = new Set(['--real', '--camera', '--microphone', '--no-open']);
const WITH_VALUE = new Set(['--host', '--port', '--keyring', '--twin-port', '--glb', '--lan-address']);

// The arguments of a real run. `--code` and `--pin` are refused with the way to give them; anything else unknown is too.
export function parseReal(args) {
  const out = { flags: new Set() };
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    const name = arg.split('=')[0];
    if (name === '--code' || name === '--pin') throw new Error(`${name} on the command line shows in \`ps\`: give it as ${name === '--code' ? 'LAMP_TRY_CODE' : 'LAMP_TRY_PIN'} in the environment, or wait for the prompt`);
    if (ONE_WORD.has(arg)) out.flags.add(arg.slice(2));
    else if (WITH_VALUE.has(arg) && args[i + 1] !== undefined) out[arg.slice(2)] = args[(i += 1)];
    else throw new Error(`unknown argument ${arg}`);
  }
  if (!out.keyring && !out.host) throw new Error('--real needs --host H (and the code and pin) or --keyring DIR (a pairing made before)');
  if (out.keyring && out.host) throw new Error('--keyring and --host: a keyring already knows its lamp; give one of them');
  return out;
}

// A line read with no echo (a TTY), or from the pipe when input is not one.
export function promptHidden(label, { input = process.stdin, output = process.stderr } = {}) {
  return new Promise((resolve, reject) => {
    output.write(label);
    let text = '';
    const finish = (value, error) => { input.setRawMode?.(false); input.pause(); input.removeListener('data', onData); output.write('\n'); error ? reject(error) : resolve(value); };
    const onData = (chunk) => {
      for (const ch of chunk.toString('utf8')) {
        if (ch === '\n' || ch === '\r') return finish(text);
        if (ch === '\u0003') return finish('', new Error('cancelled'));
        if (ch === '\u007f' || ch === '\b') text = text.slice(0, -1); else text += ch;
      }
      return undefined;
    };
    input.setRawMode?.(true);
    input.resume();
    input.on('data', onData);
  });
}

// The code and the pin: the environment first, then the hidden prompt.
export async function credentials(env = process.env, ask = promptHidden) {
  const code = env.LAMP_TRY_CODE ?? await ask('Pairing code (8 digits, not shown): ');
  const pin = env.LAMP_TRY_PIN ?? await ask('Pin (the lamp\'s certificate fingerprint, not shown): ');
  return { code, pin };
}

export const privateFolder = (pid = process.pid) => join(homedir(), `.lampos-try-real-${pid}`);

// Pairs into a private folder and returns it with the client id, or opens the keyring that was given.
export async function pairOrOpen(options, env = process.env, ask = promptHidden) {
  if (options.keyring) return { keyring: new Keyring(options.keyring), folder: undefined, client: undefined };
  const { code, pin } = await credentials(env, ask);
  const folder = privateFolder();
  mkdirSync(folder, { recursive: true, mode: 0o700 });
  chmodSync(folder, 0o700);
  const keyring = new Keyring(folder);
  try {
    const pairing = await Lamp.pair(code, options.host, { pin, ...(options.port ? { port: Number(options.port) } : {}), name: CLIENT_NAME, keyring });
    return { keyring, folder, client: pairing.client };
  } catch (error) {
    rmSync(folder, { recursive: true, force: true });
    // The lamp's words, never the code or the pin (the SDK's wrong-lamp message carries the pin's first 16 digits: scrubbed).
    const pinText = String(pin).replaceAll(' ', '').replaceAll(':', '').toLowerCase();
    let said = String(error?.message ?? 'pairing failed');
    for (const [secret, word] of [[pinText, '<pin>'], [pinText.slice(0, 16), '<pin>'], [String(code).replaceAll(' ', ''), '<code>'], [String(pin), '<pin>'], [String(code), '<code>']]) {
      if (secret.length >= 8) said = said.replaceAll(secret, word);
    }
    throw new Error(said);
  }
}

// Runs the twin on that keyring and says its URL; the page is on the network like try.js's.
export async function startReal(options, folderOrKeyringPath) {
  const twinPort = options['twin-port'] ?? String((await portFree(8737)) ? 8737 : 0);
  const args = [fileURLToPath(new URL('server.js', import.meta.url)), '--real', '--lan', '--port', twinPort === '0' ? String(await freePort()) : twinPort,
    ...(options.glb ? ['--glb', options.glb] : []), ...(options['lan-address'] ? ['--lan-address', options['lan-address']] : []),
    ...(options.flags.has('camera') ? ['--camera'] : []), ...(options.flags.has('microphone') ? ['--microphone'] : [])];
  const { LAMP_TRY_CODE: _code, LAMP_TRY_PIN: _pin, ...clean } = process.env;
  // Its own process group: a terminal's Ctrl-C reaches this program alone, which then asks the twin to end (SIGTERM) and waits.
  const twin = spawn(process.execPath, args, { env: { ...clean, LAMP_TRY_KEYRING: folderOrKeyringPath }, stdio: ['ignore', 'pipe', 'inherit'], detached: true });
  const urls = await new Promise((resolve, reject) => {
    let text = '';
    let timer;
    twin.stdout.on('data', (chunk) => {
      text += chunk;
      const local = /http:\/\/localhost:\d+\/\?t=\S+/.exec(text)?.[0];
      const network = /on your network: (http:\/\/[\d.]+:\d+\/\?t=\S+)/.exec(text)?.[1];
      if (local && network) resolve({ local, network });
      else if (local) timer ??= setTimeout(() => resolve({ local }), 1500);
    });
    twin.on('close', (code) => reject(new Error(`the twin ended (${code})`)));
  });
  return { twin, urls };
}

const freePort = () => new Promise((resolve, reject) => {
  const probe = createServer();
  probe.once('error', reject);
  probe.listen(0, '127.0.0.1', () => { const { port } = probe.address(); probe.close(() => resolve(port)); });
});

// A whole real run: pair or open, serve, wait, and on the end (a signal, or the twin ending) take the twin down, remove the
// folder this run made, and say which client to revoke. Exits the process.
export async function runReal(argv, { exit = (code) => process.exit(code), log = console.log, err = console.error } = {}) {
  let options;
  try { options = parseReal(argv); } catch (error) { err(`try: ${error.message}\nusage: see the top of real.js`); return exit(2); }
  let made;
  try { made = await pairOrOpen(options); } catch (error) { err(`try: ${error.message}`); return exit(1); }
  const keyringAt = made.folder ?? options.keyring;
  let twin;
  let ending = false;
  const end = async (code) => {
    if (ending) return;
    ending = true;
    if (twin && twin.exitCode === null) {
      twin.kill('SIGTERM');
      await Promise.race([new Promise((resolve) => twin.on('close', resolve)), new Promise((resolve) => setTimeout(resolve, 8000))]);
    }
    if (made.folder) {
      rmSync(made.folder, { recursive: true, force: true });
      log(`this run's pairing is removed here. The lamp still lists the client ${CLIENT_NAME} (${made.client}): revoke it in the app, or on the lamp: lamp-control revoke ${made.client}`);
    }
    exit(code);
  };
  process.on('SIGINT', () => end(0));
  process.on('SIGTERM', () => end(0));
  try {
    const started = await startReal(options, keyringAt);
    twin = started.twin;
    const on = [options.flags.has('camera') && 'CAMERA', options.flags.has('microphone') && 'MICROPHONE'].filter(Boolean).join(' and ');
    log(`\n  A REAL LAMP. Open this on any device on your network:\n\n    ${started.urls.network ?? started.urls.local}\n\n  (the token in it is the key; Ctrl-C stops the page and takes this run's test clips and notes off the lamp)`);
    log(on ? `  ${on} ON: anyone on your network who has this link can see or hear the room through the page, over plain HTTP.\n` : '  The camera and the microphone rows are off (give --camera / --microphone to turn them on).\n');
    // The twin ending by itself is an end with a failure, unless it ended cleanly (exit 0: it was asked to by a signal that
    // reached it too, and has cleaned up): that is a clean end of the run.
    twin.on('close', (code) => end(code === 0 ? 0 : 1));
  } catch (error) {
    err(`try: ${error.message}`);
    await end(1);
  }
  return undefined;
}
