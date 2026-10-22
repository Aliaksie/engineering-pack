import { normalizeKey } from './normalize-key.mjs';

export function receiptStore() {
  const seen = new Set();
  return {
    apply(key) {
      normalizeKey(key);
      if (seen.has(key)) return { applied: false, total: seen.size };
      seen.add(key);
      return { applied: true, total: seen.size };
    }
  };
}
