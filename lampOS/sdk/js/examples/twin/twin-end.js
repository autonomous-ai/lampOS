// How a real run's twin ends (server.js --real): ONE clean-up however many times it is asked, a third ask ends it at once, and
// the watch for a parent that has died stops at the first ask (it is one ask, not a stream of them).

// `cleanup` runs once, in order, and `exit(0)` follows; `exit(1)` is for the third ask or a clean-up past `mostMs`.
export function endHandler({ cleanup, exit, mostMs = 15_000, setTimer = setTimeout }) {
  let asks = 0;
  let ending = false;
  return async function end() {
    asks += 1;
    // A repeat ask (a second Ctrl-C, a parent's SIGTERM after a terminal's SIGINT) must not cut the clean-up short: only a third
    // one ends at once.
    if (asks >= 3) exit(1);
    if (ending) return;
    ending = true;
    setTimer(() => exit(1), mostMs).unref?.();
    await cleanup();
    exit(0);
  };
}

// Calls `end` when the parent is no longer `parent`, checked every `everyMs`; the check stops itself at its first call, so the
// clean-up it starts is not counted as more asks.
export function watchParent({ parent, ppid = () => process.ppid, end, everyMs = 2000, setTick = setInterval, clearTick = clearInterval }) {
  const tick = setTick(() => {
    if (ppid() === parent) return;
    clearTick(tick);
    end();
  }, everyMs);
  tick.unref?.();
  return () => clearTick(tick);
}
