// Starts the twin (examples/twin/server.js) for a test and waits for the address it prints. One start for every test that runs the
// twin as a child: its port is probed free and tried again when another test took it in between (onFreePort); its address is
// waited for up to `ms` (30 s: a loaded runner took more than the 10 s one test allowed, a red Node 22 job on #6915; the whole
// file still has its 60 s); and a twin that ENDS is said at once with its exit code and what it wrote on stderr, not after the wait.
// A timer never outlives the wait (an uncleared one keeps the test process alive). `t.after` kills the child, registered here.
import { spawn } from 'node:child_process';
import { onFreePort } from './free-port.js';

export function startTwin(t, serverJs, argsFor, { ms = 30000 } = {}) {
  return onFreePort((port) => {
    const child = spawn(process.execPath, [serverJs, ...argsFor(port)], { stdio: ['ignore', 'pipe', 'pipe'] });
    t.after(() => child.kill('SIGTERM'));
    let text = '';
    let errors = '';
    child.stderr.on('data', (chunk) => { errors += chunk; process.stderr.write(chunk); });
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(`the twin printed no address in ${ms / 1000} s: ${errors.trim()}`)), ms);
      child.stdout.on('data', (chunk) => {
        text += chunk;
        const found = /\?t=(\S+)/.exec(text);
        if (found) { clearTimeout(timer); resolve({ port, token: found[1], child }); }
      });
      child.once('close', (code) => { clearTimeout(timer); reject(new Error(`the twin ended (${code}) before it printed its address: ${errors.trim()}`)); });
    });
  });
}
