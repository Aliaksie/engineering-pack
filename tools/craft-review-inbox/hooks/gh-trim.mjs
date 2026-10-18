import { outputAdvice } from '../../../integrations/inbox-output.mjs';
export const evaluate = event => outputAdvice(event);
