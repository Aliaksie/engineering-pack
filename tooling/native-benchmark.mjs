#!/usr/bin/env node
// Compare whole installed operations in disposable consumers; never touch user state.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { performance } from 'node:perf_hooks';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

export function benchmarkNativeBundle(bundle) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-native-benchmark-'));
  const craft = path.resolve(bundle, `native-bin/craft${process.platform === 'win32' ? '.exe' : ''}`);
  try {
    execFileSync('git', ['init', '--quiet', root]);
    for (let i = 0; i < 25; i++) fs.writeFileSync(path.join(root, `source-${i}.js`), `export function item${i}(value) { return value + ${i}; }\n`);
    const run = (operation, args = []) => {
      const start = performance.now();
      const result = JSON.parse(execFileSync(craft, ['project-evidence', operation, '--enable', '--root', root, '--json', ...args], {
        cwd: root, encoding: 'utf8', timeout: 30000, maxBuffer: 4 * 1024 * 1024
      }));
      return { ms: performance.now() - start, result };
    };
    const version = run('version');
    const refresh = run('refresh');
    if (refresh.result.changed !== 25) throw new Error('benchmark refresh did not cover its source fixture');
    const operations = [{ operation: 'version', firstFreshProcessMs: version.ms }, { operation: 'refresh', firstFreshProcessMs: refresh.ms }];
    for (const [operation, args] of [['version', []], ['refresh', []], ['query', ['--value', 'item0', '--verify']]]) {
      const samples = [];
      for (let i = 0; i < 6; i++) {
        const sample = run(operation, args);
        if (operation === 'refresh' && sample.result.reused !== 25) throw new Error('benchmark reuse failed');
        if (operation === 'query' && (sample.result.results.length !== 1 || sample.result.sources[sample.result.results[0].path].verification !== 'verified-selected-source')) throw new Error('benchmark query failed');
        samples.push(sample.ms);
      }
      const entry = operations.find(entry => entry.operation === operation);
      if (entry) entry.subsequentFreshProcessMs = samples;
      else operations.push({ operation, freshProcessMs: samples });
    }
    const marker = path.resolve(bundle, 'native-bundle.json');
    return { bundle: path.resolve(bundle), binaryVersion: version.result.binaryVersion,
      manifestSha256: fs.existsSync(marker) ? createHash('sha256').update(fs.readFileSync(marker)).digest('hex') : null, operations };
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [candidate, baseline] = process.argv.slice(2);
    if (!candidate || process.argv.length > 4) throw new Error('usage: native-benchmark.mjs <candidate-runtime-root> [baseline-runtime-root]');
    console.log(JSON.stringify({ formatVersion: 1, measuredAt: new Date().toISOString(), platform: process.platform, arch: process.arch, node: process.version,
      candidate: benchmarkNativeBundle(candidate), ...(baseline ? { baseline: benchmarkNativeBundle(baseline) } : {}),
      limits: ['Fresh processes with OS caches retained; six repeated samples per operation.', 'Measures the installed Node adapter, version handshake and actual release binary together.',
        'No speedup threshold, RSS or process-tree measurement.'] }, null, 2));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
