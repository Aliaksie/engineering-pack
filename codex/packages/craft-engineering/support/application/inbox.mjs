import { decide } from './engine-client.mjs';
import { deriveInbox } from '../integrations/review-inbox.mjs';
export const derive = (pr, options) => deriveInbox(pr, options, facts => decide('inbox-status', facts));
