import test from 'node:test';
import assert from 'node:assert/strict';
import { bencherMetrics } from '../.github/benchmarks/bencher-metrics.mjs';

const run = (seconds, extra = {}) => ({ time_wall_clock: { value: seconds, unit: 'second' }, time_user: { value: seconds / 2, unit: 'second' }, ...extra });
const results = {
  cells: [{ name: 'hook/stop/native', file: 'native.json' }, { name: 'hook/stop/launcher', file: 'launcher.json' }, { name: 'startup/native', file: 'windows.json' }],
  runtime: { nativeBytes: 1000, pluginBytes: 2000 }
};
const exports = {
  'native.json': { results: [{ measurements: [run(0.003, { instructions: { value: 30, unit: 'count' } }), run(0.001, { memory_peak_resident: { value: 4194304, unit: 'byte' } }), run(0.002)] }] },
  'launcher.json': { results: [{ measurements: [run(0.004, { instructions: { value: 9, unit: 'count' } })] }] },
  'windows.json': { results: [{ measurements: [run(0.005), run(0.007)] }] }
};

test('maps every recorded Hyperfine metric to a Bencher measure with median and range', () => {
  const metrics = bencherMetrics(results, file => exports[file]);
  assert.deepEqual(metrics['hook/stop/native'].latency, { value: 2e6, lower_value: 1e6, upper_value: 3e6 });
  assert.deepEqual(metrics['hook/stop/native']['cpu-user-time'], { value: 1e6, lower_value: 5e5, upper_value: 1.5e6 });
  assert.equal(metrics['hook/stop/native']['peak-memory'].value, 4194304);
  assert.equal(metrics['hook/stop/native'].instructions.value, 30);
  assert.equal(metrics['startup/native'].latency.value, 6e6, 'even run counts use the middle pair');
  assert.deepEqual(metrics['install/native-binary'], { 'file-size': { value: 1000 } });
  assert.deepEqual(metrics['install/plugin'], { 'file-size': { value: 2000 } });
});

test('omits metrics a platform did not record and counters that measure a launcher shell', () => {
  const metrics = bencherMetrics(results, file => exports[file]);
  assert.equal(metrics['startup/native']['peak-memory'], undefined);
  assert.equal(metrics['hook/stop/launcher'].instructions, undefined);
  assert.equal(metrics['hook/stop/launcher'].latency.value, 4e6);
});
