// Convert suite results into Bencher Metric Format: one benchmark per measured cell, one measure per
// Hyperfine metric the platform recorded, plus installed sizes. Values are medians with min/max bounds;
// latency also reports its 95th percentile (nearest rank) as `latency-p95`.
const MEASURES = {
  time_wall_clock: 'latency', time_user: 'cpu-user-time', time_system: 'cpu-system-time',
  memory_peak_resident: 'peak-memory', instructions: 'instructions', cpu_cycles: 'cpu-cycles'
};
// Hardware counters cover only the directly started process: a launcher row would count the shell.
const COUNTERS = new Set(['instructions', 'cpu_cycles']);
// Windows charges CPU time in whole scheduler ticks (15.625 ms), so a short run reads 0 or one
// tick and the median says nothing; the mean over the runs is unbiased.
const TICKED = new Set(['cpu-user-time', 'cpu-system-time']);
// Bencher latency is nanoseconds; CPU times follow it. Memory stays in bytes, counters in counts.
const SCALE = { second: 1e9 };

const percentile = (sorted, p) => sorted[Math.max(0, Math.ceil(p * sorted.length) - 1)];
const median = sorted => (sorted[Math.floor((sorted.length - 1) / 2)] + sorted[Math.floor(sorted.length / 2)]) / 2;
const mean = list => list.reduce((total, value) => total + value, 0) / list.length;
const range = (list, value) => {
  const sorted = list.toSorted((a, b) => a - b);
  return { value: value(sorted), lower_value: sorted[0], upper_value: sorted.at(-1) };
};

// The Bencher testbed: the runner label qualified by its CPU model, since one label is served by
// several CPU generations whose timings differ by more than the changes being tracked.
export function testbed(label, cpu) {
  const model = String(cpu ?? '').toLowerCase()
    .replace(/\(r\)|\(tm\)|\b\d+-core processor\b|\bprocessor\b|\bcpu\b|@.*$/g, ' ')
    .replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  return model ? `${label}-${model}` : label;
}

// `read(file)` returns a parsed Hyperfine JSON export named in results.cells. `results.memory`
// holds peak bytes per cell measured outside Hyperfine, used where Hyperfine recorded none.
export function bencherMetrics(results, read) {
  const ticked = results.platform?.os === 'win32';
  const metrics = {};
  for (const { name, file } of results.cells) {
    const values = {};
    for (const measurement of read(file).results.flatMap(result => result.measurements)) {
      for (const [metric, { value, unit }] of Object.entries(measurement)) {
        if (!MEASURES[metric] || !Number.isFinite(value) || (COUNTERS.has(metric) && !name.endsWith('/native'))) continue;
        (values[MEASURES[metric]] ??= []).push(value * (SCALE[unit] ?? 1));
      }
    }
    const peaks = results.memory?.[name]?.filter(Number.isFinite);
    if (!values['peak-memory'] && peaks?.length) values['peak-memory'] = peaks;
    metrics[name] = Object.fromEntries(Object.entries(values).flatMap(([measure, list]) => {
      const entry = [measure, range(list, ticked && TICKED.has(measure) ? () => mean(list) : median)];
      return measure === 'latency' ? [entry, ['latency-p95', { value: percentile(list.toSorted((a, b) => a - b), 0.95) }]] : [entry];
    }));
  }
  metrics['install/native-binary'] = { 'file-size': { value: results.runtime.nativeBytes } };
  metrics['install/plugin'] = { 'file-size': { value: results.runtime.pluginBytes } };
  return metrics;
}
