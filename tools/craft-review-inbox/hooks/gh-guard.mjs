import { commandEffects } from '../../../integrations/inbox-guard.mjs';
import { decide } from '../../../application/engine-client.mjs';
export const evaluate = event => decide('inbox-policy', commandEffects(event));
