import fs from 'node:fs';
import path from 'node:path';
import { hookMapping } from '../../hosts/registry.mjs';
import { nativeTargets } from '../native-release.mjs';

// Hook commands run the package's native runtime through a generated launcher: `bin/craft`
// (POSIX sh, incl. Git Bash on Windows) and `bin/craft.cmd` (cmd and PowerShell). The
// launchers only select the bundled executable; without a working one they keep shell policy
// closed and say once, when a session starts, that the package is inactive.
const UNAVAILABLE = 'bundled native runtime unavailable';
// Bash (incl. Git Bash and macOS /bin/sh) names the platform itself; other shells ask uname once.
const PLATFORMS = { 'x86_64-unknown-linux-gnu': ["'Linux x86_64'", 'linux*:x86_64'], 'aarch64-unknown-linux-gnu': ["'Linux aarch64'", 'linux*:aarch64'],
  'aarch64-apple-darwin': ["'Darwin arm64'", 'darwin*:arm64'],
  'x86_64-pc-windows-msvc': ["MINGW*' x86_64'", "MSYS*' x86_64'", "CYGWIN*' x86_64'", 'msys:x86_64', 'cygwin:x86_64'] };

export const hookArguments = (platform, definition, item) =>
  ['hook', platform, definition.event, '--package', item.manifest.name, '--handler', definition.handler, '--scope', definition.tool === 'shell' ? 'shell' : 'any'];

// The wire answer of a hook whose runtime is unavailable: `{stdout, stderr, exitCode}`.
// Every package says it is inactive when a session starts: shown to the person where the host
// displays a session-start notice, and to the model, which hears it again after compaction.
// Other hooks stay quiet; repeating it on every tool call or prompt is only noise.
export function fallback(platform, definition, item) {
  const mapping = hookMapping(platform, definition);
  if (!mapping) return { stdout: '', stderr: '', exitCode: 0 };
  const denied = definition.tool === 'shell' && definition.event === 'before-tool' && mapping.deny;
  const reason = `Policy could not be evaluated: ${UNAVAILABLE}`;
  const shown = definition.event === 'session-start' && mapping.notice;
  const supported = nativeTargets.map(({ platform: os, arch }) => `${os} ${arch}`).join(', ');
  const inactive = `${item.manifest.name} is inactive: its bundled native runtime is missing or cannot start here (built for ${supported}). Its hooks, routing, checkpoints and telemetry are off; reinstall it or use a supported platform.`;
  const context = ['session-start', 'after-compact'].includes(definition.event) && mapping.context
    ? (shown || definition.event === 'after-compact' ? inactive : `${inactive} Tell the user once.`) : '';
  const notice = shown ? { systemMessage: inactive } : {};
  // Cursor blocks a permission hook whose answer does not match its schema, so a hook with
  // nothing to say still answers there; other hosts keep their own flow on empty output.
  const neutral = platform !== 'cursor' ? undefined : definition.event === 'before-tool' ? { permission: 'allow' }
    : definition.event === 'before-prompt' ? { continue: true } : undefined;
  if (definition.handler === 'observe' && !context) return { stdout: neutral ? `${JSON.stringify(neutral)}\n` : '', stderr: '', exitCode: 0 };
  if (platform === 'claude' && denied) return { stdout: '', stderr: `${reason}\n`, exitCode: 2 };
  let output;
  // Cursor and Copilot get the answers the runtime gives them: Cursor requires a permission,
  // Copilot keeps its own permission flow on empty output.
  if (platform === 'cursor') output = denied ? { permission: 'deny', user_message: reason, agent_message: reason }
    : neutral ?? (context ? { additional_context: context } : undefined);
  else if (platform === 'copilot') output = denied ? { permissionDecision: 'deny', permissionDecisionReason: reason }
    : definition.event !== 'before-tool' && context ? { additionalContext: context } : undefined;
  else if (platform === 'gemini') output = denied ? { decision: 'deny', reason } : context ? { ...notice, hookSpecificOutput: { hookEventName: mapping.name, additionalContext: context } } : undefined;
  else output = denied ? { hookSpecificOutput: { hookEventName: mapping.name, permissionDecision: 'deny', permissionDecisionReason: reason } }
    : context ? { ...notice, hookSpecificOutput: { hookEventName: mapping.name, additionalContext: context } } : undefined;
  return { stdout: output ? `${JSON.stringify(output)}\n` : '', stderr: '', exitCode: 0 };
}

const executable = target => `support/native-bin/${target}/native-bin/craft${target.includes('windows') ? '.exe' : ''}`;

// A registered hook command runs the bundled runtime from the host's own shell, which picks it
// with builtins only: Bash (incl. Git Bash and macOS /bin/sh) and zsh name the platform, and
// Windows has one target for PowerShell. Without a match (e.g. dash), with no executable or
// when the runtime exits above 2, the command hands over to the launcher, which then answers
// without starting the runtime again: `CRAFT_NATIVE_FAILED` says this one already failed.
export function shellHook(root, args) {
  const cases = nativeTargets.map(({ target }) => `${PLATFORMS[target].filter(pattern => !/[ ']/.test(pattern)).join('|')}) b="$r/${executable(target)}" ;;`);
  return `r="${root}"; case "$OSTYPE:\${HOSTTYPE:-$CPUTYPE}" in ${cases.join(' ')} *) b= ;; esac; `
    + '[ -n "$CRAFT_NATIVE_BINARY" ] && b=$CRAFT_NATIVE_BINARY; '
    + `if [ -x "$b" ]; then CRAFT_PACKAGE_ROOT="$r" "$b" ${args}; s=$?; [ "$s" -le 2 ] && exit "$s"; CRAFT_NATIVE_FAILED=1; export CRAFT_NATIVE_FAILED; fi; `
    + `exec "$r/bin/craft" ${args}`;
}
export function powershellHook(root, args) {
  const windows = nativeTargets.find(({ platform }) => platform === 'win32').target;
  return `$r = "${root}"; $b = if ($env:CRAFT_NATIVE_BINARY) { $env:CRAFT_NATIVE_BINARY } else { "$r\\${executable(windows).replaceAll('/', '\\')}" }; `
    // An executable that cannot start throws instead of setting an exit code. A .NET check, not
    // Test-Path: that cmdlet autoloads a module, about 100 ms on every hook call.
    + `if ([System.IO.File]::Exists($b)) { $env:CRAFT_PACKAGE_ROOT = $r; $s = 3; try { & $b ${args}; $s = $LASTEXITCODE } catch {}; if ($s -ge 0 -and $s -le 2) { exit $s }; $env:CRAFT_NATIVE_FAILED = '1' }; `
    + `& "$r\\bin\\craft.cmd" ${args}; exit $LASTEXITCODE`;
}
function sh(answers) {
  const cases = nativeTargets.map(({ target }) => `  ${PLATFORMS[target].join('|')}) binary="$here/../${executable(target)}" ;;`);
  const quote = text => `'${text.replaceAll("'", "'\\''")}'`;
  const fallbacks = answers.map(({ key, wire }) => `  ${quote(key)}) ${wire.stdout ? `printf '%s\\n' ${quote(wire.stdout.trimEnd())}; ` : ''}${wire.stderr ? `printf '%s\\n' ${quote(wire.stderr.trimEnd())} >&2; ` : ''}exit ${wire.exitCode} ;;`);
  return `#!/bin/sh
# Generated by the export build. Runs this package's bundled native runtime.
here=\${0%/*}
# Commands that account installed guidance (the status line) read this package's root.
CRAFT_PACKAGE_ROOT=$here/..
export CRAFT_PACKAGE_ROOT
if [ -n "$OSTYPE" ] && [ -n "$HOSTTYPE" ]; then platform="$OSTYPE:$HOSTTYPE"; else platform=$(uname -sm 2>/dev/null); fi
case $platform in
${cases.join('\n')}
  *) binary= ;;
esac
# A development build, as for the other runtime entry points.
[ -n "$CRAFT_NATIVE_BINARY" ] && binary=$CRAFT_NATIVE_BINARY
# The hook command already ran it and it failed: answer without starting it again.
[ -n "$CRAFT_NATIVE_FAILED" ] && binary=
if [ -n "$binary" ] && [ -f "$binary" ]; then
  [ -x "$binary" ] || chmod u+x "$binary" 2>/dev/null
  "$binary" "$@"
  status=$?
  # The runtime exits 0-2; anything else means it could not start or crashed.
  [ "$status" -le 2 ] && exit "$status"
fi
# No working bundled runtime: deny shell policy, say once at session start that it is inactive.
case "$1 $2 $3 $7" in
${fallbacks.join('\n')}
  'hook '*) exit 0 ;;
esac
printf '%s\\n' 'craft: ${UNAVAILABLE}' >&2
exit 1
`;
}
function cmd(answers) {
  const windows = nativeTargets.find(({ platform }) => platform === 'win32').target;
  const lines = answers.map(({ key }, index) => `if "%~1 %~2 %~3 %~7"=="${key}" goto answer${index}`);
  const labels = answers.flatMap(({ wire }, index) => [`:answer${index}`,
    ...(wire.stdout ? [`echo ${wire.stdout.trimEnd()}`] : []), ...(wire.stderr ? [`>&2 echo ${wire.stderr.trimEnd()}`] : []), `exit /b ${wire.exitCode}`]);
  return [`@echo off`, `rem Generated by the export build. Runs this package's bundled native runtime.`, `setlocal`, `set "CRAFT_PACKAGE_ROOT=%~dp0.."`,
    `if defined CRAFT_NATIVE_BINARY (set "binary=%CRAFT_NATIVE_BINARY%") else set "binary=%~dp0..\\${executable(windows).replaceAll('/', '\\')}"`, `if defined CRAFT_NATIVE_FAILED goto unavailable`, `if not exist "%binary%" goto unavailable`, `"%binary%" %*`,
    `if %ERRORLEVEL% GEQ 0 if %ERRORLEVEL% LEQ 2 exit /b %ERRORLEVEL%`,
    `:unavailable`, ...lines, `if "%~1"=="hook" exit /b 0`, `>&2 echo craft: ${UNAVAILABLE}`, `exit /b 1`, ...labels, ''].join('\r\n');
}
// Both launchers for a package that registers hooks on this host: `[[path, text]]`.
export function launchers(item, platform) {
  const answers = item.manifest.hooks.filter(definition => hookMapping(platform, definition)).map(definition => {
    const args = hookArguments(platform, definition, item);
    return { key: [args[0], args[1], args[2], args[6]].join(' '), wire: fallback(platform, definition, item) };
  }).filter(({ wire }) => wire.stdout || wire.stderr || wire.exitCode)
    .filter((answer, index, all) => all.findIndex(other => other.key === answer.key) === index);
  return [['bin/craft', sh(answers)], ['bin/craft.cmd', cmd(answers)]];
}
export function emitLaunchers(item, destination, platform) {
  for (const [name, text] of launchers(item, platform)) {
    fs.mkdirSync(path.join(destination, 'bin'), { recursive: true });
    fs.writeFileSync(path.join(destination, name), text, { mode: 0o755 });
    fs.chmodSync(path.join(destination, name), 0o755);
  }
}
