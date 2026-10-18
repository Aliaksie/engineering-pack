// Package entry point; exported imports resolve to the bundled application.
import { evaluateReviewPublication } from '../../../application/review.mjs';
export const evaluate = event => evaluateReviewPublication(event);
