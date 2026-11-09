import test from 'node:test';
import assert from 'node:assert/strict';
import { bencherMetrics, testbed } from '../.github/benchmarks/bencher-metrics.mjs';

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
  assert.deepEqual(metrics['hook/stop/native']['latency-p95'], { value: 3e6 });
  assert.deepEqual(metrics['startup/native']['latency-p95'], { value: 7e6 });
  assert.deepEqual(metrics['install/native-binary'], { 'file-size': { value: 1000 } });
  assert.deepEqual(metrics['install/plugin'], { 'file-size': { value: 2000 } });
});

test('omits metrics a platform did not record and counters that measure a launcher shell', () => {
  const metrics = bencherMetrics(results, file => exports[file]);
  assert.equal(metrics['startup/native']['peak-memory'], undefined);
  assert.equal(metrics['hook/stop/launcher'].instructions, undefined);
  assert.equal(metrics['hook/stop/launcher'].latency.value, 4e6);
});

test('reports the mean CPU time on Windows, where it is charged in whole scheduler ticks', () => {
  const tick = 0.015625, ticked = (user) => ({ time_wall_clock: { value: 0.01, unit: 'second' }, time_user: { value: user, unit: 'second' } });
  const windows = { ...results, platform: { os: 'win32' }, cells: [{ name: 'hook/stop/native', file: 'ticks.json' }] };
  const read = () => ({ results: [{ measurements: [ticked(0), ticked(0), ticked(tick), ticked(0)] }] });
  assert.equal(bencherMetrics(windows, read)['hook/stop/native']['cpu-user-time'].value, tick / 4 * 1e9);
  assert.equal(bencherMetrics({ ...windows, platform: { os: 'linux' } }, read)['hook/stop/native']['cpu-user-time'].value, 0, 'elsewhere the median');
});

test('takes peak memory measured outside Hyperfine only where Hyperfine recorded none', () => {
  const memory = { 'startup/native': [5e6, 7e6, 6e6], 'hook/stop/native': [9e9] };
  const metrics = bencherMetrics({ ...results, memory }, file => exports[file]);
  assert.deepEqual(metrics['startup/native']['peak-memory'], { value: 6e6, lower_value: 5e6, upper_value: 7e6 });
  assert.equal(metrics['hook/stop/native']['peak-memory'].value, 4194304);
});

test('names the testbed by runner label and CPU model', () => {
  assert.equal(testbed('windows-latest', 'AMD EPYC 7763 64-Core Processor                '), 'windows-latest-amd-epyc-7763');
  assert.equal(testbed('windows-latest', 'INTEL(R) XEON(R) PLATINUM 8573C'), 'windows-latest-intel-xeon-platinum-8573c');
  assert.equal(testbed('macos-latest', 'Apple M1 (Virtual)'), 'macos-latest-apple-m1-virtual');
  assert.equal(testbed('ubuntu-latest', 'Intel(R) Xeon(R) CPU E5-2673 v4 @ 2.30GHz'), 'ubuntu-latest-intel-xeon-e5-2673-v4');
  assert.equal(testbed('ubuntu-latest', null), 'ubuntu-latest');
});
