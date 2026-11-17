// The page server's one connection to a REAL lamp, kept: when it ends (the lamp restarts, the Wi-Fi drops, or this side's keepalive
// ends it for want of a pong) the page is told why at once, the connection is made again (the SDK's own pauses, 1, 2, 5, 10 s), what
// the page subscribed to is subscribed again, and the page goes on. Before this the twin connected once, so a single drop left every
// row saying "closed" until the page was restarted. Everything else is the SDK's `Lamp`, through a proxy: `lamp.grants`, `lamp.say(...)`.
//
// While it is down, a call answers the real reason ("the lamp's connection ended: ..., reconnecting"), not a bare "closed". What
// cannot be mended by trying again (a revoked pairing, another lamp at the address, a refused hello) ends the link and says why.
import { RECONNECT_PAUSES } from '../../src/client.js';
import { LampError } from '../../src/index.js';

const pauseFor = (ms) => new Promise((resolve) => { const timer = setTimeout(resolve, ms); timer.unref?.(); });

export async function linked(connect, { pauses = RECONNECT_PAUSES, say = () => {}, onUp = () => {}, onEnd = () => {}, wait = pauseFor } = {}) {
  let current = await connect();
  const subscriptions = [];
  let down = null; // why, while there is no connection
  let stopped = false;
  const ended = (why) => new LampError(`the lamp's connection ended: ${why}; reconnecting`, { code: 'closed' });

  const supervise = async () => {
    while (!stopped) {
      await current.closed();
      if (stopped) return;
      down = current.endedWhy ?? 'the connection ended';
      say(`the lamp's connection ended: ${down}; reconnecting`);
      let failures = 0;
      let next = null;
      while (!stopped && !next) {
        await wait(pauses[Math.min(failures, pauses.length - 1)] * 1000);
        if (stopped) return;
        try {
          next = await connect();
          // What the page listened to, again, on the new connection; a lamp that refuses one ends the link (a grant gone).
          for (const [event, handler, options] of subscriptions) await next.on(event, handler, options);
        } catch (error) {
          next?.close().catch(() => {});
          next = null;
          if (error instanceof LampError && error.code !== 'closed') {
            stopped = true;
            down = error.message;
            say(`the lamp's connection cannot be made again: ${down}`);
            onEnd(error);
            return;
          }
          failures += 1;
        }
      }
      current = next;
      down = null;
      say("the lamp's connection is back");
      onUp(current);
    }
  };
  supervise().catch((error) => { say(`the lamp's connection ended for good: ${error?.message ?? error}`); onEnd(error); });

  return new Proxy({}, {
    get(_, key) {
      if (key === 'on') {
        return async (event, handler, options) => {
          if (down) throw ended(down);
          const result = await current.on(event, handler, options);
          subscriptions.push([event, handler, options]);
          return result;
        };
      }
      if (key === 'off') {
        return async (event, handler) => {
          const at = subscriptions.findIndex(([e, h]) => e === event && h === handler);
          if (at >= 0) subscriptions.splice(at, 1);
          return down ? undefined : current.off(event, handler);
        };
      }
      if (key === 'close') {
        return async () => { stopped = true; return current.close(); };
      }
      if (key === 'link') return { up: down === null, why: down };
      const value = Reflect.get(current, key, current);
      if (typeof value !== 'function') return value;
      // A call while the connection is down says why, in the lamp's real words, at once.
      if (down) return () => Promise.reject(ended(down));
      return value.bind(current);
    },
  });
}
