// Finding lamps on the home network by mDNS. Needs the optional
// `bonjour-service` package (npm installs it by default); without it, pair with
// the lamp's address. `find` lists the lamps that announce the control door
// (`_lampos._tcp`, with TXT `id`, `name`, `v` and `fp`). `pairable` is what
// `lamp pair CODE` with no `--host` uses: those, and the lamps the image's avahi
// already announces (`_autonomous._tcp`, named `lamp-xxxx`), kept only if their
// control port answers, which it does only while the owner has developer access
// on. Any host on the network can answer mDNS, so what is found only says where
// to connect: the pairing's code and its pin say whether it is the lamp.
import { connect as tcpConnect } from 'node:net';
import * as schema from './schema.js';
import { LampError } from './errors.js';

// What the lamp image's avahi announces today (scripts/imager): every device,
// its instance named by its host name, so a lamp is `lamp-xxxx`.
export const DEVICE_SERVICE = 'autonomous';
export const LAMP_PREFIX = 'lamp-';

const address = (service) => (service.addresses ?? []).find((a) => !a.includes(':')) ?? service.addresses?.[0] ?? '';

// The `_lampos._tcp` lamps and, with `devices`, the `_autonomous._tcp` lamps no
// `_lampos._tcp` answer named, on the control port.
async function browse(ms, devices) {
  let Bonjour;
  try {
    ({ Bonjour } = await import('bonjour-service'));
  } catch {
    throw new LampError("finding lamps needs bonjour-service: npm i bonjour-service", { code: 'no_mdns' });
  }
  const bonjour = new Bonjour();
  const found = [];
  const others = [];
  const browsers = [bonjour.find({ type: schema.SERVICE, protocol: 'tcp' }, (service) => {
    const txt = service.txt ?? {};
    if (String(txt.v) !== String(schema.VERSION)) return;
    found.push({ id: txt.id ?? '', name: txt.name ?? '', host: service.host, port: service.port ?? schema.PORT, fp: txt.fp ?? '', address: address(service) });
  })];
  if (devices) {
    browsers.push(bonjour.find({ type: DEVICE_SERVICE, protocol: 'tcp' }, (service) => {
      if (!String(service.name ?? '').startsWith(LAMP_PREFIX)) return;
      others.push({ id: '', name: service.name, host: service.host, port: schema.PORT, fp: '', address: address(service) });
    }));
  }
  await new Promise((resolve) => setTimeout(resolve, ms));
  for (const browser of browsers) browser.stop();
  bonjour.destroy();
  return [...found, ...others.filter((lamp) => !found.some((known) => known.host === lamp.host))];
}

// The lamps that announce `_lampos._tcp` within `ms`.
export async function find(ms = 3000) {
  return browse(ms, false);
}

// Whether the lamp's control port takes a TCP connection: nothing is sent, so
// no code or proof reaches a host that is not the lamp.
export function answers(lamp, ms = 1500) {
  return new Promise((resolve) => {
    const socket = tcpConnect({ host: lamp.address || lamp.host, port: lamp.port });
    const done = (ok) => { clearTimeout(timer); socket.destroy(); resolve(ok); };
    const timer = setTimeout(() => done(false), ms);
    socket.once('connect', () => done(true));
    socket.once('error', () => done(false));
  });
}

// The lamps `lamp pair CODE` could pair with: the ones mDNS finds whose control
// port answers within `probe` ms, as it does only while developer access is on.
// `look` stands in for mDNS in a test.
export async function pairable(ms = 3000, { probe = 1500, look = browse } = {}) {
  const lamps = await look(ms, true);
  const ok = await Promise.all(lamps.map((lamp) => answers(lamp, probe)));
  return lamps.filter((_, i) => ok[i]);
}

// The one lamp to pair with among `lamps` (with a `pin`, only one whose mDNS
// `fp` starts it), or `why` there is none to pick.
export function choose(lamps, pin) {
  const fit = pin == null ? lamps : lamps.filter((lamp) => lamp.fp && pin.startsWith(lamp.fp));
  if (!fit.length) {
    return { why: "no lamp with developer access on answered on this network: check that the owner turned it on in the app and that this computer is on the lamp's network, or say where it is with --host lamp-xxxx.local (or its IP address)" };
  }
  if (fit.length > 1) {
    return { why: `${fit.length} lamps answered: ${fit.map((lamp) => `${lamp.name || lamp.id} (${lamp.host || lamp.address})`).join(', ')}; say which with --host` };
  }
  return { lamp: fit[0] };
}
