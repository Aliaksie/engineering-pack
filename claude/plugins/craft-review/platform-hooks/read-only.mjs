#!/usr/bin/env node
import * as handler from '../hooks/read-only.mjs';
import { runHook } from '../platform-runtime/hook.mjs';
import { prepareEvent, reuseRoutingHint, commitRoutingHint, hookFailure, hookDefault, flushTelemetry, observeHook, hookDiagnostic } from '../support/application/context-delivery/prepare.mjs';
await runHook(handler.evaluate, 'claude', {"name":"read-only","event":"before-tool","tool":"shell","handler":"hooks/read-only.mjs"}, handler.prepare ?? prepareEvent, { observational: handler.observational === true, reuse: reuseRoutingHint, commit: commitRoutingHint, failure: hookFailure, default: hookDefault, flush: flushTelemetry, run: observeHook, diagnostic: hookDiagnostic });
