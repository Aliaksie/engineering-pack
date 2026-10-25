import { telemetryReport } from '../application/telemetry/report.mjs';
import { captureTelemetry, readTelemetryCapture, forgetTelemetryCapture } from '../application/telemetry/capture.mjs';
import { withTelemetry, observe } from '../application/telemetry/observe.mjs';
export function execute(action, options, root) {
  return withTelemetry(root, { session: options.session, host: options.host }, () => observe('cli.command', () => executeRequest(action,options,root), () => ({ command:action }), { failureCode:'INVALID_INPUT', reuse:true }));
}
function executeRequest(action, options, root) {
  if (action === 'telemetry-capture') return captureTelemetry(root, { session: options.session });
  if (action === 'telemetry-forget') return forgetTelemetryCapture(root, options.capture);
  if (action === 'telemetry-report') {
    if (options.capture && options.session) throw new Error('choose --capture or --session for telemetry-report');
    return options.capture ? readTelemetryCapture(root, options.capture) : telemetryReport(root, { session: options.session });
  }
  throw new Error('usage: cli.mjs <telemetry-capture|telemetry-forget|telemetry-report> [--root project] [options]');
}
