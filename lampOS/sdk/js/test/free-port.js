// A free port for a test's child, and a start that is tried again when the port was taken in between. The port is probed free and
// closed, and only then does the child bind it: another test file can take it in the gap, and the child dies with EADDRINUSE (a red
// Node 22 job on #6745). `start(...ports)` is called with fresh probed ports; it throws when its child ended early, with the child's
// stderr in the message, and a start that failed for that reason alone is tried again (up to `tries`).
import { createServer } from 'node:net';

export const freePort = () => new Promise((resolve) => {
  const probe = createServer();
  probe.listen(0, '127.0.0.1', () => { const { port } = probe.address(); probe.close(() => resolve(port)); });
});

const TAKEN = /EADDRINUSE|is in use/;

export async function onFreePort(start, { tries = 5, ports = 1 } = {}) {
  for (let attempt = 1; ; attempt += 1) {
    const found = [];
    for (let i = 0; i < ports; i += 1) found.push(await freePort());
    try {
      return await start(...found);
    } catch (error) {
      if (attempt >= tries || !TAKEN.test(String(error?.message))) throw error;
      process.stderr.write(`free-port: a probed port was taken, starting again (attempt ${attempt + 1} of ${tries})\n`);
    }
  }
}
