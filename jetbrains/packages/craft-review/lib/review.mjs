import { spawnSync } from 'node:child_process';

const SHA = /^[0-9a-f]{40}(?:[0-9a-f]{24})?$/;
function git(root, args) {
  const result = spawnSync('git', ['-c', 'core.fsmonitor=false', '-C', root, ...args], { encoding: 'utf8', timeout: 15000, maxBuffer: 16 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`Git snapshot failed: ${result.stderr?.trim() || result.error?.message || result.status}`);
  return result.stdout;
}
export function snapshot({ root, base, head, provider = 'local', repository = 'local', changeId = null }) {
  if (!['local', 'github', 'gitlab'].includes(provider)) throw new Error('unsupported review provider');
  if (provider !== 'local' && !/^[1-9][0-9]*$/.test(String(changeId))) throw new Error('remote change id required');
  if (typeof repository !== 'string' || !repository.trim()) throw new Error('repository identity required');
  for (const ref of [base, head]) if (typeof ref !== 'string' || !ref || ref.startsWith('-')) throw new Error('explicit base and head references required');
  const baseSha = git(root, ['rev-parse', '--verify', '--end-of-options', `${base}^{commit}`]).trim();
  const headSha = git(root, ['rev-parse', '--verify', '--end-of-options', `${head}^{commit}`]).trim();
  const mergeBase = git(root, ['merge-base', baseSha, headSha]).trim();
  const files = git(root, ['diff', '--name-only', '-z', '--no-renames', mergeBase, headSha, '--']).split('\0').filter(Boolean);
  const diff = git(root, ['diff', '--no-ext-diff', '--no-textconv', '--no-color', '--no-renames', '--unified=3', mergeBase, headSha, '--']);
  return { formatVersion: 1, provider, repository, changeId, baseSha, headSha, mergeBase, files, diff,
    verification: 'source snapshot only; no project code, tests or model executed' };
}
export function validateSnapshot(value) {
  if (value?.formatVersion !== 1 || !['local', 'github', 'gitlab'].includes(value.provider) || !SHA.test(value.baseSha) || !SHA.test(value.headSha)
    || !SHA.test(value.mergeBase) || typeof value.repository !== 'string' || !Array.isArray(value.files) || typeof value.diff !== 'string') throw new Error('invalid review snapshot');
  if (value.provider !== 'local' && (!/^[1-9][0-9]*$/.test(String(value.changeId)) || !value.repository.trim())) throw new Error('remote review identity required');
  if (new Set(value.files).size !== value.files.length || value.files.some((file) => typeof file !== 'string' || !file || file.startsWith('/') || file.includes('\\') || file.split('/').includes('..'))) throw new Error('invalid changed file inventory');
  return value;
}
export function validateReview(input, review) {
  validateSnapshot(input);
  if (review?.baseSha !== input.baseSha || review?.headSha !== input.headSha) throw new Error('review revisions do not match snapshot; regenerate stale findings');
  if (typeof review.summary !== 'string' || !review.summary.trim() || !Array.isArray(review.findings) || !Array.isArray(review.verification)
    || !review.verification.length || review.verification.some((entry) => typeof entry !== 'string' || !entry.trim())) throw new Error('review summary, findings and verification required');
  if (!['approve', 'request-changes', 'needs-verification'].includes(review.recommendation)) throw new Error('review recommendation must be approve, request-changes or needs-verification');
  if (review.context !== undefined && (!Array.isArray(review.context) || review.context.some((entry) => typeof entry !== 'string' || !entry.trim()))) throw new Error('review context must be an array of nonempty strings');
  const ids = new Set();
  const severityCounts = { high: 0, medium: 0, low: 0 };
  for (const finding of review.findings) {
    if (!finding || typeof finding.id !== 'string' || !finding.id || ids.has(finding.id)) throw new Error('finding IDs must be unique');
    ids.add(finding.id);
    if (typeof finding.severity !== 'string' || !Object.hasOwn(severityCounts, finding.severity) || !input.files.includes(finding.path)
      || !Number.isSafeInteger(finding.line) || finding.line < 1 || !['old', 'new'].includes(finding.side)) throw new Error('finding location/severity is invalid');
    for (const field of ['summary', 'evidence']) if (typeof finding[field] !== 'string' || !finding[field].trim()) throw new Error(`finding ${field} required`);
    severityCounts[finding.severity]++;
  }
  if (review.recommendation === 'approve' && (severityCounts.high || severityCounts.medium)) throw new Error('approve recommendation conflicts with high or medium findings');
  return { valid: true, headSha: input.headSha, findings: review.findings.length, recommendation: review.recommendation, severityCounts,
    limitation: 'Structural validation only. Remote head, line applicability and semantic claims require publisher/reviewer verification.' };
}

export function publicationPlan(input, review, { currentHead, currentBase, authorized = false } = {}) {
  validateReview(input, review);
  if (authorized !== true) throw new Error('publication requires explicit user or job authorization');
  if (currentHead !== input.headSha || currentBase !== input.baseSha) throw new Error('remote base or head changed; do not publish stale findings');
  if (input.provider === 'local' || input.changeId === null) throw new Error('remote review identity required for publication');
  return { provider: input.provider, repository: input.repository, changeId: input.changeId, headSha: input.headSha,
    summary: review.summary, recommendation: review.recommendation, ...(review.context !== undefined ? { context: review.context } : {}),
    findings: review.findings, verification: review.verification, action: 'comment-only', execution: 'not-published' };
}
