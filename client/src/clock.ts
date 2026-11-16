/**
 * Call `tick` each `step` ms until `until`, and one time at `until`, so that a
 * view can change at the exact moment (a login code expires). Returns a stop function.
 */
export function watch(until: string, tick: (now: Date) => void, step = 30_000): () => void {
  const end = Date.parse(until);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const next = () => {
    const left = end - Date.now();
    if (left > 0) timer = setTimeout(run, Math.min(step, left));
  };
  const run = () => {
    tick(new Date());
    next();
  };
  next();
  return () => clearTimeout(timer);
}

/**
 * Call `load` each `ms` and give each value to `apply`, until `done` is true
 * for a value or until the stop function runs. A failed load tries again at the next tick.
 */
export function poll<T>(load: () => Promise<T>, apply: (value: T) => void, ms: number, done: (value: T) => boolean = () => false): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const run = async () => {
    try {
      const value = await load();
      if (stopped) return;
      apply(value);
      if (done(value)) return;
    } catch {
      // The engine can restart. Try again at the next tick.
    }
    if (!stopped) timer = setTimeout(run, ms);
  };
  timer = setTimeout(run, ms);
  return () => {
    stopped = true;
    clearTimeout(timer);
  };
}
