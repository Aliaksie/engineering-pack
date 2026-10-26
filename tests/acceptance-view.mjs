import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { nativeCommand } from './craft.mjs';

// `craft acceptance-view` over one input object ({contract, evidence, current, probes, focus,
// blockers}), each document written to its own file as a caller would keep them.
export function acceptanceView(view, input) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'craft-acceptance-view-'));
  try {
    const file = (name, value) => { const target = path.join(directory, `${name}.json`); fs.writeFileSync(target, JSON.stringify(value)); return target; };
    const argv = ['acceptance-view', '--root', directory, '--view', view, '--contract', file('contract', input.contract),
      '--acceptance-evidence', file('evidence', input.evidence), '--current-kind', input.current.kind];
    if (input.current.identity !== null) argv.push('--current-identity', input.current.identity);
    if (input.probes !== undefined) argv.push('--probes', file('probes', input.probes));
    if (input.focus !== undefined) argv.push('--focus', file('focus', input.focus));
    if (input.blockers) argv.push('--blockers');
    const outcome = nativeCommand(argv, { structured: true });
    return outcome.exitCode ? { ok: false, exitCode: outcome.exitCode, error: outcome.stderr.trimEnd() }
      : { ok: true, result: JSON.parse(outcome.stdout) };
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
}

// The engineering-contract digest that evidence records name, as the views report it.
export const contractDigest = contract => acceptanceView('qa', { contract, evidence: { formatVersion: 1, records: [] },
  current: { kind: 'environment', identity: null } }).result.contract;
