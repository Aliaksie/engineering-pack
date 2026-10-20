#!/usr/bin/env node
import * as handler from '../hooks/observe.mjs';
import { runHook } from '../platform-runtime/hook.mjs';
import { prepareEvent, reuseRoutingHint, commitRoutingHint, hookFailure, hookDefault, flushTelemetry, observeHook, hookDiagnostic } from '../support/application/context-delivery/prepare.mjs';
await runHook(handler.evaluate, 'codex', {"name":"observe-turn-end","event":"turn-end","tool":"any","handler":"hooks/observe.mjs"}, handler.prepare ?? prepareEvent, { observational: handler.observational === true, reuse: reuseRoutingHint, commit: commitRoutingHint, failure: hookFailure, default: hookDefault, flush: flushTelemetry, run: (event, callback) => observeHook({ ...event, packageName: "craft-stories" }, callback), diagnostic: hookDiagnostic });
