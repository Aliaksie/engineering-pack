export function normalizeKey(key) {
  if (typeof key !== 'string' || !key.trim()) throw new TypeError('Invalid event key');
  return key.trim().toLowerCase();
}
