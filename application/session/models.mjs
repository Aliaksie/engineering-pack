import { observe, diagnostic } from '../telemetry/observe.mjs';
import { decide } from '../engine-client.mjs';
export const modelAdvice = (input, project) => observe('model.advice', () => { const result = decide('model-advice', { input, project }); if (result.decision === 'unavailable') diagnostic('model','MODEL_ADVICE_UNAVAILABLE'); return result; }, result => ({ decision: result.decision, capabilityClass: result.capabilityClass ?? input.capabilityClass }));
export const capabilityAdvice = (operation, risks) => decide('capability', { operation, risks });
