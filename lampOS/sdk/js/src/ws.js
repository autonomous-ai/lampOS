// `ws`, the SDK's one dependency. An install from a path without
// `--install-links` is a link to the checkout, and Node then cannot find `ws`
// from it: say what to run, not just Node's bare module error. `ws` is
// CommonJS, so it is loaded synchronously (no top-level await: the SDK stays
// a synchronous module, and `require('lampos')` keeps working on Node 22.12+).
import { createRequire } from 'node:module';

export const WS_MISSING =
  'lampos needs its `ws` package and cannot find it: install the SDK with `npm i --install-links /path/to/lampOS/lampOS/sdk/js` (a copy, not a link), or run `npm install` in the SDK folder';

let ws;
try {
  ws = createRequire(import.meta.url)('ws');
} catch (error) {
  throw error?.code === 'MODULE_NOT_FOUND' ? new Error(WS_MISSING, { cause: error }) : error;
}
export const WebSocket = ws;
export const WebSocketServer = ws.WebSocketServer;
