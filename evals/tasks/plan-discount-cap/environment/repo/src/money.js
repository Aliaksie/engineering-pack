// Amounts are integer cents; rates and percentages are applied once and rounded half up.
export function percentOf(cents, percent) {
  return Math.round((cents * percent) / 100);
}

export function format(cents) {
  const sign = cents < 0 ? '-' : '';
  const absolute = Math.abs(cents);
  return `${sign}$${Math.floor(absolute / 100)}.${String(absolute % 100).padStart(2, '0')}`;
}
