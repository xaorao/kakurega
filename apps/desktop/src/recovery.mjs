// Decides how often a crashed part may be brought back. A part that keeps crashing is not restarted forever: after
// `max` restarts inside `windowMs` the owner is told instead, so a broken install cannot hide behind a restart loop.
export function createRestartPolicy({ max = 3, windowMs = 300000, baseDelayMs = 500 } = {}) {
  let times = [];
  return {
    next(now = Date.now()) {
      times = times.filter((t) => now - t < windowMs);
      if (times.length >= max) return { restart: false, attempts: times.length };
      const delayMs = baseDelayMs * 2 ** times.length;
      times.push(now);
      return { restart: true, delayMs, attempts: times.length };
    },
  };
}

// Reasons Electron gives when a renderer is gone. A clean exit is the window closing, not a crash.
export function rendererCrashed(details) {
  return !!details && details.reason !== "clean-exit";
}
