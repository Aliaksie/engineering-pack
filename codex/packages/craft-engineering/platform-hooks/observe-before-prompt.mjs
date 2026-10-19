#!/usr/bin/env node
import * as handler from '../hooks/observe.mjs';
import { runHook } from '../platform-runtime/hook.mjs';
import { prepareEvent, reuseRoutingHint, commitRoutingHint, hookFailure, hookDefault, flushTelemetry, observeHook, hookDiagnostic } from '../support/application/context-delivery/prepare.mjs';
await runHook(handler.evaluate, 'codex', {"name":"observe-before-prompt","event":"before-prompt","tool":"any","handler":"hooks/observe.mjs"}, handler.prepare ?? prepareEvent, { observational: handler.observational === true, reuse: reuseRoutingHint, commit: commitRoutingHint, failure: hookFailure, default: hookDefault, flush: flushTelemetry, run: observeHook, diagnostic: hookDiagnostic });
