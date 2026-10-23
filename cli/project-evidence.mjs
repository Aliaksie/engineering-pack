#!/usr/bin/env node
import path from 'node:path';
import { projectEvidence } from '../application/project-evidence.mjs';

const args = process.argv.slice(2);
if (!args.length || args.includes('--help')) {
  console.log('project-evidence version|refresh|status|query|backup|rebuild --enable --root <repository> [--member <id>=<absolute-root> ...] [--binary <absolute-path>] [--field id|name|path|target --value <exact-value> --limit 30 --after <id> --verify] [--max-files N --max-bytes N --max-output-bytes N --max-duration-ms N] [--json]');
} else {
  try {
    const operation = args.shift(), options = {}, members = [], flags = new Set();
    while (args.length) {
      const key = args.shift();
      if (['--enable', '--verify', '--json'].includes(key)) flags.add(key);
      else if (key === '--member' && args.length) {
        const value = args.shift(), separator = value.indexOf('=');
        if (separator < 1) throw new Error('member must be id=absolute-root');
        members.push({ id: value.slice(0, separator), root: value.slice(separator + 1) });
      }
      else if (['--root', '--binary', '--field', '--value', '--limit', '--after', '--max-files', '--max-bytes', '--max-output-bytes', '--max-duration-ms'].includes(key) && args.length) options[key.slice(2)] = args.shift();
      else throw new Error(`unknown or incomplete option: ${key}`);
    }
    const limit = options.limit === undefined ? 30 : Number(options.limit);
    if (!Number.isInteger(limit) || limit < 1 || limit > 200) throw new Error('limit must be between 1 and 200');
    const budget = {};
    for (const [flag, field] of [['max-files', 'maxFiles'], ['max-bytes', 'maxBytes'], ['max-output-bytes', 'maxOutputBytes'], ['max-duration-ms', 'maxDurationMs']]) {
      if (options[flag] === undefined) continue;
      const value = Number(options[flag]);
      if (!Number.isSafeInteger(value) || value < 1) throw new Error(`${flag} must be a positive safe integer`);
      budget[field] = value;
    }
    const result = projectEvidence({ enabled: flags.has('--enable'), root: path.resolve(options.root ?? '.'), operation, binary: options.binary,
      input: { ...(members.length ? { members } : {}), ...budget, field: options.field, value: options.value, limit, after: options.after, verify: flags.has('--verify') } });
    if (flags.has('--json')) console.log(JSON.stringify(result));
    else if (result.results) {
      console.log(`${result.results.length} result(s); ${result.coverage.freshness}; inventory ${result.coverage.inventoryComplete ? 'complete' : 'partial'}`);
      for (const row of result.results) console.log(`${row.path}:${row.evidence.line}:${row.evidence.column} ${row.evidence.kind} ${row.evidence.name} [${row.verification ?? 'stored-snapshot'}]`);
      if (result.continuation) console.log(`Continue with --after ${result.continuation}`);
    } else console.log(JSON.stringify(result, null, 2));
  } catch (error) { console.error(`${error.code ?? 'PROJECT_EVIDENCE_ERROR'}: ${error.message}`); process.exitCode = 1; }
}
