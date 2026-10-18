#!/usr/bin/env node
// Validate recorded rubric evidence. This is deliberately not an LLM evaluator.
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { json, loadCapabilities } from './lib/capabilities.mjs';
import { HOSTS } from '../hosts/registry.mjs';

export function evaluateReport(packages, report) {
  const item = packages.find(({ manifest }) => manifest.name === report.package);
  if (!item) throw new Error('unknown eval package');
  for (const key of ['adapter', 'model', 'revision', 'evaluator', 'response']) {
    if (typeof report[key] !== 'string' || !report[key].trim()) throw new Error(`${key} is required`);
  }
  if (!Object.hasOwn(HOSTS, report.adapter)) throw new Error('unknown eval adapter');
  if (report.packageVersion !== item.manifest.version) throw new Error('eval package version does not match source');
  const cases = item.manifest.evals.map((entry) => json(path.join(item.root, entry)));
  const testCase = cases.find((entry) => entry.id === report.case);
  if (!testCase) throw new Error('unknown eval case');
  if (!Array.isArray(report.verdicts)) throw new Error('criterion verdicts required');
  const ids = report.verdicts.map((entry) => entry.criterion);
  if (new Set(ids).size !== ids.length || ids.length !== testCase.criteria.length
      || testCase.criteria.some((entry) => !ids.includes(entry.id))) throw new Error('each criterion needs exactly one verdict');
  for (const entry of report.verdicts) {
    if (typeof entry.passed !== 'boolean' || typeof entry.evidence !== 'string' || !entry.evidence.trim()) throw new Error('boolean verdict and evidence required');
  }
  const passed = report.verdicts.filter((entry) => entry.passed).length;
  return { package: report.package, case: report.case, adapter: report.adapter, passed, total: ids.length, success: passed === ids.length };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    if (process.argv.length !== 3) throw new Error('usage: node tooling/eval-report.mjs <recorded-result.json>');
    const packages = loadCapabilities(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..'));
    const result = evaluateReport(packages, json(path.resolve(process.argv[2])));
    process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
    if (!result.success) process.exitCode = 1;
  } catch (error) { process.stderr.write(`${error.message}\n`); process.exitCode = 2; }
}
