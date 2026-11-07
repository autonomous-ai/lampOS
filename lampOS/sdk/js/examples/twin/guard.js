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

// The page's own origin only: http://localhost:PORT or http://127.0.0.1:PORT.
export function originOk(origin, port) {
  return origin === `http://localhost:${port}` || origin === `http://127.0.0.1:${port}`;
}
