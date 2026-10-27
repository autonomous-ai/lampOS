// Finding lamps on the home network by mDNS (`_lampos._tcp`). Needs the
// optional `bonjour-service` package; without it, pair with the lamp's address.
// Any host on the network can answer mDNS, so a lamp's `fp` (the first 16 hex of
// its certificate's SHA-256) only helps pick it: pairing checks the full pin.
import * as schema from './schema.js';
import { LampError } from './errors.js';

export async function find(ms = 3000) {
  let Bonjour;
  try {
    ({ Bonjour } = await import('bonjour-service'));
  } catch {
    throw new LampError("finding lamps needs bonjour-service: npm i bonjour-service", { code: 'no_mdns' });
  }
  const bonjour = new Bonjour();
  const found = [];
  const browser = bonjour.find({ type: schema.SERVICE, protocol: 'tcp' }, (service) => {
    const txt = service.txt ?? {};
    if (String(txt.v) !== String(schema.VERSION)) return;
    found.push({ id: txt.id ?? '', name: txt.name ?? '', host: service.host, port: service.port ?? schema.PORT, fp: txt.fp ?? '' });
  });
  await new Promise((resolve) => setTimeout(resolve, ms));
  browser.stop();
  bonjour.destroy();
  return found;
}
