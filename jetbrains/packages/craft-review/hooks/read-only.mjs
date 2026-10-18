// Package entry point; exported imports resolve to the bundled application.
import { evaluateReviewPublication } from '../support/application/review.mjs';
export const evaluate = event => evaluateReviewPublication(event);
