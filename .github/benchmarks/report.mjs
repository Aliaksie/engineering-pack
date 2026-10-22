export function assessReports(reports, revision) {
  const failures = [], diagnostics = [];
  for (const name of ['components', 'filesystem', 'turn']) {
    const report = reports[name];
    if (!report || report.loadError || !Array.isArray(report.samples) || !report.samples.length) {
      failures.push(`${name}: missing or unreadable samples`);
      continue;
    }
    if (report.revision !== revision) failures.push(`${name}: unexpected measured revision`);
    if (report.failures !== 0) failures.push(`${name}: process or assertion failures`);
    if (name !== 'turn') {
      if (report.samples.some(sample => sample.correct !== true)) failures.push(`${name}: failed sample checks`);
    } else {
      if (report.exportsUnchanged !== true) failures.push('turn: exported source changed during measurement');
      if (report.samples.some(sample => sample.wireMatches !== true)) failures.push('turn: wire output changed');
      if (report.samples.some(sample => sample.stateMatches !== true)) failures.push('turn: selected task state changed');
      const lossTurns = report.samples.filter(sample => sample.dropped > 0).length;
      const mismatches = report.samples.filter(sample => sample.telemetryMatches !== true).length;
      if (report.reportedDrops || mismatches) diagnostics.push(`Telemetry: ${report.reportedDrops} reported drops in ${lossTurns} turns; ${mismatches} comparisons differ from the first-sample reference. Loss in that reference can inflate mismatch counts.`);
    }
    const expected = report.protocol?.rounds;
    const summaries = Object.values(report.summary ?? {});
    if (!Number.isInteger(expected) || expected < 1 || !summaries.length || summaries.some(value => value.n !== expected)) {
      failures.push(`${name}: incomplete repeated measurements`);
    }
  }
  return { failures, diagnostics, timingPolicy: 'Diagnostic only; no elapsed-time threshold gates.' };
}

export function formatSummary(assessment, reports, revision) {
  const lines = ['## Portable performance diagnostics', '', `Measured revision: ${revision}`, '',
    assessment.failures.length ? '**Correctness checks failed.**' : '**Correctness checks passed.**', '',
    '| Synthetic turn | Median ms | p95 ms |', '|---|---:|---:|'];
  for (const [name, stats] of Object.entries(reports.turn?.summary ?? {})) {
    lines.push(`| ${name} | ${Number(stats.median).toFixed(1)} | ${Number(stats.p95).toFixed(1)} |`);
  }
  lines.push('', assessment.timingPolicy, '',
    'Synthetic manual dispatch; excludes actual tools, models and native host scheduling. Runner hardware/cache differences limit comparisons.', '');
  for (const message of [...assessment.failures, ...assessment.diagnostics]) lines.push(`- ${message}`);
  if (!assessment.diagnostics.length) lines.push('- No retained telemetry mismatch/drop diagnostic reported; this does not prove complete history.');
  return lines.join('\n') + '\n';
}
