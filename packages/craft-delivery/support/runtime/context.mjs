import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';

const slug = '[a-z0-9]+(?:-[a-z0-9]+)*';
const targetPattern = new RegExp(`^(${slug}):(${slug})(/references/(?:${slug}/)*${slug}\\.md)?$`);
const MAX_FILE_BYTES = 1024 * 1024;
const MAX_TARGETS = 256;

// Explicit roots only; never search a user's plugin cache or follow linked resources.
function localFile(root, relative) {
  let current = root;
  for (const part of relative.split('/')) {
    current = path.join(current, part);
    const stat = fs.lstatSync(current, { throwIfNoEntry: false });
    if (!stat) return null;
    if (stat.isSymbolicLink()) throw new Error(`symlink in context path: ${relative}`);
  }
  const stat = fs.statSync(current);
  if (!stat.isFile() || stat.size > MAX_FILE_BYTES) throw new Error(`context file is not a bounded regular file: ${relative}`);
  return current;
}

export function contextReport(bundleRoot, plan, { budgetBytes } = {}) {
  if (budgetBytes !== undefined && (!Number.isSafeInteger(budgetBytes) || budgetBytes < 1)) throw new Error('budgetBytes must be a positive safe integer');
  const root = fs.realpathSync(bundleRoot);
  const requests = (plan.loading?.selections ?? []).flatMap((entry) => [...entry.skills, ...entry.references].map((target) => ({ target, selectedBy: entry.id })));
  for (const target of plan.skills ?? []) if (!requests.some((entry) => entry.target === target)) requests.push({ target, selectedBy: 'module:configured-skills' });
  if (requests.length > MAX_TARGETS) throw new Error(`context report supports at most ${MAX_TARGETS} selections`);
  const files = new Map();
  const missing = [];
  const packages = new Map();
  function packageRoot(pkg) {
    if (packages.has(pkg)) return packages.get(pkg);
    for (const prefix of ['', `packages/${pkg}/`, `plugins/${pkg}/`]) {
      for (const manifest of ['capability.json', 'source-capability.json']) {
        const file = localFile(root, `${prefix}${manifest}`);
        if (file && JSON.parse(fs.readFileSync(file, 'utf8')).name === pkg) {
          const result = { prefix, generated: manifest === 'source-capability.json' };
          packages.set(pkg, result);
          return result;
        }
      }
    }
    packages.set(pkg, null);
    return null;
  }
  function record(relative, target, selectedBy) {
    const file = localFile(root, relative);
    if (!file) { missing.push({ target, reason: 'file not present in this bundle' }); return; }
    if (files.has(relative)) {
      const entry = files.get(relative);
      entry.selections++;
      if (!entry.selectedBy.includes(selectedBy)) entry.selectedBy.push(selectedBy);
      return;
    }
    const content = fs.readFileSync(file);
    if (content.length > MAX_FILE_BYTES) throw new Error(`context file exceeds read limit: ${relative}`);
    files.set(relative, { target, path: relative, bytes: content.length,
      sha256: createHash('sha256').update(content).digest('hex'),
      estimatedTokens: Math.ceil(content.length / 4), selections: 1, selectedBy: [selectedBy] });
  }
  for (const { target, selectedBy } of requests) {
    const match = targetPattern.exec(target);
    if (!match) throw new Error(`invalid context target: ${target}`);
    const [, pkg, name, reference] = match;
    const location = packageRoot(pkg);
    if (!location) { missing.push({ target, reason: 'package not present in this bundle' }); continue; }
    const { prefix, generated } = location;
    const plain = `skills/${name}${reference ?? '/SKILL.md'}`;
    const namespaced = `skills/${pkg}-${name}${reference ?? '/SKILL.md'}`;
    let relative = plain;
    if (generated) relative = localFile(root, `${prefix}${namespaced}`) ? namespaced : plain;
    else if (!reference && name.startsWith('workflow-')) relative = `workflows/${name.slice('workflow-'.length)}.md`;
    record(`${prefix}${relative}`, target, selectedBy);
    if (!reference) record(`${prefix}AGENTS.md`, `${pkg}:instructions`, selectedBy);
  }
  const entries = [...files.values()];
  const bytes = entries.reduce((sum, entry) => sum + entry.bytes, 0);
  const repeatedSelections = entries.reduce((sum, entry) => sum + entry.selections - 1, 0);
  const hashes = new Map();
  for (const entry of entries) hashes.set(entry.sha256, [...(hashes.get(entry.sha256) ?? []), entry.path]);
  return {
    formatVersion: 1, scope: 'selected-guidance-files; not live session usage',
    complete: missing.length === 0, files: entries, missing,
    totals: { files: entries.length, bytes, estimatedTokens: entries.reduce((sum, entry) => sum + entry.estimatedTokens, 0), repeatedSelections },
    identicalContent: [...hashes.values()].filter((paths) => paths.length > 1),
    estimator: 'ceil(UTF-8 bytes / 4) per unique path; heuristic, not a tokenizer',
    budget: { bytes: budgetBytes ?? null, status: budgetBytes === undefined ? 'not-configured' : bytes > budgetBytes ? 'exceeded' : missing.length ? 'unknown' : 'within', action: 'report-only; no guidance omitted' },
    sessionUsage: { currentTokens: null, capacityTokens: null, status: 'unknown; no host observation supplied' },
    limits: 'Measures package resources, not host-loaded overlay copies. Includes selected entry files, explicit references and required package instructions once per path. Excludes unloaded links, host skill-discovery metadata, tool schemas, conversation and tool output. Missing files make totals a lower bound.'
  };
}
