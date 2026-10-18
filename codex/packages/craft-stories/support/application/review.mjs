import { publishingCommand } from '../integrations/review-commands.mjs';
import { decide } from './engine-client.mjs';
import { reviewSnapshot } from '../platform/index.mjs';
export const snapshot = request => decide('review-snapshot', { snapshot: reviewSnapshot(request) });
export const validateReview = (snapshot, review) => decide('review-validate', { snapshot, review });
export const publicationPlan = (snapshot, review, target) => decide('review-publication', { snapshot, review, target });

export const evaluateReviewPublication = event => decide('review-publication-policy', {
  readOnly: event.project?.review?.readOnly === true, commandExecution: event.tool === 'shell', publishes: publishingCommand(event.command) });
