import { decide } from '../engine-client.mjs';
import { contextFiles } from '../../platform/index.mjs';
export function contextReport(bundleRoot, plan, { budgetBytes } = {}) {
  const requests = decide('context-requests', { plan });
  return decide('context-summary', { ...contextFiles(bundleRoot, requests), budgetBytes });
}
