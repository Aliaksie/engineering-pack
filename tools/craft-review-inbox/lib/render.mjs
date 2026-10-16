import fs from 'node:fs';
import path from 'node:path';
import { atomicWrite, jsonFile, noSymlinks } from './io.mjs';

const cell = (value) => String(value ?? '—').replaceAll('|', '\\|').replace(/[\r\n]+/g, ' ');
export function states(config) {
  const result = [];
  for (const short of Object.keys(config.repos)) {
    const directory = path.join(config.state, short);
    noSymlinks(directory);
    if (!fs.existsSync(directory)) continue;
    for (const name of fs.readdirSync(directory).filter((name) => /^pr_[1-9][0-9]*\.json$/.test(name)).sort()) result.push(jsonFile(path.join(directory, name)));
  }
  return result.sort((a, b) => String(a.repo).localeCompare(String(b.repo)) || a.number - b.number);
}
export function render(config, { now = new Date().toISOString(), failures = [] } = {}) {
  const all = states(config);
  const drafts = all.filter((state) => state.is_draft && state.status !== 'CLOSED_OR_MERGED');
  const open = all.filter((state) => !state.is_draft && state.status !== 'CLOSED_OR_MERGED');
  const sections = [
    ['DRAFT_READY', 'Ready to post — waiting on your word'], ['ANSWERED', 'Someone replied'],
    ['FIXED_PENDING_VERIFY', 'Claimed fixed — verify'], ['RE_REVIEW_NEEDED', 'Head moved past review'],
    ['NEW', 'Never reviewed'], ['STALE_MINE', 'Silent too long'], ['AWAITING_AUTHOR', 'Waiting on author'], ['CLEAN', 'No outstanding action']
  ];
  const warning = failures.length ? `\n> Scan incomplete for: ${failures.map(cell).join(', ')}. Stored rows may be stale.\n` : '';
  let inbox = `# PR Loop — INBOX\n\nScanned ${now}; ${open.length} reviewable PR(s); ${drafts.length} tracked draft(s) hidden. No remote action was performed.\n${warning}`;
  for (const [status, title] of sections) {
    const rows = open.filter((state) => state.status === status);
    if (!rows.length) continue;
    inbox += `\n## ${title} (${rows.length})\n\n`;
    for (const state of rows) inbox += `- **${cell(state.repo)} #${state.number}** — ${cell(state.title)} · ${cell(state.author)} · ${cell(state.url)}${state.local_event_pending_scan ? ` · local ${cell(state.local_event_pending_scan)} recorded; rescan to reconcile remote status` : ''}\n`;
  }
  let ledger = `# PR Loop — history ledger\n\nGenerated from local state; do not hand-edit. Rendered ${now}.\n${warning}\n| Repo | PR | Title | Status | Head | Reviewed | Posted |\n|---|---|---|---|---|---|---|\n`;
  for (const state of all) ledger += `| ${cell(state.repo)} | #${state.number} | ${cell(state.title)} | ${cell(state.status)}${state.is_draft ? ' (draft; excluded from agenda)' : ''} | ${cell(state.head_sha)} | ${cell(state.reviewed_at)} | ${cell(state.posted_at)} |\n`;
  atomicWrite(path.join(config.home, 'ledger.md'), ledger);
  atomicWrite(path.join(config.home, 'INBOX.md'), inbox);
  return { open: open.length, drafts: drafts.length, total: all.length, failures };
}
