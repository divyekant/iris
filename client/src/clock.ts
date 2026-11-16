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
