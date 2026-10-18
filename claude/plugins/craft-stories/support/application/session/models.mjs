import { decide } from '../engine-client.mjs';
export const modelAdvice = (input, project) => decide('model-advice', { input, project });
export const capabilityAdvice = (operation, risks) => decide('capability', { operation, risks });
