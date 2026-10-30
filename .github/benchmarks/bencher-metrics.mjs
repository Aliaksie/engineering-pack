// Convert suite results into Bencher Metric Format: one benchmark per measured cell, one measure per
// Hyperfine metric the platform recorded, plus installed sizes. Values are medians with min/max bounds.
const MEASURES = {
  time_wall_clock: 'latency', time_user: 'cpu-user-time', time_system: 'cpu-system-time',
  memory_peak_resident: 'peak-memory', instructions: 'instructions', cpu_cycles: 'cpu-cycles'
};
// Hardware counters cover only the directly started process: a launcher row would count the shell.
const COUNTERS = new Set(['instructions', 'cpu_cycles']);
// Bencher latency is nanoseconds; CPU times follow it. Memory stays in bytes, counters in counts.
const SCALE = { second: 1e9 };

const median = sorted => (sorted[Math.floor((sorted.length - 1) / 2)] + sorted[Math.floor(sorted.length / 2)]) / 2;

// `read(file)` returns a parsed Hyperfine JSON export named in results.cells.
export function bencherMetrics(results, read) {
  const metrics = {};
  for (const { name, file } of results.cells) {
    const values = {};
    for (const measurement of read(file).results.flatMap(result => result.measurements)) {
      for (const [metric, { value, unit }] of Object.entries(measurement)) {
        if (!MEASURES[metric] || !Number.isFinite(value) || (COUNTERS.has(metric) && !name.endsWith('/native'))) continue;
        (values[MEASURES[metric]] ??= []).push(value * (SCALE[unit] ?? 1));
      }
    }
    metrics[name] = Object.fromEntries(Object.entries(values).map(([measure, list]) => {
      const sorted = list.toSorted((a, b) => a - b);
      return [measure, { value: median(sorted), lower_value: sorted[0], upper_value: sorted.at(-1) }];
    }));
  }
  metrics['install/native-binary'] = { 'file-size': { value: results.runtime.nativeBytes } };
  metrics['install/plugin'] = { 'file-size': { value: results.runtime.pluginBytes } };
  return metrics;
}
