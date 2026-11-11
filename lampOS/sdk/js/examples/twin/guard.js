// Who may use the twin's page and its live link. Binding to 127.0.0.1 keeps
// other computers out, not other websites: a browser lets any page it has open
// connect a WebSocket to localhost, with no CORS. So every request carries a
// token made at start (printed in the URL), and a /live upgrade must also come
// from the twin's own origin (the local leader's P2 on eighth/body-twin).
import { randomBytes, timingSafeEqual } from 'node:crypto';

export const newToken = () => randomBytes(24).toString('base64url');

// The request's `t`, compared in constant time; any other length is refused
// before the compare, which needs equal lengths.
export function tokenOk(url, token) {
  const given = Buffer.from(new URL(url, 'http://localhost').searchParams.get('t') ?? '');
  const wanted = Buffer.from(token);
  return given.length === wanted.length && timingSafeEqual(given, wanted);
}

// The page's own origin only: http://localhost:PORT or http://127.0.0.1:PORT, and with --lan the one address the
// twin printed (`lanHost`, "ADDRESS:PORT", fixed at start: never the request's own Host, which a page can choose).
export function originOk(origin, port, lanHost) {
  return origin === `http://localhost:${port}` || origin === `http://127.0.0.1:${port}` || (Boolean(lanHost) && origin === `http://${lanHost}`);
}

// Whether a request's Host header is one of the twin's own (the same three), so a page reached under another
// name (DNS rebinding) is refused before the token is even looked at.
export function hostOk(host, port, lanHost) {
  return host === `localhost:${port}` || host === `127.0.0.1:${port}` || (Boolean(lanHost) && host === lanHost);
}

// The network address the twin prints with --lan. A computer has many IPv4 addresses and a phone reaches only
// the one on its own network: the real interfaces (Ethernet, Wi-Fi), never a virtual one (Docker and its bridges,
// virtual machines, VPN and tailscale tunnels, loopback). `table` is os.networkInterfaces().
const VIRTUAL = /^(lo|docker\d*|br-|br\d+|veth|virbr|tailscale\d*|ts\d+|tun\d*|tap\d*|wg\d*|zt|utun\d*|vmnet\d*|vboxnet\d*|cni|flannel|kube|cali)/i;
const ipv4 = (table) => Object.entries(table).flatMap(([name, list]) => (list ?? []).filter((i) => i && i.family === 'IPv4' && !i.internal).map((i) => ({ name, address: i.address })));
export const lanAddresses = (table) => ipv4(table).filter(({ name }) => !VIRTUAL.test(name));

// The one address to bind and print: `wanted` (the person's --lan-address) if it is one of this computer's own
// IPv4 addresses, else the first real one. `others` are the other real addresses, to name a choice. An address
// that is not this computer's, or no real address at all, is `error`, with `own` (every address, to choose from).
export function chooseLan(table, wanted) {
  const real = lanAddresses(table);
  const own = ipv4(table);
  if (wanted !== undefined) {
    const found = own.find(({ address }) => address === wanted);
    if (!found) return { error: `${wanted} is not an IPv4 address of this computer`, own };
    return { address: found.address, name: found.name, others: real.filter(({ address }) => address !== wanted) };
  }
  if (!real.length) return { error: 'no network address found', own };
  return { address: real[0].address, name: real[0].name, others: real.slice(1) };
}
