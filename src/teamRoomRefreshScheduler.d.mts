export function createCoalescedRefreshScheduler(
  refresh: () => Promise<unknown>,
  delayMs?: number
): {
  schedule: () => boolean;
  cancel: () => void;
};
