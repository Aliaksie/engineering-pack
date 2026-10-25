import { observe, withTelemetry } from './telemetry/observe.mjs';
import { decide } from './engine-client.mjs';
import { reviewSnapshot } from '../platform/index.mjs';
export const snapshot = request => withTelemetry(request.root, {}, () => observe('review.completed', () => decide('review-snapshot', { snapshot: reviewSnapshot(request) }), () => ({ phase: 'prepared' }), { failureCode:'REVIEW_INVALID' }));
export const validateReview = (snapshot, review) => observe('review.completed', () => decide('review-validate', { snapshot, review }), () => ({ phase: 'validated' }), { failureCode:'REVIEW_INVALID' });
export const publicationPlan = (snapshot, review, target) => observe('review.publication-planned', () => decide('review-publication', { snapshot, review, target }), () => ({ phase:'publication-planned' }), { failureCode:'REVIEW_INVALID' });
