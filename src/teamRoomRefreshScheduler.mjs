/**
 * Coalesces bursty WebSocket notifications into authoritative snapshot refreshes.
 * A running refresh is never overlapped; notifications received during it schedule
 * one trailing refresh so the final persisted state is observed.
 */
export function createCoalescedRefreshScheduler(refresh, delayMs = 75) {
  if (typeof refresh !== "function") throw new TypeError("refresh must be a function");
  if (!Number.isInteger(delayMs) || delayMs < 0 || delayMs > 5000) {
    throw new RangeError("delayMs must be an integer from 0 to 5000");
  }

  let timer = null;
  let running = false;
  let dirty = false;
  let cancelled = false;

  const schedule = () => {
    if (cancelled) return false;
    dirty = true;
    if (timer !== null || running) return false;
    timer = setTimeout(() => {
      timer = null;
      if (cancelled || !dirty) return;
      dirty = false;
      running = true;
      Promise.resolve()
        .then(refresh)
        .finally(() => {
          running = false;
          if (dirty && !cancelled) schedule();
        });
    }, delayMs);
    return true;
  };

  return {
    schedule,
    cancel() {
      cancelled = true;
      dirty = false;
      if (timer !== null) clearTimeout(timer);
      timer = null;
    }
  };
}
