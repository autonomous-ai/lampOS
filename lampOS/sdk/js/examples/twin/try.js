// The try-it app: ONE command that starts a virtual lamp, the 3D twin, a control panel that drives the lamp
// through the SDK, buttons for what happens in the room, and a log of the lamp's events, and prints ONE URL
// for a browser on this network (a phone, a laptop).
//
//   node examples/twin/try.js [--lamp-sim PATH] [--bin-dir DIR] [--twin-port N] [--glb PATH] [--no-open]
//
// --lamp-sim PATH  the lamp-sim binary (default: `lamp-sim` on the PATH; lamp-body and lamp-control beside it)
// The lamp lives in a private folder of its own (~/.lampsim-try-PID, 0700) that Ctrl-C removes, and listens on
// 127.0.0.1 only: only this program reaches it. The page is the one thing on the network, and it answers only
// with the token in its URL (printed once). The camera and the microphone are OFF: the lamp has neither grant,
// so no picture of the room is ever on the page. It is the SDK, unchanged: each button is one SDK call, or one
// `lamp-sim` stimulus (someone arrives, a phrase, a tap, the air), and the log shows the call and the lamp's
// own answer, a refusal in its words. For a program of your own on the same lamp: LAMPOS_HOME=<folder>/sdk.
import { execFile } from 'node:child_process';
import { rmSync } from 'node:fs';
import { createServer } from 'node:net';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArguments, portFree, start } from './virtual-lamp.js';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const freePort = () => new Promise((resolve, reject) => {
  const probe = createServer();
  probe.once('error', reject);
  probe.listen(0, '127.0.0.1', () => { const { port } = probe.address(); probe.close(() => resolve(port)); });
});

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  let options;
  try { options = parseArguments(process.argv.slice(2)); } catch (error) { console.error(`${error.message}\nusage: see the top of this file`); process.exit(2); }
  const dir = join(homedir(), `.lampsim-try-${process.pid}`);
  let ending = false;
  let running;
  const clean = () => rmSync(dir, { recursive: true, force: true });
  try {
    // No host camera or microphone here: the page is on the network.
    if (options.flags.some((f) => f.startsWith('--camera') || f === '--microphone')) throw new Error('the try-it app has no camera or microphone: they would put the room on the network');
    options.dir = dir;
    options.port = String(await freePort());
    options['twin-port'] ??= String((await portFree(8737)) ? 8737 : await freePort());
    options.flags.push('--try');
    running = await start(options, (line) => console.log(line));
    // lamp-sim starts with one person in front of the lamp; the room starts empty here, so "someone arrives" is an arrival.
    await new Promise((resolve) => execFile(options['lamp-sim'] ?? 'lamp-sim', ['people', '0', dir], () => resolve()));
    console.log(`\n  Open this on any device on your network:\n\n    ${running.network ?? running.url}\n\n  (the token in it is the key; camera and microphone are off; Ctrl-C stops everything)\n`);
    // The lamp's parts end with lamp-sim, a moment after it; the folder goes once they are gone.
    const end = async (code) => {
      ending = true;
      running.stop();
      await Promise.race([running.closed.catch(() => {}), sleep(5_000)]);
      await sleep(1_500);
      clean();
      process.exit(code);
    };
    process.on('SIGINT', () => end(0));
    process.on('SIGTERM', () => end(0));
    await running.closed;
  } catch (error) {
    if (ending) await sleep(60_000); // the Ctrl-C path is cleaning up and will exit
    console.error(`try: ${error.message}`);
    // Whatever ended (the twin, a crash), the lamp's parts are stopped before the folder goes.
    running?.stop();
    if (running) await Promise.race([running.closed.catch(() => {}), sleep(5_000)]);
    await sleep(2_000);
    clean();
    process.exit(1);
  }
}
