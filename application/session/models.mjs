import { observe } from '../telemetry/observe.mjs';
import { decide } from '../engine-client.mjs';
export const modelAdvice = (input, project) => observe('model.advice', () => decide('model-advice', { input, project }), result => ({ decision: result.decision, capabilityClass: result.capabilityClass ?? input.capabilityClass }));
export const capabilityAdvice = (operation, risks) => decide('capability', { operation, risks });
