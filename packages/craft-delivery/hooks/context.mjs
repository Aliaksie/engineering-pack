// Package entry point; exported imports resolve to the bundled application.
import { evaluateContextDelivery } from '../../../application/context-delivery/prepare.mjs';
export const evaluate = event => evaluateContextDelivery(event);
